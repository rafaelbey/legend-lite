// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ResolvedMapping` (ResolvedMapping.java at ad86fa895, ported line for line as a struct): a mapping RESOLVED for
//! synthesis -- its pre-passed record (extends flattened, same-extent inheritance applied, store refs qualified,
//! implicit operation sets appended; every step a rewrite of THIS record under construction, `MappingPrePass`), its
//! authored surface, the sets' own declared keys, the sets the validation walled, and its include closure. Every
//! closure question synthesis asks -- a set by id, the operation set of a class, roots, pair entries, a union member's
//! ordinal -- is asked of this record. Whether a class is MAPPED for this mapping is the ledger's answer over the
//! pre-passed closure (`MappingLedger::is_mapped`), never a graph-wide fact.
//!
//! The record borrows the phase's E-local inputs for the call: the lifted views ([`LiftedViews`]) and the include
//! facts ([`MappingClosures`], through its [`Closure`]). RE-EXPRESSED (census §4): [`ResolvedMapping::closure`] is
//! `[md] ++ closure.mappings()` (Java's `seen` re-did `mappings()`' own dedup, which seeds it with `md`'s name); and
//! [`ResolvedMapping::first_relational`] is the one "first Relational set with this id" lookup JoinChainEmission
//! (JCE:132-139, :758-771) and UnionSynthesis (US:231-240) spelled three times.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use lite_model::linked::LinkedMap;
use lite_model::names::set_id_of_class_mapping;
use lite_model::{
    AssociationMapping, ClassMapping, ClassMappingInheritance, ClassMappingRelational, ClassMappingUnion,
    EnumerationMapping, JStr, LegacyMappingDefinition, MappingDefinitionClassBindingDeclaredKeys as DeclaredKeys,
    MappingInclude, PropertyMappingJoin,
};

use crate::error::Thrown;
use crate::lifted_views::LiftedViews;
use crate::mapping_closures::{Closure, MappingClosures};

/// A mapping resolved for synthesis.
#[derive(Clone)]
pub struct ResolvedMapping<'a> {
    /// The pre-passed record (Java's `md`).
    pub md: Arc<LegacyMappingDefinition>,
    /// The included part of the mapping's closure.
    pub closure: Closure<'a>,
    surface: Arc<LegacyMappingDefinition>,
    declared_keys: Rc<HashMap<JStr, Arc<DeclaredKeys>>>,
    /// The sets the validation walled, BY SET ID (never by object: the construction steps rebuild the records), with
    /// the recorded reason.
    invalid: Rc<LinkedMap<JStr, JStr>>,
    /// The lifted view bodies (E.5) every view-expansion site of this mapping reads.
    views: &'a LiftedViews<'a>,
    /// This record's OWN sets by class (declaration order) and by effective id (the first set declaring an id), and
    /// the ROOT set per class over the whole closure, built with the record.
    own_by_class: HashMap<JStr, Vec<ClassMapping>>,
    own_by_id: HashMap<JStr, ClassMapping>,
    roots: LinkedMap<JStr, ClassMapping>,
}

impl<'a> ResolvedMapping<'a> {
    /// `new ResolvedMapping(md, surface, declaredKeys, invalid, closure, views)` (:53). The closure's roots are read
    /// here; nothing else of it (laziness is answer-bearing).
    pub fn new(
        md: Arc<LegacyMappingDefinition>,
        surface: Arc<LegacyMappingDefinition>,
        declared_keys: Rc<HashMap<JStr, Arc<DeclaredKeys>>>,
        invalid: Rc<LinkedMap<JStr, JStr>>,
        closure: Closure<'a>,
        views: &'a LiftedViews<'a>,
    ) -> ResolvedMapping<'a> {
        let mut by_class: HashMap<JStr, Vec<ClassMapping>> = HashMap::new();
        let mut by_id: HashMap<JStr, ClassMapping> = HashMap::new();
        for cm in md.class_mappings.iter() {
            by_class.entry(cm.class_name().clone()).or_default().push(cm.clone());
            by_id.entry(ResolvedMapping::id_of(cm)).or_insert_with(|| cm.clone());
        }
        let mut roots_by_class = closure.roots().clone();
        Closure::own_roots(&md, &mut roots_by_class);
        ResolvedMapping {
            md,
            closure,
            surface,
            declared_keys,
            invalid,
            views,
            own_by_class: by_class,
            own_by_id: by_id,
            roots: roots_by_class,
        }
    }

    /// `views()` (:75).
    pub fn views(&self) -> &'a LiftedViews<'a> {
        self.views
    }

    /// The phase's include facts (where Java asks `MappingClosures.of(model)`: AS:67, MappingValidation:51).
    pub fn closures(&self) -> &'a MappingClosures<'a> {
        self.closure.owner()
    }

    /// `withMapping(rewritten)` (:79): the same record over a rewritten mapping (a construction step, the multi-hop
    /// injection).
    pub fn with_mapping(&self, rewritten: Arc<LegacyMappingDefinition>) -> ResolvedMapping<'a> {
        ResolvedMapping::new(
            rewritten,
            self.surface.clone(),
            self.declared_keys.clone(),
            self.invalid.clone(),
            self.closure.clone(),
            self.views,
        )
    }

    /// `withInvalid(walled)` (:84): the same record with the sets the validation walled (by set id).
    pub fn with_invalid(&self, walled: LinkedMap<JStr, JStr>) -> ResolvedMapping<'a> {
        ResolvedMapping::new(
            self.md.clone(),
            self.surface.clone(),
            self.declared_keys.clone(),
            Rc::new(walled),
            self.closure.clone(),
            self.views,
        )
    }

    /// `surface()` (:88).
    pub fn surface(&self) -> &Arc<LegacyMappingDefinition> {
        &self.surface
    }

    /// `declaredKeys()` (:89).
    pub fn declared_keys(&self) -> &HashMap<JStr, Arc<DeclaredKeys>> {
        &self.declared_keys
    }

    /// `invalidReason(cm)` (:92): the validation's recorded reason for `cm`'s set.
    pub fn invalid_reason(&self, cm: &ClassMapping) -> Option<&JStr> {
        self.invalid.get(&ResolvedMapping::id_of(cm))
    }

    /// `raw()` (:99).
    pub fn raw(&self) -> &Arc<LegacyMappingDefinition> {
        &self.md
    }

    /// `qualifiedName()` (:100).
    pub fn qualified_name(&self) -> &JStr {
        &self.md.qualified_name
    }

    /// `classMappings()` (:101).
    pub fn class_mappings(&self) -> &Arc<[ClassMapping]> {
        &self.md.class_mappings
    }

    /// `includes()` (:102).
    pub fn includes(&self) -> &Arc<[Arc<MappingInclude>]> {
        &self.md.includes
    }

    /// `associationMappings()` (:103).
    pub fn association_mappings(&self) -> &Arc<[AssociationMapping]> {
        &self.md.association_mappings
    }

    /// `enumerationMappings()` (:104).
    pub fn enumeration_mappings(&self) -> &Arc<[Arc<EnumerationMapping>]> {
        &self.md.enumeration_mappings
    }

    /// `ResolvedMapping.idOf(cm)` (:109): a set's EFFECTIVE id (`SetId`, the one rule).
    pub fn id_of(cm: &ClassMapping) -> JStr {
        set_id_of_class_mapping(cm)
    }

    /// `set(setId)` (:116): the set with id `set_id` -- this mapping's own first, else one visible through the includes
    /// (whose walk is loud for an include the model has not); none for no id or no such set.
    pub fn set(&self, set_id: Option<&JStr>) -> Result<Option<ClassMapping>, Thrown> {
        let Some(set_id) = set_id else { return Ok(None) };
        if let Some(own) = self.own_by_id.get(set_id) {
            return Ok(Some(own.clone()));
        }
        Ok(self.closure.sets()?.get(set_id).cloned())
    }

    /// `closure()` (:125): this mapping then its includes, depth-first in include order, each once -- RE-EXPRESSED as
    /// `[md] ++ closure.mappings()` (which excludes `md`'s own name).
    pub fn closure(&self) -> Vec<Arc<LegacyMappingDefinition>> {
        let included = self.closure.mappings();
        let mut out = Vec::with_capacity(included.len() + 1);
        out.push(self.md.clone());
        out.extend(included.iter().cloned());
        out
    }

    /// `relationalSets(classFqn)` (:142): `class_fqn`'s Relational sets across [`ResolvedMapping::closure`] -- this
    /// mapping's own in declaration order, then each include's.
    pub fn relational_sets(&self, class_fqn: Option<&JStr>) -> Vec<Arc<ClassMappingRelational>> {
        let mut out = Vec::new();
        if let Some(own) = class_fqn.and_then(|c| self.own_by_class.get(c)) {
            for cm in own {
                if let ClassMapping::ClassMappingRelational(rcm) = cm {
                    out.push(rcm.clone());
                }
            }
        }
        out.extend(self.closure.relational_sets(class_fqn));
        out
    }

    /// `includedSets()` (:155): set ids visible through the includes (a later include overrides an earlier one;
    /// substitutions applied); own sets excluded.
    pub fn included_sets(&self) -> Result<&LinkedMap<JStr, ClassMapping>, Thrown> {
        self.closure.sets()
    }

    /// `visibleSets()` (:160): every visible set by id -- the includes', then this mapping's own on top.
    pub fn visible_sets(&self) -> Result<LinkedMap<JStr, ClassMapping>, Thrown> {
        let mut out = self.closure.sets()?.clone();
        for cm in self.md.class_mappings.iter() {
            out.put(ResolvedMapping::id_of(cm), cm.clone());
        }
        Ok(out)
    }

    /// `unionOf(classFqn)` (:170): the Union operation set for `class_fqn` -- own first, else the one found through the
    /// includes.
    pub fn union_of(&self, class_fqn: Option<&JStr>) -> Option<Arc<ClassMappingUnion>> {
        if let Some(own) = class_fqn.and_then(|c| self.own_by_class.get(c)) {
            for cm in own {
                if let ClassMapping::ClassMappingUnion(u) = cm {
                    return Some(u.clone());
                }
            }
        }
        self.closure.union(class_fqn)
    }

    /// `inheritanceOf(classFqn)` (:180): the Inheritance operation set for `class_fqn`, the same rule.
    pub fn inheritance_of(&self, class_fqn: &JStr) -> Option<Arc<ClassMappingInheritance>> {
        if let Some(own) = self.own_by_class.get(class_fqn) {
            for cm in own {
                if let ClassMapping::ClassMappingInheritance(ih) = cm {
                    return Some(ih.clone());
                }
            }
        }
        self.closure.inheritance(class_fqn)
    }

    /// `roots()` (:191): ROOT set per class -- the includes' (deeper first), this mapping's own overriding; the `*` set
    /// or the class's sole set.
    pub fn roots(&self) -> &LinkedMap<JStr, ClassMapping> {
        &self.roots
    }

    /// `isRootOrSole(set)` (:202): is `set` the ROOT (or sole) set of its class -- the engine's
    /// `rootClassMappingByClass` answer, judged in the OWNING scope (the closure's roots, this mapping's own
    /// overriding).
    pub fn is_root_or_sole(&self, set: &ClassMapping) -> bool {
        self.roots
            .get(set.class_name())
            .is_some_and(|root| ResolvedMapping::id_of(root) == ResolvedMapping::id_of(set))
    }

    /// `enumerationMappingsWithIncludes()` (:208): own enumeration mappings plus the includes', transitively.
    pub fn enumeration_mappings_with_includes(&self) -> Result<Vec<Arc<EnumerationMapping>>, Thrown> {
        let mut out: Vec<Arc<EnumerationMapping>> = self.md.enumeration_mappings.to_vec();
        out.extend(self.closure.enumeration_mappings()?.iter().cloned());
        Ok(out)
    }

    /// `pairEntries(classFqn)` (:216): per-pair association entries for `class_fqn` -- own first, then each
    /// include's, depth-first.
    pub fn pair_entries(&self, class_fqn: &JStr) -> Result<LinkedMap<JStr, Vec<Arc<PropertyMappingJoin>>>, Thrown> {
        let mut out: LinkedMap<JStr, Vec<Arc<PropertyMappingJoin>>> = LinkedMap::new();
        self.closure.own_pairs(&self.md, class_fqn, &mut out)?;
        for (set_id, joins) in self.closure.pair_entries(class_fqn)?.iter() {
            out.get_or_insert_with(set_id.clone(), Vec::new)
                .extend(joins.iter().cloned());
        }
        Ok(out)
    }

    /// `memberOrdinal(memberIds, setId)` (:226): a union member's ordinal for `set_id` -- the member itself, or the
    /// member whose `extends` chain reaches it; -1 otherwise.
    pub fn member_ordinal(&self, member_ids: &[JStr], set_id: Option<&JStr>) -> Result<i32, Thrown> {
        if let Some(direct) = set_id.and_then(|s| member_ids.iter().position(|m| m == s)) {
            return Ok(direct as i32);
        }
        for (i, member) in member_ids.iter().enumerate() {
            let mut m = self.set(Some(member))?;
            let mut seen: HashSet<JStr> = HashSet::new();
            while let Some(ClassMapping::ClassMappingRelational(r)) = &m
                && let Some(ext) = &r.extends_set_id
                && seen.insert(ext.clone())
            {
                if Some(ext) == set_id {
                    return Ok(i as i32);
                }
                let next = self.set(Some(ext))?;
                m = next;
            }
        }
        Ok(-1)
    }

    /// The FIRST Relational set of this mapping's OWN class mappings whose effective id is `set_id`: the one helper
    /// for JCE:132-139 (an Inline-embedded PM's referenced set), JCE:758-771 (the JoinNav collector's) and US:231-240
    /// (the routed-join collector's), which spelled it three times (census §4).
    pub fn first_relational(&self, set_id: &JStr) -> Option<Arc<ClassMappingRelational>> {
        self.md.class_mappings.iter().find_map(|cm| match cm {
            ClassMapping::ClassMappingRelational(r2) if &ResolvedMapping::id_of(cm) == set_id => Some(r2.clone()),
            _ => None,
        })
    }
}
