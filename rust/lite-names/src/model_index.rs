// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The model index (2026-11-12, the names slice: Java's `ModelBuilder`): the one indexed view of a graph's elements,
//! every kind in its own slot vector by an id from one [`SymbolTable`], built from the name-resolved, knowledge-adopted
//! elements before Phase E ([`ModelIndex::from`]) and added to at the E-to-F gate ([`ModelIndex::add`]). It resolves
//! no name and checks no type: it reads what it is given, verbatim.
//!
//! Ported line for line, Java's answers and refusals included:
//! - **Registration order** is the iteration accessors' order ([`ModelIndex::classes`] and kin), every batch's
//!   elements in list order; the lazy indexes (direct subclasses, association ends, legacy sets by id) walk the slot
//!   vectors in **id order**, as Java's `for (... : classes)` over the `ArrayList` does.
//! - **Identity** (PHASE3.md step 0's census, applied as it classified each site): "this very object already sits in
//!   its slot" (`model.ModelBuilder.indexed.*`, class (b)) is [`same`]; a self-association's end injected twice is one
//!   association (`ModelBuilder.findAssociationAtClass`, class (c)) by [`same`].
//! - **Java's hash order** reaches no answer here: every `HashMap` of `ModelBuilder` is only looked up; the maps whose
//!   order is seen (`duplicateElements`, the primitive extensions) are insertion-ordered in both.
//! - A **duplicate** registration in the shared element namespace is recorded (`duplicateElements`), not thrown; the
//!   slot is last-wins. A database registered twice keeps the first one's filter, join or view index when the second
//!   declares none (Java writes a secondary index only when it is non-empty).

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use lite_base::same;
use lite_error::Phase;
use lite_model::linked::LinkedMap;
use lite_model::protocol as p;
use lite_model::{
    AssociationDefinition, AssociationDefinitionAssociationEndDefinition, ClassDefinition, ClassMapping,
    ConnectionDefinition, DataDefinition, DataSpaceDefinition, DatabaseDefinition, DatabaseDefinitionFilterDefinition,
    DatabaseDefinitionJoinDefinition, DatabaseDefinitionTableDefinition, DatabaseDefinitionViewDefinition,
    EnumDefinition, Function, ImportScope, JStr, LegacyMappingDefinition, MappingDefinition, MeasureDefinition,
    ModelChainConnectionDefinition, ModelConnectionDefinition, PackageableElement, ParsedModel, ProfileDefinition,
    RelationalOperation, RuntimeDefinition, ServiceDefinition, cat,
};
use lite_types::Primitive;

use crate::error::Thrown;
use crate::knowledge::{Knowledge, KnowledgeMemo};
use crate::symbol_table::{SymbolTable, UNRESOLVED};
use crate::synth_fqn;
use crate::table_index::TableIndex;

type View = Arc<DatabaseDefinitionViewDefinition>;
type Table = Arc<DatabaseDefinitionTableDefinition>;
type End = Arc<AssociationDefinitionAssociationEndDefinition>;

/// A view with the spelling the index gave it (`ModelBuilder.IndexedView`).
#[derive(Clone, Debug)]
pub struct IndexedView {
    pub definition: View,
    pub lift: ViewLift,
}

/// A view's lift identity (`ModelBuilder.ViewLift`): the owning database of the include closure and the lift's own
/// spelling (a schema view `SCHEMA.NAME`, a top-level view bare).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewLift {
    pub owner_db: JStr,
    pub spelling: JStr,
}

impl ViewLift {
    /// `fqn()`: the lifted function's name, `<owner>$view$<spelling>`.
    pub fn fqn(&self) -> Result<JStr, Thrown> {
        synth_fqn::view(&self.owner_db, &self.spelling)
    }
}

/// A legacy mapping's set, and the mapping defining it (`ModelBuilder.LegacySet`).
#[derive(Clone, Debug)]
pub struct LegacySet {
    pub defining: Arc<LegacyMappingDefinition>,
    pub set: ClassMapping,
}

/// One association end injected onto a class.
#[derive(Clone, Debug)]
struct InjectedEnd {
    association: Arc<AssociationDefinition>,
    end: End,
}

/// An insertion-ordered set of ids (Java's `LinkedHashSet<Integer>`).
#[derive(Clone, Debug, Default)]
struct OrderedIds {
    order: Vec<i32>,
    seen: HashSet<i32>,
}

impl OrderedIds {
    fn add(&mut self, id: i32) {
        if self.seen.insert(id) {
            self.order.push(id);
        }
    }

    fn contains(&self, id: i32) -> bool {
        self.seen.contains(&id)
    }
}

/// The model index (Java's `ModelBuilder`).
pub struct ModelIndex {
    symbols: SymbolTable,
    element_order: OrderedIds,
    registered_elements: HashSet<i32>,
    classes: Vec<Option<Arc<ClassDefinition>>>,
    associations: Vec<Option<Arc<AssociationDefinition>>>,
    association_ends_by_owner: OnceLock<HashMap<JStr, HashMap<JStr, Vec<InjectedEnd>>>>,
    enums: Vec<Option<Arc<EnumDefinition>>>,
    profiles: Vec<Option<Arc<ProfileDefinition>>>,
    measures: Vec<Option<Arc<MeasureDefinition>>>,
    databases: Vec<Option<Arc<DatabaseDefinition>>>,
    legacy_mappings: Vec<Option<Arc<LegacyMappingDefinition>>>,
    legacy_sets_by_id: OnceLock<HashMap<JStr, Vec<LegacySet>>>,
    mappings: Vec<Option<Arc<MappingDefinition>>>,
    services: Vec<Option<Arc<ServiceDefinition>>>,
    data_elements: Vec<Option<Arc<DataDefinition>>>,
    data_spaces: Vec<Option<Arc<DataSpaceDefinition>>>,
    runtimes: Vec<Option<Arc<RuntimeDefinition>>>,
    connections: Vec<Option<Arc<ConnectionDefinition>>>,
    model_connections: HashMap<JStr, Arc<ModelConnectionDefinition>>,
    model_chain_connections: HashMap<JStr, Arc<ModelChainConnectionDefinition>>,
    foreign_connections: HashSet<JStr>,
    functions: Vec<Option<Vec<Function>>>,
    functions_by_id: HashMap<JStr, Vec<Function>>,
    filters_by_db: HashMap<i32, HashMap<JStr, Arc<DatabaseDefinitionFilterDefinition>>>,
    joins_by_db: HashMap<i32, HashMap<JStr, Arc<DatabaseDefinitionJoinDefinition>>>,
    views_by_db: HashMap<i32, HashMap<JStr, IndexedView>>,
    tables_by_db: HashMap<i32, TableIndex>,
    views_in_order: HashMap<i32, Vec<IndexedView>>,
    duplicate_elements: LinkedMap<JStr, JStr>,
    imports: Arc<ImportScope>,
    element_imports: HashMap<JStr, Arc<ImportScope>>,
    primitive_extensions: LinkedMap<JStr, JStr>,
    primitive_type_variables: LinkedMap<JStr, Arc<[Box<p::ParameterDefinition>]>>,
    direct_subclasses: OnceLock<HashMap<JStr, Vec<JStr>>>,
    knowledge: OnceLock<KnowledgeMemo>,
    derived: Mutex<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

fn put_at_id<T>(list: &mut Vec<Option<T>>, id: i32, value: T) {
    let i = id as usize;
    while list.len() <= i {
        list.push(None);
    }
    list[i] = Some(value);
}

fn id_get<T>(list: &[Option<T>], id: i32) -> Option<&T> {
    if id < 0 {
        return None;
    }
    list.get(id as usize).and_then(Option::as_ref)
}

impl ModelIndex {
    fn new(imports: Arc<ImportScope>, element_imports: HashMap<JStr, Arc<ImportScope>>) -> ModelIndex {
        ModelIndex {
            symbols: SymbolTable::new(),
            element_order: OrderedIds::default(),
            registered_elements: HashSet::new(),
            classes: Vec::new(),
            associations: Vec::new(),
            association_ends_by_owner: OnceLock::new(),
            enums: Vec::new(),
            profiles: Vec::new(),
            measures: Vec::new(),
            databases: Vec::new(),
            legacy_mappings: Vec::new(),
            legacy_sets_by_id: OnceLock::new(),
            mappings: Vec::new(),
            services: Vec::new(),
            data_elements: Vec::new(),
            data_spaces: Vec::new(),
            runtimes: Vec::new(),
            connections: Vec::new(),
            model_connections: HashMap::new(),
            model_chain_connections: HashMap::new(),
            foreign_connections: HashSet::new(),
            functions: Vec::new(),
            functions_by_id: HashMap::new(),
            filters_by_db: HashMap::new(),
            joins_by_db: HashMap::new(),
            views_by_db: HashMap::new(),
            tables_by_db: HashMap::new(),
            views_in_order: HashMap::new(),
            duplicate_elements: LinkedMap::new(),
            imports,
            element_imports,
            primitive_extensions: LinkedMap::new(),
            primitive_type_variables: LinkedMap::new(),
            direct_subclasses: OnceLock::new(),
            knowledge: OnceLock::new(),
            derived: Mutex::new(HashMap::new()),
        }
    }

    /// `ModelBuilder.from(model)`: the index of a parsed model, its import scopes carried through. Throws what
    /// ingesting throws (a legacy mapping's duplicate class mappings or set ids, an inline connection of an unexpected
    /// kind).
    pub fn from(model: &ParsedModel) -> Result<ModelIndex, Thrown> {
        let element_imports: HashMap<JStr, Arc<ImportScope>> = model
            .element_imports
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut mb = ModelIndex::new(model.imports.clone(), element_imports);
        mb.add(&model.elements)?;
        Ok(mb)
    }

    /// `add(elements)`: index a batch; an element already indexed (this very object in its slot) is skipped, and the
    /// lazy indexes are rebuilt on the next read. Phase 1 interns every name (registration order), phase 2 the
    /// data-model elements, phase 3a the top-level definitions, phase 3b the runtimes.
    pub fn add(&mut self, elements: &[PackageableElement]) -> Result<(), Thrown> {
        let fresh: Vec<&PackageableElement> = elements.iter().filter(|el| !self.indexed(el)).collect();
        if fresh.is_empty() {
            return Ok(());
        }
        self.direct_subclasses = OnceLock::new();
        self.association_ends_by_owner = OnceLock::new();
        self.knowledge = OnceLock::new();
        self.derived = Mutex::new(HashMap::new());

        for el in &fresh {
            let id = self.symbols.intern(el.qualified_name());
            self.element_order.add(id);
        }

        for el in &fresh {
            match el {
                PackageableElement::ClassDefinition(cd) => {
                    let id = self.intern_element(&cd.qualified_name);
                    put_at_id(&mut self.classes, id, cd.clone());
                }
                PackageableElement::PrimitiveExtensionDefinition(pe) => {
                    self.primitive_extensions
                        .put(pe.qualified_name.clone(), pe.base_type_name.clone());
                    self.primitive_type_variables
                        .put(pe.qualified_name.clone(), pe.type_variables.clone());
                }
                PackageableElement::AssociationDefinition(ad) => {
                    let id = self.intern_element(&ad.qualified_name);
                    put_at_id(&mut self.associations, id, ad.clone());
                }
                PackageableElement::EnumDefinition(ed) => {
                    let id = self.intern_element(&ed.qualified_name);
                    put_at_id(&mut self.enums, id, ed.clone());
                }
                PackageableElement::ProfileDefinition(pd) => {
                    let id = self.intern_element(&pd.qualified_name);
                    put_at_id(&mut self.profiles, id, pd.clone());
                }
                PackageableElement::MeasureDefinition(me) => {
                    let id = self.intern_element(&me.qualified_name);
                    put_at_id(&mut self.measures, id, me.clone());
                }
                PackageableElement::DatabaseDefinition(db) => self.ingest_database(db),
                _ => {}
            }
        }

        for el in &fresh {
            match el {
                PackageableElement::LegacyMappingDefinition(md) => self.ingest_legacy_mapping(md)?,
                PackageableElement::MappingDefinition(md) => {
                    let id = self.symbols.intern(&md.qualified_name);
                    put_at_id(&mut self.mappings, id, md.clone());
                }
                PackageableElement::ServiceDefinition(sd) => {
                    let id = self.symbols.intern(&sd.qualified_name);
                    put_at_id(&mut self.services, id, sd.clone());
                }
                PackageableElement::DataDefinition(dd) => {
                    let id = self.symbols.intern(&dd.qualified_name);
                    put_at_id(&mut self.data_elements, id, dd.clone());
                }
                PackageableElement::DataSpaceDefinition(ds) => {
                    let id = self.symbols.intern(&ds.qualified_name);
                    put_at_id(&mut self.data_spaces, id, ds.clone());
                }
                PackageableElement::ConnectionDefinition(cd) => {
                    let id = self.symbols.intern(&cd.qualified_name);
                    put_at_id(&mut self.connections, id, cd.clone());
                }
                PackageableElement::ModelConnectionDefinition(mc) => {
                    self.model_connections.insert(mc.qualified_name.clone(), mc.clone());
                }
                PackageableElement::ModelChainConnectionDefinition(mcc) => {
                    self.model_chain_connections
                        .insert(mcc.qualified_name.clone(), mcc.clone());
                }
                PackageableElement::FunctionDefinition(fd) => {
                    self.append_function(Function::FunctionDefinition(fd.clone()))
                }
                PackageableElement::NativeFunctionDefinition(nfd) => {
                    self.append_function(Function::NativeFunctionDefinition(nfd.clone()))
                }
                _ => {}
            }
        }

        for el in &fresh {
            if let PackageableElement::RuntimeDefinition(rd) = el {
                self.ingest_runtime(rd)?;
            }
        }
        Ok(())
    }

    /// `indexed(el)`: whether this very object already sits in its slot (by identity: an equal second declaration
    /// is a duplicate to record, never a repeat to skip).
    fn indexed(&self, el: &PackageableElement) -> bool {
        let id = self.symbols.resolve_id(el.qualified_name());
        if id == UNRESOLVED {
            return false;
        }
        fn at<T>(slots: &[Option<Arc<T>>], id: i32, el: &Arc<T>) -> bool {
            id_get(slots, id).is_some_and(|x| same(x, el))
        }
        match el {
            PackageableElement::ClassDefinition(cd) => at(&self.classes, id, cd),
            PackageableElement::AssociationDefinition(ad) => at(&self.associations, id, ad),
            PackageableElement::EnumDefinition(ed) => at(&self.enums, id, ed),
            PackageableElement::ProfileDefinition(pd) => at(&self.profiles, id, pd),
            PackageableElement::MeasureDefinition(me) => at(&self.measures, id, me),
            PackageableElement::DatabaseDefinition(db) => at(&self.databases, id, db),
            PackageableElement::LegacyMappingDefinition(md) => at(&self.legacy_mappings, id, md),
            PackageableElement::MappingDefinition(md) => at(&self.mappings, id, md),
            PackageableElement::ServiceDefinition(sd) => at(&self.services, id, sd),
            PackageableElement::DataDefinition(dd) => at(&self.data_elements, id, dd),
            PackageableElement::DataSpaceDefinition(ds) => at(&self.data_spaces, id, ds),
            PackageableElement::ConnectionDefinition(cd) => at(&self.connections, id, cd),
            PackageableElement::RuntimeDefinition(rd) => at(&self.runtimes, id, rd),
            PackageableElement::FunctionDefinition(_) | PackageableElement::NativeFunctionDefinition(_) => {
                let f = el.as_function().expect("a function");
                id_get(&self.functions, id).is_some_and(|overloads| overloads.iter().any(|x| same_function(x, &f)))
            }
            PackageableElement::PrimitiveExtensionDefinition(pe) => {
                self.primitive_extensions.contains_key(&pe.qualified_name)
            }
            PackageableElement::ModelConnectionDefinition(mc) => self
                .model_connections
                .get(&mc.qualified_name)
                .is_some_and(|x| same(x, mc)),
            PackageableElement::ModelChainConnectionDefinition(mcc) => self
                .model_chain_connections
                .get(&mcc.qualified_name)
                .is_some_and(|x| same(x, mcc)),
            // the pre-E clean-sheet surface is Phase E's input only; the index holds nothing for it
            PackageableElement::CleanSheetMappingDefinition(_) => true,
            PackageableElement::ExecutionEnvironmentDefinition(_)
            | PackageableElement::PersistenceDefinition(_)
            | PackageableElement::PersistenceContextDefinition(_)
            | PackageableElement::FunctionActivatorDefinition(_)
            | PackageableElement::GenericSectionElementDefinition(_)
            | PackageableElement::OpaqueElementDefinition(_) => false,
        }
    }

    /// `internElement(fqn)`: intern, and record a duplicate when the slot is already taken in the shared element
    /// namespace (engine parity: "Duplicated element"); registration still proceeds last-wins.
    fn intern_element(&mut self, fqn: &JStr) -> i32 {
        let id = self.symbols.intern(fqn);
        if !self.registered_elements.insert(id) {
            self.duplicate_elements
                .put_if_absent(fqn.clone(), cat!("Duplicated element '", fqn, "'"));
        }
        id
    }

    /// `duplicateElements()`: the element FQNs registered more than once, each with its message, in first-seen order.
    pub fn duplicate_elements(&self) -> impl Iterator<Item = &(JStr, JStr)> {
        self.duplicate_elements.iter()
    }

    /// `PureModelContext.refuseDuplicates(index, walls)` (Java's `ModelIntegrity.refuseDuplicates`; it reads only
    /// [`ModelIndex::duplicate_elements`], so it is the index's -- ruling 28, the owner's answer of 2026-11-12, moved in
    /// the names fixes of 2026-11-13): every FQN registered more than once is refused with the engine's "Duplicated
    /// element" -- strict (`walls` `None`), the first one's `ModelException` (MODEL, the FQN its element); tolerant, each
    /// walled under its FQN with its message's first line, a key already walled keeping its first reason.
    pub fn refuse_duplicates(&self, mut walls: Option<&mut LinkedMap<JStr, JStr>>) -> Result<(), Thrown> {
        for (fqn, message) in self.duplicate_elements.iter() {
            let refusal = Thrown::model(Phase::Model, message.clone(), Some(fqn));
            match walls.as_deref_mut() {
                None => return Err(refusal),
                Some(w) => {
                    w.put_if_absent(fqn.clone(), refusal.first_line());
                }
            }
        }
        Ok(())
    }

    fn ingest_database(&mut self, db: &Arc<DatabaseDefinition>) {
        let id = self.intern_element(&db.qualified_name);
        put_at_id(&mut self.databases, id, db.clone());
        self.tables_by_db.insert(id, TableIndex::of(db));
        // last-wins as every slot of a database registered twice: a second definition with none of a kind clears the
        // first's (until 2026-11-13 the first's stayed beside the second's tables -- the names fixes, item 6, ruling 28)
        self.filters_by_db.remove(&id);
        self.joins_by_db.remove(&id);
        self.views_by_db.remove(&id);
        self.views_in_order.remove(&id);
        if !db.filters.is_empty() || !db.multi_grain_filters.is_empty() {
            let mut by_name = HashMap::new();
            for f in db.filters.iter() {
                by_name.insert(f.name.clone(), f.clone());
            }
            for f in db.multi_grain_filters.iter() {
                by_name.insert(f.name.clone(), f.clone());
            }
            self.filters_by_db.insert(id, by_name);
        }
        if !db.joins.is_empty() {
            let mut by_name = HashMap::new();
            for j in db.joins.iter() {
                by_name.insert(j.name.clone(), j.clone());
            }
            self.joins_by_db.insert(id, by_name);
        }
        if !db.views.is_empty() || !db.schemas.is_empty() {
            // THE ONE place a view's spelling is decided: a top-level view is its bare name, a schema view
            // SCHEMA.NAME (reachable bare too, first declared wins)
            let mut by_name: HashMap<JStr, IndexedView> = HashMap::new();
            let mut declared: Vec<IndexedView> = Vec::new();
            for v in db.default_schema_views().iter() {
                let iv = IndexedView {
                    definition: v.clone(),
                    lift: ViewLift {
                        owner_db: db.qualified_name.clone(),
                        spelling: v.name.clone(),
                    },
                };
                by_name.insert(v.name.clone(), iv.clone());
                declared.push(iv);
            }
            for s in db.schemas.iter() {
                for v in s.views.iter() {
                    let iv = IndexedView {
                        definition: v.clone(),
                        lift: ViewLift {
                            owner_db: db.qualified_name.clone(),
                            spelling: cat!(&s.name, ".", &v.name),
                        },
                    };
                    by_name.insert(iv.lift.spelling.clone(), iv.clone());
                    by_name.entry(v.name.clone()).or_insert_with(|| iv.clone());
                    declared.push(iv);
                }
            }
            if !by_name.is_empty() {
                self.views_by_db.insert(id, by_name);
                self.views_in_order.insert(id, declared);
            }
        }
    }

    /// `primitiveTypeVariables(fqn)`: the type variables a declared primitive extension declares; empty otherwise.
    pub fn primitive_type_variables(&self, fqn: &JStr) -> Arc<[Box<p::ParameterDefinition>]> {
        self.primitive_type_variables.get(fqn).cloned().unwrap_or_default()
    }

    /// `primitiveExtensionFqns()`: the declared primitive extensions, in declaration order.
    pub fn primitive_extension_fqns(&self) -> impl Iterator<Item = &JStr> {
        self.primitive_extensions.iter().map(|(k, _)| k)
    }

    /// Whether `fqn` is a declared primitive extension (`primitiveExtensionFqns().contains`).
    pub fn is_primitive_extension(&self, fqn: &JStr) -> bool {
        self.primitive_extensions.contains_key(fqn)
    }

    /// `findPrimitiveExtension(name)`: the base primitive behind an extension FQN, extension-of-extension chains chased
    /// (at most 16 hops), exact FQNs only.
    pub fn find_primitive_extension(&self, name: &JStr) -> Option<Primitive> {
        let mut cur = name.clone();
        for _ in 0..16 {
            let base = self.primitive_extensions.get(&cur)?;
            // the primitives' names are ASCII: a lone surrogate's U+FFFD in the lossy text matches none
            let base_s = base.lossy();
            let prim = Primitive::find_by_fqn(&base_s)
                .or_else(|| Primitive::find_by_fqn(&format!("meta::pure::metamodel::type::{base_s}")));
            if prim.is_some() {
                return prim;
            }
            cur = base.clone();
        }
        None
    }

    fn ingest_legacy_mapping(&mut self, md: &Arc<LegacyMappingDefinition>) -> Result<(), Thrown> {
        let id = self.intern_element(&md.qualified_name);
        put_at_id(&mut self.legacy_mappings, id, md.clone());
        self.legacy_sets_by_id = OnceLock::new();
        // R2: within one mapping a class maps once, or through set-id'd class mappings of which exactly one is the
        // root (a union's members, with no root, load; duplicate set ids or two roots are refused)
        let mut by_class: LinkedMap<JStr, Vec<&ClassMapping>> = LinkedMap::new();
        for cm in md.class_mappings.iter() {
            by_class.get_or_insert_with(cm.class_name().clone(), Vec::new).push(cm);
        }
        for (class, sets) in by_class.iter() {
            if sets.len() == 1 {
                continue;
            }
            let roots = sets.iter().filter(|cm| cm.root()).count();
            let mut set_ids: HashSet<JStr> = HashSet::new();
            let ids_distinct = sets.iter().all(|cm| {
                matches!(cm, ClassMapping::ClassMappingUnion(_)) && cm.set_id().is_none()
                    || set_ids.insert(lite_model::names::set_id_of_class_mapping(cm))
            });
            if roots != 1 || !ids_distinct {
                if roots == 0 && ids_distinct {
                    continue;
                }
                return Err(Thrown::model(
                    Phase::Normalize,
                    cat!(
                        "MappingDefinition '",
                        &md.qualified_name,
                        "' contains multiple ClassMappings for class '",
                        class,
                        "'. Each MappingDefinition may map a given class at most once, or through distinct set IDs \
                         with exactly one root marker (*) naming the .all() set."
                    ),
                    None,
                ));
            }
        }
        // CROSS-CLASS effective-id collision (engine MappingValidator spans all class mappings)
        let mut id_owner: HashMap<JStr, JStr> = HashMap::new();
        for cm in md.class_mappings.iter() {
            if matches!(cm, ClassMapping::ClassMappingUnion(_)) && cm.set_id().is_none() {
                continue;
            }
            let effective = lite_model::names::set_id_of_class_mapping(cm);
            match id_owner.get(&effective) {
                Some(prev) if prev != cm.class_name() => {
                    return Err(Thrown::model(
                        Phase::Normalize,
                        cat!(
                            "MappingDefinition '",
                            &md.qualified_name,
                            "': set id '",
                            &effective,
                            "' is used by class mappings of both '",
                            prev,
                            "' and '",
                            cm.class_name(),
                            "' \u{2014} duplicated set ids are ambiguous"
                        ),
                        None,
                    ));
                }
                Some(_) => {}
                None => {
                    id_owner.insert(effective, cm.class_name().clone());
                }
            }
        }
        Ok(())
    }

    /// A runtime and its anonymous inline connections (registered like their standalone twins).
    fn ingest_runtime(&mut self, rd: &Arc<RuntimeDefinition>) -> Result<(), Thrown> {
        let id = self.symbols.intern(&rd.qualified_name);
        put_at_id(&mut self.runtimes, id, rd.clone());
        for inline in rd.inline_connections.iter() {
            match inline {
                PackageableElement::ConnectionDefinition(cd) => {
                    let id = self.symbols.intern(&cd.qualified_name);
                    put_at_id(&mut self.connections, id, cd.clone());
                }
                PackageableElement::ModelConnectionDefinition(mc) => {
                    self.model_connections.insert(mc.qualified_name.clone(), mc.clone());
                }
                PackageableElement::ModelChainConnectionDefinition(mcc) => {
                    self.model_chain_connections
                        .insert(mcc.qualified_name.clone(), mcc.clone());
                }
                PackageableElement::GenericSectionElementDefinition(ge) => {
                    self.foreign_connections.insert(ge.qualified_name.clone());
                }
                other => {
                    return Err(Thrown::illegal_state(cat!(
                        "unexpected inline connection kind: ",
                        other.java_simple_name()
                    )));
                }
            }
        }
        Ok(())
    }

    fn append_function(&mut self, f: Function) {
        let id = self.symbols.intern(f.qualified_name());
        let i = id as usize;
        while self.functions.len() <= i {
            self.functions.push(None);
        }
        self.functions[i].get_or_insert_with(Vec::new).push(f.clone());
        self.functions_by_id
            .entry(lite_model::function_id::mangle(&f))
            .or_default()
            .push(f);
    }

    // ====================================================================
    // Lookups by FQN
    // ====================================================================

    /// `knowledge()`: THE knowledge kernel over this index (class lookup native-first, the memoized subtype relation,
    /// the ancestor and subtree walks), rebuilt when a batch is added.
    pub fn knowledge(&self) -> Knowledge<'_> {
        Knowledge::new(self, self.knowledge.get_or_init(KnowledgeMemo::default))
    }

    /// `KnowledgeLayer.derived(key, derive)`: a fact derived from this index, computed on first ask and the same value
    /// after (a pure function of the index; computed outside the lock, first writer wins, as Java's get-then-put).
    pub fn derived<T: Any + Send + Sync>(&self, derive: impl FnOnce(&ModelIndex) -> T) -> Arc<T> {
        let key = TypeId::of::<T>();
        if let Some(hit) = self.derived.lock().expect("derived").get(&key) {
            return hit.clone().downcast::<T>().expect("a derived fact of its key's type");
        }
        let made: Arc<dyn Any + Send + Sync> = Arc::new(derive(self));
        let mut memo = self.derived.lock().expect("derived");
        let kept = memo.entry(key).or_insert(made).clone();
        kept.downcast::<T>().expect("a derived fact of its key's type")
    }

    /// `directSubclasses(fqn)`: the model classes that directly extend `fqn` (bare and generic heads alike), in slot
    /// order; built on first read.
    pub fn direct_subclasses(&self, fqn: &JStr) -> &[JStr] {
        let index = self.direct_subclasses.get_or_init(|| {
            let mut index: HashMap<JStr, Vec<JStr>> = HashMap::new();
            for cd in self.classes.iter().flatten() {
                for sup in cd.super_classes.iter() {
                    if let Some(head) = sup.raw_class_name() {
                        index.entry(head.clone()).or_default().push(cd.qualified_name.clone());
                    }
                }
            }
            index
        });
        index.get(fqn).map_or(&[], Vec::as_slice)
    }

    /// `findClass(fqn)`: exact FQN only.
    pub fn find_class(&self, fqn: &JStr) -> Option<&Arc<ClassDefinition>> {
        id_get(&self.classes, self.symbols.resolve_id(fqn))
    }

    /// `findAssociation(fqn)`.
    pub fn find_association(&self, fqn: &JStr) -> Option<&Arc<AssociationDefinition>> {
        id_get(&self.associations, self.symbols.resolve_id(fqn))
    }

    /// `findAssociationProperty(owner, prop)`: the target class of the association end named `prop` injected onto
    /// `owner` (the last declared).
    pub fn find_association_property(&self, owner_class_fqn: &JStr, prop_name: &JStr) -> Option<&p::TypeExpression> {
        self.find_association_end(owner_class_fqn, prop_name)
            .map(|e| &e.target_class)
    }

    /// `findAssociationOf(owner, prop)`: the association whose end `prop` injects onto `owner` or, nearest first, a
    /// class it extends (a bare name or a generic's head, as `directSubclasses`: until 2026-11-13 a bare name only, the
    /// names fixes, item 6, ruling 28); two at one level is a model error (`ModelException`, MODEL).
    pub fn find_association_of(
        &self,
        owner_class_fqn: &JStr,
        prop_name: &JStr,
    ) -> Result<Option<&Arc<AssociationDefinition>>, Thrown> {
        let mut visited: HashSet<JStr> = HashSet::new();
        let mut level: std::collections::VecDeque<JStr> = std::collections::VecDeque::from([owner_class_fqn.clone()]);
        while let Some(cls) = level.pop_front() {
            if !visited.insert(cls.clone()) {
                continue;
            }
            if let Some(hit) = self.find_association_at_class(&cls, prop_name)? {
                return Ok(Some(hit));
            }
            if let Some(cd) = self.find_class(&cls) {
                for sup in cd.super_classes.iter() {
                    if let Some(head) = sup.raw_class_name() {
                        level.push_back(head.clone());
                    }
                }
            }
        }
        Ok(None)
    }

    fn find_association_at_class(
        &self,
        owner_class_fqn: &JStr,
        prop_name: &JStr,
    ) -> Result<Option<&Arc<AssociationDefinition>>, Thrown> {
        let mut hits: Vec<&Arc<AssociationDefinition>> = Vec::new();
        for e in self.injected_ends(owner_class_fqn, prop_name) {
            // a self-association naming both ends alike injects twice: one association (by identity)
            if hits.last().is_none_or(|last| !same(*last, &e.association)) {
                hits.push(&e.association);
            }
        }
        if hits.len() > 1 {
            let mut names: Vec<u16> = Vec::new();
            for (i, h) in hits.iter().enumerate() {
                if i > 0 {
                    names.extend(", ".encode_utf16());
                }
                names.extend_from_slice(&h.qualified_name.0);
            }
            return Err(Thrown::model(
                Phase::Model,
                cat!(
                    "property '",
                    prop_name,
                    "' is injected onto class '",
                    owner_class_fqn,
                    "' by ",
                    hits.len().to_string(),
                    " associations (",
                    names,
                    "); duplicate association-end names are ambiguous"
                ),
                Some(owner_class_fqn),
            ));
        }
        Ok(hits.first().copied())
    }

    /// `findAssociationEnd(owner, prop)`: the end named `prop` injected onto `owner`; with several, the last declared.
    pub fn find_association_end(&self, owner_class_fqn: &JStr, prop_name: &JStr) -> Option<&End> {
        self.injected_ends(owner_class_fqn, prop_name).last().map(|e| &e.end)
    }

    fn injected_ends(&self, owner_class_fqn: &JStr, prop_name: &JStr) -> &[InjectedEnd] {
        let idx = self.association_ends_by_owner.get_or_init(|| {
            let mut idx: HashMap<JStr, HashMap<JStr, Vec<InjectedEnd>>> = HashMap::new();
            for ad in self.associations.iter().flatten() {
                // each end injects onto the class the OPPOSITE end targets
                index_end(&mut idx, ad, &ad.property2.target_class, &ad.property1);
                index_end(&mut idx, ad, &ad.property1.target_class, &ad.property2);
            }
            idx
        });
        idx.get(owner_class_fqn)
            .and_then(|m| m.get(prop_name))
            .map_or(&[], Vec::as_slice)
    }

    /// `findEnum(fqn)`.
    pub fn find_enum(&self, fqn: &JStr) -> Option<&Arc<EnumDefinition>> {
        id_get(&self.enums, self.symbols.resolve_id(fqn))
    }

    /// `findProfile(fqn)`.
    pub fn find_profile(&self, fqn: &JStr) -> Option<&Arc<ProfileDefinition>> {
        id_get(&self.profiles, self.symbols.resolve_id(fqn))
    }

    /// `findMeasure(fqn)`.
    pub fn find_measure(&self, fqn: &JStr) -> Option<&Arc<MeasureDefinition>> {
        id_get(&self.measures, self.symbols.resolve_id(fqn))
    }

    /// `findDatabase(fqn)`: exact FQN only (a bare name reaching this lookup is an upstream qualification failure).
    pub fn find_database(&self, fqn: &JStr) -> Option<&Arc<DatabaseDefinition>> {
        id_get(&self.databases, self.symbols.resolve_id(fqn))
    }

    /// `findMapping(fqn)`: the canonical mapping (Phase F's).
    pub fn find_mapping(&self, fqn: &JStr) -> Option<&Arc<MappingDefinition>> {
        id_get(&self.mappings, self.symbols.resolve_id(fqn))
    }

    /// `findLegacyMapping(fqn)`: the legacy mapping surface.
    pub fn find_legacy_mapping(&self, fqn: &JStr) -> Option<&Arc<LegacyMappingDefinition>> {
        id_get(&self.legacy_mappings, self.symbols.resolve_id(fqn))
    }

    /// `retainLegacySurface(md)`: archive a pre-Door-1 mapping surface into an index rebuilt from a normalized model.
    pub fn retain_legacy_surface(&mut self, md: &Arc<LegacyMappingDefinition>) {
        let id = self.symbols.intern(&md.qualified_name);
        self.element_order.add(id);
        put_at_id(&mut self.legacy_mappings, id, md.clone());
        self.legacy_sets_by_id = OnceLock::new();
    }

    /// `findService(fqn)`.
    pub fn find_service(&self, fqn: &JStr) -> Option<&Arc<ServiceDefinition>> {
        id_get(&self.services, self.symbols.resolve_id(fqn))
    }

    /// `findData(fqn)`.
    pub fn find_data(&self, fqn: &JStr) -> Option<&Arc<DataDefinition>> {
        id_get(&self.data_elements, self.symbols.resolve_id(fqn))
    }

    /// `findDataSpace(fqn)`.
    pub fn find_data_space(&self, fqn: &JStr) -> Option<&Arc<DataSpaceDefinition>> {
        id_get(&self.data_spaces, self.symbols.resolve_id(fqn))
    }

    /// `findRuntime(fqn)`.
    pub fn find_runtime(&self, fqn: &JStr) -> Option<&Arc<RuntimeDefinition>> {
        id_get(&self.runtimes, self.symbols.resolve_id(fqn))
    }

    /// `findConnection(fqn)`: a relational connection, a runtime's hoisted inline ones among them.
    pub fn find_connection(&self, fqn: &JStr) -> Option<&Arc<ConnectionDefinition>> {
        id_get(&self.connections, self.symbols.resolve_id(fqn))
    }

    /// `isModelConnection(fqn)`: a model-store or foreign connection (defined, carrying no database type).
    pub fn is_model_connection(&self, fqn: &JStr) -> bool {
        self.model_connections.contains_key(fqn)
            || self.model_chain_connections.contains_key(fqn)
            || self.foreign_connections.contains(fqn)
    }

    /// `findFunction(fqn)`: every overload at `fqn`, in registration order.
    pub fn find_function(&self, fqn: &JStr) -> &[Function] {
        id_get(&self.functions, self.symbols.resolve_id(fqn)).map_or(&[], Vec::as_slice)
    }

    /// `findFunctionById(id)`: the overloads whose engine signature id is exactly `id`.
    pub fn find_function_by_id(&self, qualified_id: &JStr) -> &[Function] {
        self.functions_by_id.get(qualified_id).map_or(&[], Vec::as_slice)
    }

    /// `hasFunctionId(id)`.
    pub fn has_function_id(&self, qualified_id: &JStr) -> bool {
        self.functions_by_id.contains_key(qualified_id)
    }

    // ====================================================================
    // Nested lookups (the database include closure, own first)
    // ====================================================================

    /// `findFilter(db, name)`.
    pub fn find_filter(
        &self,
        db_fqn: Option<&JStr>,
        filter_name: &JStr,
    ) -> Option<&Arc<DatabaseDefinitionFilterDefinition>> {
        self.find_filter_in(db_fqn, filter_name, &mut HashSet::new())
    }

    fn find_filter_in(
        &self,
        db_fqn: Option<&JStr>,
        filter_name: &JStr,
        seen: &mut HashSet<JStr>,
    ) -> Option<&Arc<DatabaseDefinitionFilterDefinition>> {
        let db_fqn = db_fqn?;
        if !seen.insert(db_fqn.clone()) {
            return None;
        }
        let id = self.symbols.resolve_id(db_fqn);
        if id == UNRESOLVED {
            return None;
        }
        if let Some(own) = self.filters_by_db.get(&id).and_then(|m| m.get(filter_name)) {
            return Some(own);
        }
        if let Some(db) = self.find_database(db_fqn) {
            for inc in db.includes.iter() {
                if let Some(hit) = self.find_filter_in(Some(inc), filter_name, seen) {
                    return Some(hit);
                }
            }
        }
        None
    }

    /// `findJoin(db, name)`.
    pub fn find_join(&self, db_fqn: Option<&JStr>, join_name: &JStr) -> Option<&Arc<DatabaseDefinitionJoinDefinition>> {
        self.find_join_in(db_fqn, join_name, &mut HashSet::new())
    }

    fn find_join_in(
        &self,
        db_fqn: Option<&JStr>,
        join_name: &JStr,
        seen: &mut HashSet<JStr>,
    ) -> Option<&Arc<DatabaseDefinitionJoinDefinition>> {
        let db_fqn = db_fqn?;
        if !seen.insert(db_fqn.clone()) {
            return None;
        }
        let id = self.symbols.resolve_id(db_fqn);
        if id != UNRESOLVED {
            if let Some(own) = self.joins_by_db.get(&id).and_then(|m| m.get(join_name)) {
                return Some(own);
            }
            if let Some(db) = self.find_database(db_fqn) {
                for inc in db.includes.iter() {
                    if let Some(hit) = self.find_join_in(Some(inc), join_name, seen) {
                        return Some(hit);
                    }
                }
            }
        }
        None
    }

    /// `findView(db, name)`: the view `name` (bare or `SCHEMA.NAME`) reached from `db`.
    pub fn find_view(&self, db_fqn: &JStr, view_name: &JStr) -> Option<&View> {
        self.indexed_view(db_fqn, view_name, &mut HashSet::new())
            .map(|iv| &iv.definition)
    }

    /// `findView(db, schema, name)`: the `default` schema (or none) is the bare spelling.
    pub fn find_view_in_schema(&self, db_fqn: &JStr, schema: Option<&JStr>, view_name: &JStr) -> Option<&View> {
        match schema {
            None => self.find_view(db_fqn, view_name),
            Some(s) if s.eq_str("default") => self.find_view(db_fqn, view_name),
            Some(s) => self.find_view(db_fqn, &cat!(s, ".", view_name)),
        }
    }

    /// `findTableDefinition(db, name)`: the table reached from `db` through the include closure, own first.
    pub fn find_table_definition(&self, db_fqn: &JStr, name: &JStr) -> Option<&Table> {
        self.find_table_definition_in(db_fqn, name, &mut HashSet::new())
    }

    fn find_table_definition_in(&self, db_fqn: &JStr, name: &JStr, seen: &mut HashSet<JStr>) -> Option<&Table> {
        if !seen.insert(db_fqn.clone()) {
            return None;
        }
        let db = self.find_database(db_fqn)?;
        if let Some(own) = self.find_own_table_definition(db_fqn, name) {
            return Some(own);
        }
        for inc in db.includes.iter() {
            if let Some(hit) = self.find_table_definition_in(inc, name, seen) {
                return Some(hit);
            }
        }
        None
    }

    /// `findOwnTableDefinition(db, name)`: the table `db` itself declares (its includes not consulted).
    pub fn find_own_table_definition(&self, db_fqn: &JStr, name: &JStr) -> Option<&Table> {
        self.tables_by_db
            .get(&self.symbols.resolve_id(db_fqn))
            .and_then(|t| t.find(name))
    }

    /// `viewsOf(db)`: the views `db` itself declares, in declaration order (top-level views, then each schema's).
    pub fn views_of(&self, db_fqn: &JStr) -> &[IndexedView] {
        let id = self.symbols.resolve_id(db_fqn);
        if id == UNRESOLVED {
            return &[];
        }
        self.views_in_order.get(&id).map_or(&[], Vec::as_slice)
    }

    /// `viewMainTable(db, view)`: THE view's main table -- the one table its non-join columns read, or for a join-only
    /// view the first chain's first join's condition tables less the terminals the columns read; loud otherwise.
    pub fn view_main_table(&self, db_fqn: &JStr, view: &DatabaseDefinitionViewDefinition) -> Result<JStr, Thrown> {
        let mut tables: Vec<JStr> = Vec::new();
        for vc in view.column_mappings.iter() {
            if !vc.expression.navigates_join() {
                for t in vc.expression.tables() {
                    if !tables.contains(&t) {
                        tables.push(t);
                    }
                }
            }
        }
        if tables.is_empty() {
            if let Some(root) = self.join_only_view_root(db_fqn, view)? {
                return Ok(root);
            }
            return Err(Thrown::model(
                Phase::Model,
                cat!(
                    "View '",
                    &view.name,
                    "' of '",
                    db_fqn,
                    "': cannot infer its main table \u{2014} no non-join column references found"
                ),
                None,
            ));
        }
        if tables.len() > 1 {
            return Err(Thrown::model(
                Phase::Model,
                cat!(
                    "View '",
                    &view.name,
                    "' of '",
                    db_fqn,
                    "' references multiple root tables ",
                    java_list(&tables),
                    "; a view must resolve to a single root table"
                ),
                None,
            ));
        }
        Ok(tables.swap_remove(0))
    }

    fn join_only_view_root(
        &self,
        db_fqn: &JStr,
        view: &DatabaseDefinitionViewDefinition,
    ) -> Result<Option<JStr>, Thrown> {
        let mut terminals: Vec<JStr> = Vec::new();
        let mut first = None;
        for vc in view.column_mappings.iter() {
            let RelationalOperation::RelationalOperationJoinNavigation(jn) = &vc.expression else {
                continue;
            };
            if jn.chain.is_empty() {
                continue;
            }
            if first.is_none() {
                first = Some(jn.chain[0].clone());
            }
            if let Some(t) = &jn.terminal {
                for x in t.tables() {
                    if !terminals.contains(&x) {
                        terminals.push(x);
                    }
                }
            }
        }
        let Some(first) = first else {
            return Ok(None);
        };
        let join_db = first.database_name.clone().unwrap_or_else(|| db_fqn.clone());
        let join_name = &first.join_name;
        // the database's own join first, then the include closure, as every join lookup (`find_join`; the engine's
        // getJoin). Until 2026-11-13 only the database's own joins: the names fixes, item 6, ruling 28
        let jd = self
            .find_database(&join_db)
            .and_then(|d| d.joins.iter().find(|j| &j.name == join_name))
            .or_else(|| self.find_join(Some(&join_db), join_name))
            .ok_or_else(|| {
                Thrown::model(
                    Phase::Model,
                    cat!(
                        "View '",
                        &view.name,
                        "' of '",
                        db_fqn,
                        "' navigates join '",
                        join_name,
                        "' which '",
                        &join_db,
                        "' does not declare"
                    ),
                    None,
                )
            })?;
        let cond: Vec<JStr> = jd
            .operation
            .tables()
            .into_iter()
            .filter(|t| !terminals.contains(t))
            .collect();
        Ok(if cond.len() == 1 { Some(cond[0].clone()) } else { None })
    }

    /// `viewLift(db, name)`: the lift identity of the view reached as `name` from `db`.
    pub fn view_lift(&self, db_fqn: &JStr, view_name: &JStr) -> Option<&ViewLift> {
        self.indexed_view(db_fqn, view_name, &mut HashSet::new())
            .map(|iv| &iv.lift)
    }

    fn indexed_view(&self, db_fqn: &JStr, view_name: &JStr, seen: &mut HashSet<JStr>) -> Option<&IndexedView> {
        if !seen.insert(db_fqn.clone()) {
            return None;
        }
        let id = self.symbols.resolve_id(db_fqn);
        if id == UNRESOLVED {
            return None;
        }
        if let Some(own) = self.views_by_db.get(&id).and_then(|m| m.get(view_name)) {
            return Some(own);
        }
        if let Some(db) = self.find_database(db_fqn) {
            for inc in db.includes.iter() {
                if let Some(hit) = self.indexed_view(inc, view_name, seen) {
                    return Some(hit);
                }
            }
        }
        None
    }

    // ====================================================================
    // Iteration accessors: registration order
    // ====================================================================

    fn in_order<'a, T>(&'a self, slots: &'a [Option<T>]) -> impl Iterator<Item = &'a T> + 'a {
        self.element_order.order.iter().filter_map(move |&id| id_get(slots, id))
    }

    /// `hasElement(fqn)`: a registered element of this model (the symbol table's id and the registration order).
    pub fn has_element(&self, fqn: &JStr) -> bool {
        let id = self.symbols.resolve_id(fqn);
        id >= 0 && self.element_order.contains(id)
    }

    /// `classes()`.
    pub fn classes(&self) -> impl Iterator<Item = &Arc<ClassDefinition>> {
        self.in_order(&self.classes)
    }

    /// `associations()`.
    pub fn associations(&self) -> impl Iterator<Item = &Arc<AssociationDefinition>> {
        self.in_order(&self.associations)
    }

    /// `functions()`: every overload, the names in registration order, each name's overloads in theirs.
    pub fn functions(&self) -> impl Iterator<Item = &Function> {
        self.in_order(&self.functions).flatten()
    }

    /// `databases()`.
    pub fn databases(&self) -> impl Iterator<Item = &Arc<DatabaseDefinition>> {
        self.in_order(&self.databases)
    }

    /// `dataElements()`.
    pub fn data_elements(&self) -> impl Iterator<Item = &Arc<DataDefinition>> {
        self.in_order(&self.data_elements)
    }

    /// `mappings()`: the canonical mappings.
    pub fn mappings(&self) -> impl Iterator<Item = &Arc<MappingDefinition>> {
        self.in_order(&self.mappings)
    }

    /// `legacyMappings()`.
    pub fn legacy_mappings(&self) -> impl Iterator<Item = &Arc<LegacyMappingDefinition>> {
        self.in_order(&self.legacy_mappings)
    }

    /// `legacySetsWithId(setId)`: every legacy mapping's set whose effective id is `setId`, with its mapping (the
    /// mappings in registration order, each one's sets in its own order); built on first read.
    pub fn legacy_sets_with_id(&self, set_id: &JStr) -> &[LegacySet] {
        let idx = self.legacy_sets_by_id.get_or_init(|| {
            let mut built: HashMap<JStr, Vec<LegacySet>> = HashMap::new();
            for m in self.legacy_mappings() {
                for cm in m.class_mappings.iter() {
                    built
                        .entry(lite_model::names::set_id_of_class_mapping(cm))
                        .or_default()
                        .push(LegacySet {
                            defining: m.clone(),
                            set: cm.clone(),
                        });
                }
            }
            built
        });
        idx.get(set_id).map_or(&[], Vec::as_slice)
    }

    /// `enums()`.
    pub fn enums(&self) -> impl Iterator<Item = &Arc<EnumDefinition>> {
        self.in_order(&self.enums)
    }

    /// `measures()`.
    pub fn measures(&self) -> impl Iterator<Item = &Arc<MeasureDefinition>> {
        self.in_order(&self.measures)
    }

    /// `runtimes()`.
    pub fn runtimes(&self) -> impl Iterator<Item = &Arc<RuntimeDefinition>> {
        self.in_order(&self.runtimes)
    }

    /// `services()`.
    pub fn services(&self) -> impl Iterator<Item = &Arc<ServiceDefinition>> {
        self.in_order(&self.services)
    }

    /// `dataSpaces()`.
    pub fn data_spaces(&self) -> impl Iterator<Item = &Arc<DataSpaceDefinition>> {
        self.in_order(&self.data_spaces)
    }

    /// `connections()`: the relational connections, a runtime's hoisted inline ones among them (an inline one is not
    /// registered, so it is listed only when a registered element shares its name).
    pub fn connections(&self) -> impl Iterator<Item = &Arc<ConnectionDefinition>> {
        self.in_order(&self.connections)
    }

    /// `importsOf(key)`: the element's own import scope, else the model-wide one.
    pub fn imports_of(&self, element_key: &JStr) -> &Arc<ImportScope> {
        self.element_imports.get(element_key).unwrap_or(&self.imports)
    }

    /// `hasDatabaseExact(fqn)`: a registered database by exact FQN only.
    pub fn has_database_exact(&self, fqn: &JStr) -> bool {
        self.find_database(fqn).is_some()
    }

    /// `imports()`: the model-wide import scope.
    pub fn imports(&self) -> &Arc<ImportScope> {
        &self.imports
    }

    /// `symbols()`: the symbol table.
    pub fn symbols(&self) -> &SymbolTable {
        &self.symbols
    }
}

/// `StoreLookups`: the store lookups every reader of store facts shares, include-closure aware, own first.
pub trait StoreLookups {
    fn find_database(&self, db_fqn: Option<&JStr>) -> Option<&Arc<DatabaseDefinition>>;
    fn find_table_definition(&self, db_fqn: &JStr, name: &JStr) -> Option<&Table>;
    fn find_own_table_definition(&self, db_fqn: &JStr, name: &JStr) -> Option<&Table>;
    fn find_view(&self, db_fqn: &JStr, name: &JStr) -> Option<&View>;
    /// `findJoin(db, name)`: the join reached from `db` (own first, then the includes); none for no database (added
    /// 2026-11-13, the names follow-ups, S376's join terminal, ruling 34: a view's join navigation's terminal is checked
    /// once its joins resolve).
    fn find_join(&self, db_fqn: Option<&JStr>, join_name: &JStr) -> Option<&Arc<DatabaseDefinitionJoinDefinition>>;
}

impl StoreLookups for ModelIndex {
    fn find_database(&self, db_fqn: Option<&JStr>) -> Option<&Arc<DatabaseDefinition>> {
        db_fqn.and_then(|d| ModelIndex::find_database(self, d))
    }

    fn find_table_definition(&self, db_fqn: &JStr, name: &JStr) -> Option<&Table> {
        ModelIndex::find_table_definition(self, db_fqn, name)
    }

    fn find_own_table_definition(&self, db_fqn: &JStr, name: &JStr) -> Option<&Table> {
        ModelIndex::find_own_table_definition(self, db_fqn, name)
    }

    fn find_view(&self, db_fqn: &JStr, name: &JStr) -> Option<&View> {
        ModelIndex::find_view(self, db_fqn, name)
    }

    fn find_join(&self, db_fqn: Option<&JStr>, join_name: &JStr) -> Option<&Arc<DatabaseDefinitionJoinDefinition>> {
        ModelIndex::find_join(self, db_fqn, join_name)
    }
}

fn index_end(
    idx: &mut HashMap<JStr, HashMap<JStr, Vec<InjectedEnd>>>,
    ad: &Arc<AssociationDefinition>,
    owner_ref: &p::TypeExpression,
    end: &End,
) {
    if let p::TypeExpression::TypeExpressionNameRef(n) = owner_ref {
        idx.entry(n.name.clone())
            .or_default()
            .entry(end.property_name.clone())
            .or_default()
            .push(InjectedEnd {
                association: ad.clone(),
                end: end.clone(),
            });
    }
}

/// Two functions are the same object (Java's `f == fn` over `List<Function>`).
fn same_function(a: &Function, b: &Function) -> bool {
    match (a, b) {
        (Function::FunctionDefinition(x), Function::FunctionDefinition(y)) => same(x, y),
        (Function::NativeFunctionDefinition(x), Function::NativeFunctionDefinition(y)) => same(x, y),
        _ => false,
    }
}

/// Java's `AbstractCollection.toString()` over strings: `[a, b]`.
pub fn java_list(items: &[JStr]) -> JStr {
    let mut out: Vec<u16> = vec![u16::from(b'[')];
    for (i, s) in items.iter().enumerate() {
        if i > 0 {
            out.extend(", ".encode_utf16());
        }
        out.extend_from_slice(&s.0);
    }
    out.push(u16::from(b']'));
    JStr(out)
}
