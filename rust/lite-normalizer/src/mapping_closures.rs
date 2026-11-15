// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `MappingClosures` (MappingClosures.java at ad86fa895): THE INCLUDE-ORDER FACTS of every mapping of a graph,
//! computed once per mapping over the mappings' SURFACES. A mapping's surface is the authored mapping plus the identity
//! sets every runtime's `JsonModelConnection` bindings synthesize into it (the cross-bake of the runtime's bindings
//! into the mapping surface), so an includer sees an included mapping's identity sets.
//!
//! Each accessor keeps the rule of the include walker it replaced, recursion shape included: the walkers differ from
//! one another (visible sets: a LATER include overrides an earlier one; operation sets per class: the LAST found wins;
//! roots: includes first, then the mapping's own), and those differences are the current semantics. The mapping's OWN
//! part is supplied by the caller (the pre-passed mapping synthesis consumes), the included part comes from here.
//!
//! RE-EXPRESSED (EF_CENSUS §4, §9.3), with the same answers:
//! - **E-local, not on the model.** Java memoizes the instance on `model.knowledge().derived`; it is written and read
//!   only inside E, so here it is a value of one `normalize` call ([`MappingClosures::new`] in
//!   `mapping_normalizer::normalize`), borrowing the index, read through [`ResolvedMapping::closures`] where Java asks
//!   `MappingClosures.of(model)`.
//! - **Lazy, unsynchronized.** Each fact is an `OnceCell` computed on first ask, as Java's null-checked fields are; a
//!   failure is not cached (Java's field stays null, so the next ask walks and throws again). Laziness is
//!   answer-bearing: `sets()` throws "Can't find mapping" for the mapping that asks, and the tolerant catch is per
//!   mapping.
//! - [`Closure`] is Java's inner class: a handle (the owner and the shared state), since the state cannot hold a
//!   reference to the map that stores it.
//! - `walkIds`' `first` map is dropped (`first.get(id) != cm` is always true where it is reached: each mapping is
//!   walked once, surfaces are memoized, no producer shares a set between two mappings' surfaces); never a derived
//!   `==`, which would suppress a genuine duplicate.
//!
//! [`ResolvedMapping::closures`]: crate::resolved_mapping::ResolvedMapping::closures

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use lite_error::Phase;
use lite_model::linked::{LinkedMap, empty, list};
use lite_model::names::set_id_of;
use lite_model::text::{contains, last_index_of};
use lite_model::{
    AssociationMapping, ClassMapping, ClassMappingInheritance, ClassMappingRelational, ClassMappingUnion,
    EnumerationMapping, JStr, LegacyMappingDefinition, MappingInclude, PropertyMapping, PropertyMappingJoin, cat,
};
use lite_names::ModelIndex;

use crate::association_synthesis;
use crate::error::Thrown;
use crate::resolved_mapping::ResolvedMapping;
use crate::store_substitution_rewrite;

/// An include naming a mapping the model has not: the model's error, in legend-engine's words (`PureModel.getMapping`).
fn missing_mapping(path: &JStr) -> Thrown {
    Thrown::model(Phase::Mapping, cat!("Can't find mapping '", path, "'"), None)
}

/// The include-order facts of one model's mappings, for one `normalize` call.
pub struct MappingClosures<'m> {
    model: &'m ModelIndex,
    surfaces: RefCell<HashMap<JStr, Arc<LegacyMappingDefinition>>>,
    closures: RefCell<HashMap<JStr, Rc<ClosureState>>>,
}

impl<'m> MappingClosures<'m> {
    /// `MappingClosures.of(model)` (:66), E-local: one per `normalize` call.
    pub fn new(model: &'m ModelIndex) -> MappingClosures<'m> {
        MappingClosures {
            model,
            surfaces: RefCell::new(HashMap::new()),
            closures: RefCell::new(HashMap::new()),
        }
    }

    /// The index the facts are over.
    pub fn model(&self) -> &'m ModelIndex {
        self.model
    }

    /// `MappingClosures.closure(mappingFqn)` (:72): the included part of `mapping_fqn`'s closure (its own part is the caller's
    /// mapping), one state per mapping.
    pub fn closure(&self, mapping_fqn: &JStr) -> Closure<'_> {
        let state = self
            .closures
            .borrow_mut()
            .entry(mapping_fqn.clone())
            .or_insert_with(|| Rc::new(ClosureState::new(mapping_fqn.clone())))
            .clone();
        Closure { owner: self, state }
    }

    /// `MappingClosures.surface(authored)` (:85): the SURFACE of the authored mapping -- itself plus the identity
    /// `ClassMapping.Relational` (`sourceUrl` set) for every class a runtime's JSON connection binds through it, unless
    /// the mapping already maps that class (user-authored wins). Memoized by the mapping's name.
    pub fn surface(&self, authored: &Arc<LegacyMappingDefinition>) -> Arc<LegacyMappingDefinition> {
        if let Some(s) = self.surfaces.borrow().get(&authored.qualified_name) {
            return s.clone();
        }
        let s = self.with_json_identity_sets(authored);
        self.surfaces
            .borrow_mut()
            .entry(authored.qualified_name.clone())
            .or_insert(s)
            .clone()
    }

    /// `MappingClosures.surfaceOf(path)` (:94).
    fn surface_of(&self, path: &JStr) -> Option<Arc<LegacyMappingDefinition>> {
        self.model.find_legacy_mapping(path).map(|m| self.surface(m))
    }

    /// `MappingClosures.withJsonIdentitySets(md)` (:98).
    fn with_json_identity_sets(&self, md: &Arc<LegacyMappingDefinition>) -> Arc<LegacyMappingDefinition> {
        let mut updated: Option<Vec<ClassMapping>> = None;
        for rd in self.model.runtimes() {
            if rd.json_connections.is_empty() || !rd.mappings.contains(&md.qualified_name) {
                continue;
            }
            for jmc in rd.json_connections.iter() {
                let class_fqn = &jmc.class_name;
                let current: &[ClassMapping] = updated.as_deref().unwrap_or(&md.class_mappings);
                if current.iter().any(|cm| cm.class_name() == class_fqn) {
                    continue; // user-authored class mapping wins
                }
                let out = updated.get_or_insert_with(|| md.class_mappings.to_vec());
                out.push(ClassMapping::ClassMappingRelational(Arc::new(ClassMappingRelational {
                    class_name: class_fqn.clone(),
                    set_id: None,
                    extends_set_id: None,
                    root: true,
                    main_table: None,
                    filter: None,
                    distinct: false,
                    group_by: empty(),
                    primary_key: empty(),
                    property_mappings: empty(),
                    source_url: Some(jmc.url.clone()),
                    property_target_sets: empty(),
                    aggregation: None,
                })));
            }
        }
        match updated {
            None => md.clone(),
            Some(cms) => Arc::new(md.with_class_mappings(list(cms))),
        }
    }
}

/// The state of one mapping's [`Closure`]: each fact computed on first ask by the walk it replaced, over surfaces.
struct ClosureState {
    fqn: JStr,
    mappings: OnceCell<Arc<[Arc<LegacyMappingDefinition>]>>,
    sets: OnceCell<LinkedMap<JStr, ClassMapping>>,
    /// `unions` and `inheritances`, built together (`union("")` builds both).
    ops: OnceCell<Ops>,
    roots: OnceCell<LinkedMap<JStr, ClassMapping>>,
    enums: OnceCell<Arc<[Arc<EnumerationMapping>]>>,
    relational_sets: OnceCell<HashMap<JStr, Vec<Arc<ClassMappingRelational>>>>,
    /// `pairEntries`, per class asked.
    pair_entries: RefCell<HashMap<JStr, Rc<PairEntries>>>,
}

/// The operation sets per class through the includes: `(unions, inheritances)`.
type Ops = (
    LinkedMap<JStr, Arc<ClassMappingUnion>>,
    LinkedMap<JStr, Arc<ClassMappingInheritance>>,
);

/// Per-pair association entries: source set id -> the routed Join PMs, in walk order.
pub type PairEntries = LinkedMap<JStr, Arc<[Arc<PropertyMappingJoin>]>>;

impl ClosureState {
    fn new(fqn: JStr) -> ClosureState {
        ClosureState {
            fqn,
            mappings: OnceCell::new(),
            sets: OnceCell::new(),
            ops: OnceCell::new(),
            roots: OnceCell::new(),
            enums: OnceCell::new(),
            relational_sets: OnceCell::new(),
            pair_entries: RefCell::new(HashMap::new()),
        }
    }
}

/// `MappingClosures.Closure` (:132, the inner class; its constructor :143): the included part of one mapping's closure.
#[derive(Clone)]
pub struct Closure<'a> {
    owner: &'a MappingClosures<'a>,
    state: Rc<ClosureState>,
}

impl<'a> Closure<'a> {
    /// The facts this closure is one of (Java's enclosing instance).
    pub fn owner(&self) -> &'a MappingClosures<'a> {
        self.owner
    }

    /// `MappingClosures.Closure.includes()` (:147).
    fn includes(&self) -> Arc<[Arc<MappingInclude>]> {
        self.owner
            .surface_of(&self.state.fqn)
            .map_or_else(empty, |m| m.includes.clone())
    }

    /// `MappingClosures.Closure.mappings()` (:153): the included mappings, depth-first in include order, each once (the mapping itself
    /// excluded).
    pub fn mappings(&self) -> &[Arc<LegacyMappingDefinition>] {
        if let Some(known) = self.state.mappings.get() {
            return known;
        }
        let mut out = Vec::new();
        let mut seen: HashSet<JStr> = HashSet::new();
        seen.insert(self.state.fqn.clone());
        for inc in self.includes().iter() {
            if let Some(m) = self.owner.surface_of(&inc.mapping_path) {
                self.walk_mappings(&m, &mut out, &mut seen);
            }
        }
        self.state.mappings.get_or_init(|| list(out))
    }

    /// `MappingClosures.Closure.walkMappings` (:168).
    fn walk_mappings(
        &self,
        md: &Arc<LegacyMappingDefinition>,
        out: &mut Vec<Arc<LegacyMappingDefinition>>,
        seen: &mut HashSet<JStr>,
    ) {
        if !seen.insert(md.qualified_name.clone()) {
            return;
        }
        out.push(md.clone());
        for inc in md.includes.iter() {
            if let Some(m) = self.owner.surface_of(&inc.mapping_path) {
                self.walk_mappings(&m, out, seen);
            }
        }
    }

    /// `MappingClosures.Closure.relationalSets(classFqn)` (:181): `class_fqn`'s Relational sets across [`Closure::mappings`], in that order
    /// then declaration order.
    pub fn relational_sets(&self, class_fqn: Option<&JStr>) -> Vec<Arc<ClassMappingRelational>> {
        let by_class = self.state.relational_sets.get_or_init(|| {
            let mut out: HashMap<JStr, Vec<Arc<ClassMappingRelational>>> = HashMap::new();
            for m in self.mappings() {
                for cm in m.class_mappings.iter() {
                    if let ClassMapping::ClassMappingRelational(rcm) = cm {
                        out.entry(rcm.class_name.clone()).or_default().push(rcm.clone());
                    }
                }
            }
            out
        });
        class_fqn.and_then(|c| by_class.get(c)).cloned().unwrap_or_default()
    }

    /// `MappingClosures.Closure.sets()` (:201): set ids visible through the includes, transitively -- a nearer mapping's set overrides a deeper
    /// one, a LATER include overrides an earlier one; an include's store substitutions apply to everything pulled
    /// through it (grandparents included). Loud for an include the model has not ("Can't find mapping").
    pub fn sets(&self) -> Result<&LinkedMap<JStr, ClassMapping>, Thrown> {
        if let Some(known) = self.state.sets.get() {
            return Ok(known);
        }
        let mut out = LinkedMap::new();
        self.walk_sets(&self.includes(), &mut out, &mut HashSet::new())?;
        Ok(self.state.sets.get_or_init(|| out))
    }

    /// `MappingClosures.Closure.walkSets` (:211).
    fn walk_sets(
        &self,
        incs: &[Arc<MappingInclude>],
        by_set_id: &mut LinkedMap<JStr, ClassMapping>,
        seen: &mut HashSet<JStr>,
    ) -> Result<(), Thrown> {
        for inc in incs {
            if !seen.insert(inc.mapping_path.clone()) {
                continue;
            }
            let included = self
                .owner
                .surface_of(&inc.mapping_path)
                .ok_or_else(|| missing_mapping(&inc.mapping_path))?;
            let mut local = LinkedMap::new();
            self.walk_sets(&included.includes, &mut local, seen)?;
            for cm in included.class_mappings.iter() {
                local.put(ResolvedMapping::id_of(cm), cm.clone());
            }
            if !inc.substitutions.is_empty() {
                local = LinkedMap::of(
                    local
                        .iter()
                        .map(|(k, v)| (k.clone(), store_substitution_rewrite::apply(v, &inc.substitutions))),
                );
            }
            for (k, v) in local.iter() {
                by_set_id.put(k.clone(), v.clone());
            }
        }
        Ok(())
    }

    /// The operation sets through the includes, built once for both lookups (`union("")`'s side effect).
    fn ops(&self) -> &Ops {
        self.state.ops.get_or_init(|| {
            let mut out = LinkedMap::new();
            let mut out_i = LinkedMap::new();
            for inc in self.includes().iter() {
                if let Some(m) = self.owner.surface_of(&inc.mapping_path) {
                    self.walk_ops(&m, &mut out, &mut out_i);
                }
            }
            (out, out_i)
        })
    }

    /// `MappingClosures.Closure.union(classFqn)` (:235): the Union operation set for `class_fqn` through the includes in the engine's order (an
    /// include's includes first, then its own sets; the LATER include after the earlier), the LAST found --
    /// `rootClassMappingByClass`'s `last()` (R1).
    pub fn union(&self, class_fqn: Option<&JStr>) -> Option<Arc<ClassMappingUnion>> {
        let (unions, _) = self.ops();
        class_fqn.and_then(|c| unions.get(c).cloned())
    }

    /// `MappingClosures.Closure.inheritance(classFqn)` (:252): the Inheritance operation set for `class_fqn`, the same last-wins rule.
    pub fn inheritance(&self, class_fqn: &JStr) -> Option<Arc<ClassMappingInheritance>> {
        let (_, inheritances) = self.ops();
        inheritances.get(class_fqn).cloned()
    }

    /// `MappingClosures.Closure.walkOps` (:257). An include the model has not contributes nothing here (`surfaceOf(..).ifPresent`).
    fn walk_ops(
        &self,
        md: &LegacyMappingDefinition,
        out: &mut LinkedMap<JStr, Arc<ClassMappingUnion>>,
        out_i: &mut LinkedMap<JStr, Arc<ClassMappingInheritance>>,
    ) {
        for inc in md.includes.iter() {
            if let Some(m) = self.owner.surface_of(&inc.mapping_path) {
                self.walk_ops(&m, out, out_i);
            }
        }
        for cm in md.class_mappings.iter() {
            match cm {
                ClassMapping::ClassMappingUnion(u) => {
                    out.put(u.class_name.clone(), u.clone());
                }
                ClassMapping::ClassMappingInheritance(ih) => {
                    out_i.put(ih.class_name.clone(), ih.clone());
                }
                _ => {}
            }
        }
    }

    /// `MappingClosures.Closure.duplicateIds()` (:275): set ids taken by more than one DISTINCT set across this mapping's closure (an include's
    /// includes first, then its own; then this mapping's own), each with the two mappings -- the engine's
    /// `collectAndValidateClassMappingIds` (R5). Not memoized (Java's is not).
    pub fn duplicate_ids(&self) -> Result<Vec<JStr>, Thrown> {
        let mut owner: LinkedMap<JStr, JStr> = LinkedMap::new();
        let mut dups = Vec::new();
        let mut seen: HashSet<JStr> = HashSet::new();
        self.walk_ids(&self.state.fqn, &mut owner, &mut dups, &mut seen)?;
        Ok(dups)
    }

    /// `MappingClosures.Closure.walkIds` (:284), its `first` map dropped (the module's note).
    fn walk_ids(
        &self,
        mapping_fqn: &JStr,
        owner: &mut LinkedMap<JStr, JStr>,
        dups: &mut Vec<JStr>,
        seen: &mut HashSet<JStr>,
    ) -> Result<(), Thrown> {
        if !seen.insert(mapping_fqn.clone()) {
            return Ok(());
        }
        let md = self
            .owner
            .surface_of(mapping_fqn)
            .ok_or_else(|| missing_mapping(mapping_fqn))?;
        for inc in md.includes.iter() {
            self.walk_ids(&inc.mapping_path, owner, dups, seen)?;
        }
        let mut own: HashSet<JStr> = HashSet::new();
        for cm in md.class_mappings.iter() {
            let id = ResolvedMapping::id_of(cm);
            match owner.get(&id) {
                Some(prev) if prev != mapping_fqn => {
                    dups.push(cat!(&id, " (", prev, ", ", mapping_fqn, ")"));
                }
                Some(_) => {}
                None => {
                    owner.put(id.clone(), mapping_fqn.clone());
                }
            }
            if !own.insert(id.clone()) {
                dups.push(cat!(&id, " (twice in ", mapping_fqn, ")"));
            }
        }
        Ok(())
    }

    /// `MappingClosures.Closure.roots()` (:312): ROOT class mapping per class through the includes -- deeper includes first, a nearer mapping
    /// overrides; the root is the `*` set or the class's sole set in that mapping.
    pub fn roots(&self) -> &LinkedMap<JStr, ClassMapping> {
        self.state.roots.get_or_init(|| {
            let mut out = LinkedMap::new();
            let mut seen: HashSet<JStr> = HashSet::new();
            for inc in self.includes().iter() {
                if seen.insert(inc.mapping_path.clone())
                    && let Some(m) = self.owner.surface_of(&inc.mapping_path)
                {
                    self.walk_roots(&m, &mut out, &mut seen);
                }
            }
            out
        })
    }

    /// `MappingClosures.Closure.walkRoots` (:328).
    fn walk_roots(
        &self,
        md: &LegacyMappingDefinition,
        out: &mut LinkedMap<JStr, ClassMapping>,
        seen: &mut HashSet<JStr>,
    ) {
        for inc in md.includes.iter() {
            if seen.insert(inc.mapping_path.clone())
                && let Some(m) = self.owner.surface_of(&inc.mapping_path)
            {
                self.walk_roots(&m, out, seen);
            }
        }
        Closure::own_roots(md, out);
    }

    /// `MappingClosures.Closure.ownRoots(md, out)` (:341): the mapping's own root sets -- the `*` set, or the class's SOLE set (engine
    /// `rootClassMappingByClass`; corpus mappings often omit `*` on singletons).
    pub fn own_roots(md: &LegacyMappingDefinition, out: &mut LinkedMap<JStr, ClassMapping>) {
        let mut sets_per_class: LinkedMap<JStr, i32> = LinkedMap::new();
        for cm in md.class_mappings.iter() {
            *sets_per_class.get_or_insert_with(cm.class_name().clone(), || 0) += 1;
        }
        for cm in md.class_mappings.iter() {
            if cm.root() || sets_per_class.get(cm.class_name()) == Some(&1) {
                out.put(cm.class_name().clone(), cm.clone());
            }
        }
    }

    /// `MappingClosures.Closure.duplicateEnumerationMappingId()`: the first enumeration mapping id taken twice across this
    /// mapping's closure -- by two mappings, or twice within one (an include's includes first, then its own, then this
    /// mapping's own, each mapping once) -- by the effective id; none when none. The engine's
    /// `collectAndValidateEnumerationMappingIds` (MappingValidator.java:197-229; 2026-11-15, the normalizer batch, item 2).
    pub fn duplicate_enumeration_mapping_id(&self) -> Result<Option<JStr>, Thrown> {
        let mut owner: LinkedMap<JStr, JStr> = LinkedMap::new();
        let mut seen: HashSet<JStr> = HashSet::new();
        self.walk_enumeration_ids(&self.state.fqn, &mut owner, &mut seen)
    }

    /// `MappingClosures.Closure.walkEnumerationIds`.
    fn walk_enumeration_ids(
        &self,
        mapping_fqn: &JStr,
        owner: &mut LinkedMap<JStr, JStr>,
        seen: &mut HashSet<JStr>,
    ) -> Result<Option<JStr>, Thrown> {
        if !seen.insert(mapping_fqn.clone()) {
            return Ok(None);
        }
        let md = self
            .owner
            .surface_of(mapping_fqn)
            .ok_or_else(|| missing_mapping(mapping_fqn))?;
        for inc in md.includes.iter() {
            if let Some(dup) = self.walk_enumeration_ids(&inc.mapping_path, owner, seen)? {
                return Ok(Some(dup));
            }
        }
        let mut own: HashSet<JStr> = HashSet::new();
        for em in md.enumeration_mappings.iter() {
            let id = set_id_of(em.mapping_id.as_ref(), &em.enum_name);
            let other = match owner.get(&id) {
                Some(prev) => prev != mapping_fqn,
                None => {
                    owner.put(id.clone(), mapping_fqn.clone());
                    false
                }
            };
            if other || !own.insert(id.clone()) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// `MappingClosures.Closure.enumerationMappings()` (:355): enumeration mappings through the includes, transitively; a bare include path
    /// resolves in the mapping's own package first. Loud for an include the model has not.
    pub fn enumeration_mappings(&self) -> Result<&[Arc<EnumerationMapping>], Thrown> {
        if let Some(known) = self.state.enums.get() {
            return Ok(known);
        }
        let mut out = Vec::new();
        let mut seen: HashSet<JStr> = HashSet::new();
        seen.insert(self.state.fqn.clone());
        if let Some(m) = self.owner.surface_of(&self.state.fqn) {
            self.walk_enums(&m, &mut out, &mut seen)?;
        }
        Ok(self.state.enums.get_or_init(|| list(out)))
    }

    /// `MappingClosures.Closure.walkEnums` (:368).
    fn walk_enums(
        &self,
        md: &LegacyMappingDefinition,
        out: &mut Vec<Arc<EnumerationMapping>>,
        seen: &mut HashSet<JStr>,
    ) -> Result<(), Thrown> {
        for inc in md.includes.iter() {
            let mut path = inc.mapping_path.clone();
            if !contains(&path.0, "::") && contains(&md.qualified_name.0, "::") {
                let cut = last_index_of(&md.qualified_name.0, &JStr::from("::").0).expect("contains `::`");
                let in_pkg = cat!(&md.qualified_name.0[..cut], "::", &path);
                if self.owner.model.find_legacy_mapping(&in_pkg).is_some() {
                    path = in_pkg;
                }
            }
            if !seen.insert(path.clone()) {
                continue;
            }
            let included = self.owner.surface_of(&path).ok_or_else(|| missing_mapping(&path))?;
            out.extend(included.enumeration_mappings.iter().cloned());
            self.walk_enums(&included, out, seen)?;
        }
        Ok(())
    }

    /// `MappingClosures.Closure.pairEntries(classFqn)` (:393): per-pair association entries for `class_fqn` (or a subclass of the end's owner)
    /// through the includes -- source set id -> the routed Join PMs, each include's, depth-first (the mapping's own
    /// entries are the caller's, [`Closure::own_pairs`]). Memoized per class; loud for an association the model has
    /// not.
    pub fn pair_entries(&self, class_fqn: &JStr) -> Result<Rc<PairEntries>, Thrown> {
        if let Some(hit) = self.state.pair_entries.borrow().get(class_fqn) {
            return Ok(hit.clone());
        }
        let mut out: LinkedMap<JStr, Vec<Arc<PropertyMappingJoin>>> = LinkedMap::new();
        let mut seen: HashSet<JStr> = HashSet::new();
        seen.insert(self.state.fqn.clone());
        for inc in self.includes().iter() {
            if let Some(m) = self.owner.surface_of(&inc.mapping_path) {
                self.walk_pairs(&m, class_fqn, &mut out, &mut seen)?;
            }
        }
        // `out.replaceAll((k, v) -> List.copyOf(v))`, then unmodifiable
        let hit = Rc::new(LinkedMap::of(out.iter().map(|(k, v)| (k.clone(), list(v.clone())))));
        Ok(self
            .state
            .pair_entries
            .borrow_mut()
            .entry(class_fqn.clone())
            .or_insert(hit)
            .clone())
    }

    /// `MappingClosures.Closure.walkPairs` (:409).
    fn walk_pairs(
        &self,
        md: &LegacyMappingDefinition,
        class_fqn: &JStr,
        out: &mut LinkedMap<JStr, Vec<Arc<PropertyMappingJoin>>>,
        seen: &mut HashSet<JStr>,
    ) -> Result<(), Thrown> {
        if !seen.insert(md.qualified_name.clone()) {
            return Ok(());
        }
        self.own_pairs(md, class_fqn, out)?;
        for inc in md.includes.iter() {
            if let Some(m) = self.owner.surface_of(&inc.mapping_path) {
                self.walk_pairs(&m, class_fqn, out, seen)?;
            }
        }
        Ok(())
    }

    /// `MappingClosures.Closure.ownPairs(md, classFqn, out)` (:421): the mapping's own pair entries for `class_fqn`; an entry with no target
    /// set id of its own is stamped with the association mapping's.
    pub fn own_pairs(
        &self,
        md: &LegacyMappingDefinition,
        class_fqn: &JStr,
        out: &mut LinkedMap<JStr, Vec<Arc<PropertyMappingJoin>>>,
    ) -> Result<(), Thrown> {
        let model = self.owner.model;
        for am in md.association_mappings.iter() {
            let AssociationMapping::AssociationMappingRelational(rel) = am else {
                continue;
            };
            let ad = model.find_association(am.association_name()).ok_or_else(|| {
                Thrown::model(
                    Phase::Mapping,
                    cat!("Can't find association '", am.association_name(), "'"),
                    None,
                )
            })?;
            for apm in rel.property_mappings.iter() {
                let PropertyMapping::PropertyMappingJoin(join) = &apm.body else {
                    continue;
                };
                let Some(source_set_id) = &apm.source_set_id else {
                    continue;
                };
                let owner = association_synthesis::association_owner_class(ad, apm.property_name());
                // the union class may INHERIT the end (B extends A picking up AE's 'e' -- extends/union family):
                // owner-or-superclass
                let Some(owner) = owner else { continue };
                if !(owner == class_fqn || model.knowledge().is_subtype(class_fqn, owner)) {
                    continue;
                }
                let stamped = if join.target_set_id.is_none() {
                    Arc::new(PropertyMappingJoin {
                        property_name: join.property_name.clone(),
                        database: join.database.clone(),
                        joins: join.joins.clone(),
                        target_set_id: apm.target_set_id.clone(),
                    })
                } else {
                    join.clone()
                };
                out.get_or_insert_with(source_set_id.clone(), Vec::new).push(stamped);
            }
        }
        Ok(())
    }
}
