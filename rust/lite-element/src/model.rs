// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `PureModelContext` (PureModelContext.java at 18feff3fe) as the concrete [`Model`], `ModelContext`'s signatures its
//! inherent methods: one implementation, so no trait (EF_CENSUS §2, §2.1's P1-P10). Every method is PMC's, its
//! name in snake case, its doc line naming PMC's method and line; Java's `Optional<X>` is `Result<Option<..>,
//! Thrown>` where Java can throw, else `Option<..>`; a `null`-for-absent list is an `Option`.
//!
//! **What the model holds** (the lead's design, from the rulings):
//! - `arena`: the model's [`ModelArena`], BORROWED (`'m`): this crate forbids `unsafe`, so it cannot own the arena its
//!   `Type<'m>`s point into; the owning handle, and the lifetime's erasure, are the FFI's (a later slice).
//! - `index`: the graph's one [`ModelIndex`], OWNED: [`Model::from`] takes it by value, adds E's products to it
//!   (`ModelIndex::add`) and retains the legacy surfaces, as PMC:176 does to the index E read.
//! - Built at freeze (census §4, Knowledge; ruling 4): the platform tables (`declarations`, `implementations`:
//!   neither build throws), the packages (`isPackage`), the element FQNs and the resolution universe as an ordered
//!   list (below) beside its `contains` predicate.
//! - The element cells (census §2.1 P4, §9.3): a `OnceLock` per FQN known at freeze -- a class cell per class of the
//!   index and of the native catalog, an enum cell per enum of either, a function cell per FQN with a native or a
//!   model function. An unknown FQN answers empty with no cell. A class or function cell caches its refusal and
//!   returns it again (Java recomputes the same answer); an enum's never fails. The function cell hands out one `Arc`
//!   per overload, so G's memo, keyed by the instance, sees one function once (census §9.1).
//! - `reach` (P2, the one derived fact F owns; ruling 4's named fields): a class's reach through its compiled
//!   supers, `Mutex<HashMap<JStr, Arc<Walked>>>`, computed OUTSIDE the lock (look, unlock, compute, insert if absent:
//!   Java's get-then-put, a rare double computation of a pure function harmless).
//! - Nothing of the overlay: [`Overlay`] is a view over a model (ruling 11, census ASK 5 (a)).
//!
//! **Self-reference.** The model owns its index, so it cannot hold the `TypeClassifier` and `FunctionCompiler` that
//! borrow it, as Java's fields do: they are views made per call ([`Model::classifier`], [`Model::function_compiler`]).
//!
//! **The boot layer** ([`CheckedLayer`], P1): the boot's elements by identity, one [`IdSet`] per kind over their
//! `Arc`s, and its functions' ids. A graph built over it must hold the boot's very `Arc`s, never clones, which it
//! does: `ModelIndex::add` clones an element's `Arc`, never its record, and skips the one already in its slot.
//!
//! **The resolution universe's order** (census ASK 6 (b), the lead's 1 (c)): Java iterates a `HashSet`, an order no
//! Rust collection reproduces; the ordered list is declared instead -- `element_fqns`' kinds in Java's order
//! (classes, enums, measures, associations, mappings, legacy mappings, databases, runtimes, functions, primitive
//! extensions, native enums), each FQN once at its first place in the index's order, then the platform's names
//! (`lite_names::resolver::platform_fqns()`, a set with no order of its own) not already there, in Java's `String`
//! order (UTF-16 units). Java's `contains` and its iteration disagree (a function id is a member and is never
//! iterated); both are kept as Java has them.
//!
//! RE-EXPRESSED (census §4): `derived` (an open `Class<?>`-keyed map) is named fields, `Reach` alone here;
//! `packages`, `declarations`, `implementations` built at freeze; `resolutionUniverse` a predicate plus an ordered
//! list; `Walked`'s rethrown exception the cell's cached refusal. DON'T PORT (census §3): the public constructors
//! (PMC:61, :68) and the wall-sink-only `from` (PMC:169); `ModelContext`'s 25 overridden default bodies.

use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::sync::{Arc, Mutex, OnceLock};

use lite_base::{Alloc, IdSet, ModelArena};
use lite_model::linked::LinkedMap;
use lite_model::{
    AssociationDefinition, AssociationDefinitionAssociationEndDefinition, ClassDefinition, ConnectionDefinition,
    DataDefinition, DataSpaceDefinition, DatabaseDefinition, DatabaseDefinitionJoinDefinition,
    DatabaseDefinitionTableDefinition, DatabaseDefinitionTableDefinitionMilestoning, DatabaseDefinitionViewDefinition,
    EnumDefinition, Function, FunctionDefinition, FunctionId, JStr, KeyThread, LegacyMappingDefinition,
    MappingDefinition, MeasureDefinition, NativeFunctionDefinition, NormalizedModel, PoisonKey,
    PoisonKeyForAssociation, PoisonKeyForClass, PoisonKeyForSet, ProfileDefinition, RuntimeDefinition,
};
use lite_names::{IndexUniverse, ModelIndex, StoreLookups, Thrown, Universe};
use lite_platform::{DeclarationTable, ImplementationTable, Registrations};
use lite_types::{RelationType, Type, platform};

use crate::class_compiler;
use crate::function_compiler::FunctionCompiler;
use crate::store_compiler;
use crate::type_classifier::TypeClassifier;
use crate::typed::{Property, TypedClass, TypedEnum, TypedFunction};

/// The concrete model (Java's `PureModelContext`): the graph's index, its frozen Knowledge, its lazy typed elements.
/// `Send + Sync`, shared by the threads planning on it; no `&mut` after freeze.
pub struct Model<'m> {
    arena: &'m ModelArena,
    index: ModelIndex,
    declarations: DeclarationTable,
    implementations: ImplementationTable,
    /// every proper prefix of an element FQN (`isPackage`)
    packages: HashSet<JStr>,
    /// `elementFqns()`, ordered (the module's docs)
    element_fqns: Arc<[JStr]>,
    /// the resolution universe's iteration, ordered (the module's docs)
    universe: Arc<[JStr]>,
    class_cells: HashMap<JStr, Cell<Arc<TypedClass<'m>>>>,
    enum_cells: HashMap<JStr, OnceLock<Arc<TypedEnum>>>,
    function_cells: HashMap<JStr, Cell<Overloads<'m>>>,
    reach: Mutex<HashMap<JStr, Arc<Walked>>>,
}

/// An element cell: set once, its compiled element or its refusal (`Thrown`), returned again.
type Cell<T> = OnceLock<Result<T, Thrown>>;

/// An FQN's overloads, one `Arc` per overload (the function cell hands each out once).
type Overloads<'m> = Arc<[Arc<TypedFunction<'m>>]>;

/// The model's shape, checked by the compiler on day one: shared between threads (PHASE3_DESIGN.md §2.6).
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Model<'static>>();
    assert_send_sync::<CheckedLayer>();
    assert_send_sync::<Walked>();
};

/// `Walked` (PMC:211): a class's reach (itself included) and the first refusal met on it (census §4: `(set,
/// Option<Refusal>)`; a class whose cell refuses is a dead end, its supers not walked).
#[derive(Debug)]
pub struct Walked {
    /// in the walk's order (Java's copies a `HashSet`'s; only `contains` reads it)
    classes: LinkedMap<JStr, ()>,
    failure: Option<Thrown>,
}

impl Walked {
    /// `classes().contains(fqn)`.
    pub fn contains(&self, fqn: &JStr) -> bool {
        self.classes.contains_key(fqn)
    }

    /// `failure()`: the first refusal on the reach, the class cell's cached one.
    pub fn failure(&self) -> Option<&Thrown> {
        self.failure.as_ref()
    }
}

/// `CheckedLayer` (PMC:102): a layer whose elements passed the integrity check on their own (the boot layer, once per
/// process) -- its elements by identity, one set per kind, and its functions' dispatch signatures. A graph built over
/// it checks only its own elements and the rules that span the two (`model_integrity::check`'s `prior`).
pub struct CheckedLayer {
    /// shared by [`CheckedLayer::without`]'s copies: an identity set has no iteration to copy it by
    elements: Arc<LayerElements>,
    signature_keys: HashSet<FunctionId>,
}

/// The layer's elements by identity (Java's one `IdentityHashMap` set over every kind; an `IdSet` holds one key
/// type, so one per kind: `checkLayer` adds classes, functions, associations, enums, databases, mappings).
struct LayerElements {
    classes: IdSet<Arc<ClassDefinition>>,
    functions: IdSet<Arc<FunctionDefinition>>,
    native_functions: IdSet<Arc<NativeFunctionDefinition>>,
    associations: IdSet<Arc<AssociationDefinition>>,
    enums: IdSet<Arc<EnumDefinition>>,
    databases: IdSet<Arc<DatabaseDefinition>>,
    mappings: IdSet<Arc<MappingDefinition>>,
}

impl CheckedLayer {
    /// `contains(cd)` (PMC:107) for a class: this very definition is the layer's.
    pub fn contains_class(&self, cd: &Arc<ClassDefinition>) -> bool {
        self.elements.classes.contains(cd)
    }

    /// `contains(f)` for a function, either variant.
    pub fn contains_function(&self, f: &Function) -> bool {
        match f {
            Function::FunctionDefinition(fd) => self.elements.functions.contains(fd),
            Function::NativeFunctionDefinition(nf) => self.elements.native_functions.contains(nf),
        }
    }

    /// `contains(a)` for an association.
    pub fn contains_association(&self, a: &Arc<AssociationDefinition>) -> bool {
        self.elements.associations.contains(a)
    }

    /// `contains(e)` for an enum.
    pub fn contains_enum(&self, e: &Arc<EnumDefinition>) -> bool {
        self.elements.enums.contains(e)
    }

    /// `contains(db)` for a database.
    pub fn contains_database(&self, db: &Arc<DatabaseDefinition>) -> bool {
        self.elements.databases.contains(db)
    }

    /// `contains(md)` for a mapping.
    pub fn contains_mapping(&self, md: &Arc<MappingDefinition>) -> bool {
        self.elements.mappings.contains(md)
    }

    /// `signatureKeys()` (PMC:111): the layer's functions' ids.
    pub fn signature_keys(&self) -> &HashSet<FunctionId> {
        &self.signature_keys
    }

    /// `without(adopted)` (PMC:118): this layer with the `adopted` ids no longer its signatures (a graph that took
    /// the platform's own Pure for them declares them itself), its elements the same (Java returns itself when
    /// `adopted` is empty: an equal layer here, the elements shared).
    pub fn without(&self, adopted: &HashSet<FunctionId>) -> CheckedLayer {
        CheckedLayer {
            elements: self.elements.clone(),
            signature_keys: self
                .signature_keys
                .iter()
                .filter(|k| !adopted.contains(*k))
                .cloned()
                .collect(),
        }
    }
}

impl<'m> Model<'m> {
    // ---- P1: construction and the F gate ----

    /// `from(normalized, index, wallSink, prior, registrations)` (PMC:176): THE Phase-E-to-F gate -- `index` (the one
    /// E read) gains E's products (`normalized.elements`) and retains the legacy surfaces; then the integrity pass
    /// (`model_integrity::check`, PMC:92: strict when `wall_sink` is none, else every failing element's first line
    /// in the sink), then the freeze (the module's docs). The model's types go in `arena`.
    pub fn from(
        arena: &'m ModelArena,
        normalized: &NormalizedModel,
        index: ModelIndex,
        wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
        prior: Option<&CheckedLayer>,
        registrations: &Registrations,
    ) -> Result<Model<'m>, Thrown> {
        unimplemented!("lite-element skeleton: PureModelContext.from")
    }

    /// `checkLayer(normalized, index, registrations)` (PMC:132): the layer checked on its own, strictly (a failure is
    /// a platform bug), over a local [`ModelArena`]; its classes, functions, associations, enums, databases and
    /// mappings by identity, its functions' ids. The caller (`Compiler.boot`, :371) reads nothing of `index` after.
    pub fn check_layer(
        normalized: &NormalizedModel,
        index: ModelIndex,
        registrations: &Registrations,
    ) -> Result<CheckedLayer, Thrown> {
        unimplemented!("lite-element skeleton: PureModelContext.checkLayer")
    }

    /// The type classifier over the index (PMC:45's field, a view per call).
    pub fn classifier(&self) -> TypeClassifier<'_> {
        TypeClassifier::new(&self.index)
    }

    /// The function compiler over the index (PMC:46's field, a view per call).
    pub fn function_compiler(&self) -> FunctionCompiler<'_> {
        FunctionCompiler::new(&self.index, self.classifier())
    }

    /// The graph's index (`model`, PMC:40): the legacy surfaces' archive and every lookup it answers.
    pub fn index(&self) -> &ModelIndex {
        &self.index
    }

    /// The model's arena, where its typed elements' types live.
    pub fn arena(&self) -> &'m ModelArena {
        self.arena
    }

    // ---- P2: subtyping ----

    /// `findType(fqn)` (PMC:193): the kind an FQN names (Knowledge, cheap: no structure built); its FQN in `arena`.
    pub fn find_type<'a, A: Alloc>(&self, arena: &'a A, fqn: &JStr) -> Option<Type<'a>> {
        self.classifier().find_type(arena, fqn)
    }

    /// `subtree(baseFqn)` (PMC:216): every strict subclass, graph and catalog, in discovery order (`KnowledgeLayer`).
    pub fn subtree(&self, base_fqn: &JStr) -> Vec<JStr> {
        self.index.knowledge().subtree(base_fqn)
    }

    /// `isSubtype(childFqn, parentFqn)` (PMC:221): the same name, or `parent` (or `Nil`, the bottom) on the child's
    /// reach; else the reach's first refusal, when it has one; else false.
    pub fn is_subtype(&self, child_fqn: &JStr, parent_fqn: &JStr) -> Result<bool, Thrown> {
        unimplemented!("lite-element skeleton: PureModelContext.isSubtype")
    }

    /// `reach(cls)` (PMC:240): the class's reach, breadth first through each class's compiled supers (its cell),
    /// `Nil` not walked past; memoized in `reach`, computed outside the lock.
    fn reach(&self, cls: &JStr) -> Arc<Walked> {
        unimplemented!("lite-element skeleton: PureModelContext.reach")
    }

    // ---- P4: the element memos ----

    /// `findClass(fqn)` (PMC:285): the typed class, compiled on first demand into its cell (the class compiler over
    /// the classifier's definition, the native catalog first), its refusal cached; none for an FQN that is no class.
    /// Borrowed from the model, so no field outlives it holding one (PHASE3_DESIGN.md §2.6:
    /// `NoEagerTypeReferencesTest` by type).
    pub fn find_class(&self, fqn: &JStr) -> Result<Option<&TypedClass<'m>>, Thrown> {
        let Some(cell) = self.class_cells.get(fqn) else {
            return Ok(None);
        };
        let classifier = self.classifier();
        let outcome = cell.get_or_init(|| {
            let cd = classifier
                .class_def(fqn)
                .unwrap_or_else(|| unreachable!("a class cell is made for a class the classifier defines"));
            class_compiler::compile(&classifier, self.arena, cd).map(Arc::new)
        });
        match outcome {
            Ok(tc) => Ok(Some(tc.as_ref())),
            Err(e) => Err(e.clone()),
        }
    }

    /// `findClassDefinition(fqn)` (PMC:298): the parsed definition, the native catalog first.
    pub fn find_class_definition(&self, fqn: &JStr) -> Option<&Arc<ClassDefinition>> {
        self.classifier().class_def(fqn)
    }

    /// `findEnum(fqn)` (PMC:372): the typed enum (its definition's name and values), made on first demand; the user
    /// model's first, then the native catalog's. Borrowed from the model, as a class is.
    pub fn find_enum(&self, fqn: &JStr) -> Option<&TypedEnum> {
        let cell = self.enum_cells.get(fqn)?;
        let typed = cell.get_or_init(|| {
            let def = self
                .classifier()
                .enum_def(fqn)
                .unwrap_or_else(|| unreachable!("an enum cell is made for an enum the classifier defines"));
            Arc::new(TypedEnum {
                qualified_name: def.qualified_name.clone(),
                values: def.values.clone(),
            })
        });
        Some(typed.as_ref())
    }

    /// `findFunctionById(qualifiedId)` (PMC:387): the typed overloads whose definition is a model function with that
    /// id or the native with it (model ones first), each found in its FQN's overload list by its definition.
    pub fn find_function_by_id(&self, qualified_id: &JStr) -> Result<Vec<Arc<TypedFunction<'m>>>, Thrown> {
        unimplemented!("lite-element skeleton: PureModelContext.findFunctionById")
    }

    /// `findFunction(fqn)` (PMC:405): every overload at `fqn`, compiled on first demand into its cell
    /// (`FunctionCompiler::compile_all`: natives first, then the model's), one `Arc` per overload; its refusal cached;
    /// none for an FQN with no function.
    pub fn find_function(&self, fqn: &JStr) -> Result<&[Arc<TypedFunction<'m>>], Thrown> {
        let Some(cell) = self.function_cells.get(fqn) else {
            return Ok(&[]);
        };
        let outcome = cell.get_or_init(|| {
            self.function_compiler()
                .compile_all(self.arena, fqn)
                .map(|overloads| overloads.into_iter().map(Arc::new).collect())
        });
        match outcome {
            Ok(overloads) => Ok(overloads),
            Err(e) => Err(e.clone()),
        }
    }

    /// `findProperty(classFqn, name)` (PMC:416, with its path, :428): the most derived declaration of `name` on the
    /// class or a superclass, else an association end injected onto the class (classified at lookup, in `arena`, its
    /// bounds checked in `ModelIntegrity`'s words under its association: `require_valid_bounds_at`); a class met again
    /// on its own path an "Inheritance cycle" refusal attributed to the class the walk starts from.
    pub fn find_property<'a, A: Alloc>(
        &'a self,
        arena: &'a A,
        class_fqn: &JStr,
        name: &JStr,
    ) -> Result<Option<Property<'a>>, Thrown> {
        unimplemented!("lite-element skeleton: PureModelContext.findProperty")
    }

    // ---- P5: the name universe ----

    /// `resolutionUniverse().contains(name)` (PMC:320): a platform name, a primitive extension, an element of the
    /// graph, or a graph function's signature id.
    pub fn resolution_universe_contains(&self, name: &JStr) -> bool {
        IndexUniverse { index: &self.index }.contains(name)
    }

    /// `resolutionUniverse()`'s iteration (PMC:329): the element FQNs, then the platform's names, ordered (the
    /// module's docs).
    pub fn resolution_universe(&self) -> &[JStr] {
        &self.universe
    }

    /// `elementFqns()` (PMC:345): every element FQN of the kinds Java lists, the primitive extensions and the native
    /// enums, each once, ordered (the module's docs).
    pub fn element_fqns(&self) -> &[JStr] {
        &self.element_fqns
    }

    /// `isPackage(fqn)` (PMC:477): a proper prefix of an element FQN (by `::` segments).
    pub fn is_package(&self, fqn: &JStr) -> bool {
        self.packages.contains(fqn)
    }

    /// `functionFqns()` (PMC:857): every model function's FQN, each once, in the index's order (Java's is a
    /// `HashSet`: the order its one reader, G's eager compile, iterates is G's question).
    pub fn function_fqns(&self) -> Vec<JStr> {
        unimplemented!("lite-element skeleton: PureModelContext.functionFqns")
    }

    // ---- P6: the mapping facts E stamped ----

    /// `findMapping(fqn)` (PMC:503).
    pub fn find_mapping(&self, fqn: &JStr) -> Option<&Arc<MappingDefinition>> {
        self.index.find_mapping(fqn)
    }

    /// `findLegacyMapping(fqn)` (PMC:304): the pre-Door-1 surface, an analysis archive.
    pub fn find_legacy_mapping(&self, fqn: &JStr) -> Option<&Arc<LegacyMappingDefinition>> {
        self.index.find_legacy_mapping(fqn)
    }

    /// `unionMemberClasses(mappingFqn, classFqn)` (PMC:513): none for an unknown mapping or a class it maps by no
    /// union.
    pub fn union_member_classes(&self, mapping_fqn: &JStr, class_fqn: &JStr) -> Option<&Arc<[JStr]>> {
        self.find_mapping(mapping_fqn)?.facts.union_members_of(class_fqn)
    }

    /// `routedTargetClass(mappingFqn, ownerClass, prop)` (PMC:520).
    pub fn routed_target_class(&self, mapping_fqn: &JStr, owner_class: &JStr, prop: &JStr) -> Option<&JStr> {
        self.find_mapping(mapping_fqn)?
            .facts
            .routed_target_class(owner_class, prop)
    }

    /// `mixedUnionMembers(mappingFqn, classFqn)` (PMC:529).
    pub fn mixed_union_members(&self, mapping_fqn: &JStr, class_fqn: &JStr) -> Option<&Arc<[JStr]>> {
        self.find_mapping(mapping_fqn)?.facts.mixed_union_of(class_fqn)
    }

    /// `unionKeyThreads(mappingFqn, classFqn)` (PMC:536).
    pub fn union_key_threads(&self, mapping_fqn: &JStr, class_fqn: &JStr) -> Option<&Arc<[Arc<KeyThread>]>> {
        self.find_mapping(mapping_fqn)?.facts.union_key_threads_of(class_fqn)
    }

    /// `mappingPoison(mappingFqn, classFqn)` (PMC:546): the class's poison (`PoisonKey.ForClass`).
    pub fn mapping_poison(&self, mapping_fqn: &JStr, class_fqn: &JStr) -> Option<&JStr> {
        let key = PoisonKey::PoisonKeyForClass(Arc::new(PoisonKeyForClass {
            class_fqn: class_fqn.clone(),
        }));
        self.find_mapping(mapping_fqn)?.facts.poison(&key)
    }

    /// `mappingSetPoison(mappingFqn, classFqn, setId)` (PMC:552): the set's poison (`PoisonKey.ForSet`).
    pub fn mapping_set_poison(&self, mapping_fqn: &JStr, class_fqn: &JStr, set_id: &JStr) -> Option<&JStr> {
        let key = PoisonKey::PoisonKeyForSet(Arc::new(PoisonKeyForSet {
            class_fqn: class_fqn.clone(),
            set_id: set_id.clone(),
        }));
        self.find_mapping(mapping_fqn)?.facts.poison(&key)
    }

    /// `mappingAssociationPoison(mappingFqn, associationFqn)` (PMC:560): the association's poison
    /// (`PoisonKey.ForAssociation`).
    pub fn mapping_association_poison(&self, mapping_fqn: &JStr, association_fqn: &JStr) -> Option<&JStr> {
        let key = PoisonKey::PoisonKeyForAssociation(Arc::new(PoisonKeyForAssociation {
            association_fqn: association_fqn.clone(),
        }));
        self.find_mapping(mapping_fqn)?.facts.poison(&key)
    }

    // ---- P7: the execution elements, and the overlay ----

    /// `findRuntime(fqn)` (PMC:567): the model's runtime (an [`Overlay`] answers its own first).
    pub fn find_runtime(&self, fqn: &JStr) -> Option<&Arc<RuntimeDefinition>> {
        self.index.find_runtime(fqn)
    }

    /// `findConnection(fqn)` (PMC:666): the model's connection (an [`Overlay`] answers its own first).
    pub fn find_connection(&self, fqn: &JStr) -> Option<&Arc<ConnectionDefinition>> {
        self.index.find_connection(fqn)
    }

    /// `isExecutionContextElement(fqn)` (PMC:703): a mapping, a legacy mapping, a runtime, a connection or a
    /// database (an [`Overlay`] counts its own runtime and connection).
    pub fn is_execution_context_element(&self, fqn: &JStr) -> bool {
        self.index.find_mapping(fqn).is_some()
            || self.index.find_legacy_mapping(fqn).is_some()
            || self.find_runtime(fqn).is_some()
            || self.find_connection(fqn).is_some()
            || self.index.find_database(fqn).is_some()
    }

    /// `withExecutionOverlay(runtime, connection)` (PMC:621, ruling 11): a view resolving exactly the two supplied
    /// elements, everything else this model's; refused (Java's two `IllegalArgumentException`s, their texts) when
    /// either would shadow a model element, or the runtime names a mapping the model has not.
    pub fn with_execution_overlay(
        &self,
        runtime: Arc<RuntimeDefinition>,
        connection: Arc<ConnectionDefinition>,
    ) -> Result<Overlay<'_, 'm>, Thrown> {
        unimplemented!("lite-element skeleton: PureModelContext.withExecutionOverlay")
    }

    // ---- P8: the platform tables ----

    /// `declarations()` (PMC:645): the catalog's natives and the model's functions, one per id; built at freeze.
    pub fn declarations(&self) -> &DeclarationTable {
        &self.declarations
    }

    /// `implementations()` (PMC:656): what executes each declaration; built at freeze from the declarations and the
    /// platform's registrations.
    pub fn implementations(&self) -> &ImplementationTable {
        &self.implementations
    }

    // ---- P9: the metamodel registry ----

    /// `tracksClassifier(classifierFqn)` (PMC:816): one of the five classifiers the metamodel store tracks (Class,
    /// Enumeration, Association, Mapping, Database).
    pub fn tracks_classifier(&self, classifier_fqn: &JStr) -> bool {
        [
            platform::CLASS_METACLASS,
            platform::ENUMERATION,
            ASSOCIATION_METACLASS,
            platform::MAPPING,
            platform::DATABASE,
        ]
        .iter()
        .any(|c| classifier_fqn.eq_str(c))
    }

    /// `classifierInstances(classifierFqn)` (PMC:821): a tracked classifier's extent, distinct, in Java's `String`
    /// order (UTF-16 units, TRIAGE B5); none for an untracked one (a user class: the store lane's).
    pub fn classifier_instances(&self, classifier_fqn: &JStr) -> Option<Vec<JStr>> {
        unimplemented!("lite-element skeleton: PureModelContext.classifierInstances")
    }

    // ---- P10: the store lookups and the delegations ----

    /// `findMeasure(fqn)` (PMC:472).
    pub fn find_measure(&self, fqn: &JStr) -> Option<&Arc<MeasureDefinition>> {
        self.index.find_measure(fqn)
    }

    /// `findProfile(fqn)` (PMC:498).
    pub fn find_profile(&self, fqn: &JStr) -> Option<&Arc<ProfileDefinition>> {
        self.index.find_profile(fqn)
    }

    /// `findData(fqn)` (PMC:578).
    pub fn find_data(&self, fqn: &JStr) -> Option<&Arc<DataDefinition>> {
        self.index.find_data(fqn)
    }

    /// `findDataSpace(fqn)` (PMC:584).
    pub fn find_data_space(&self, fqn: &JStr) -> Option<&Arc<DataSpaceDefinition>> {
        self.index.find_data_space(fqn)
    }

    /// `dataElements()` (PMC:590), in the index's order.
    pub fn data_elements(&self) -> impl Iterator<Item = &Arc<DataDefinition>> {
        self.index.data_elements()
    }

    /// `isModelConnection(fqn)` (PMC:676).
    pub fn is_model_connection(&self, fqn: &JStr) -> bool {
        self.index.is_model_connection(fqn)
    }

    /// `databases()` (PMC:681), in the index's order.
    pub fn databases(&self) -> impl Iterator<Item = &Arc<DatabaseDefinition>> {
        self.index.databases()
    }

    /// `findAssociationOf(ownerClassFqn, propName)` (PMC:686): the association injecting `prop_name` onto the class
    /// or an ancestor (the index's refusal, as it throws).
    pub fn find_association_of(
        &self,
        owner_class_fqn: &JStr,
        prop_name: &JStr,
    ) -> Result<Option<&Arc<AssociationDefinition>>, Thrown> {
        self.index.find_association_of(owner_class_fqn, prop_name)
    }

    /// `findAssociation(fqn)` (PMC:692).
    pub fn find_association(&self, fqn: &JStr) -> Option<&Arc<AssociationDefinition>> {
        self.index.find_association(fqn)
    }

    /// `findAssociationEnd(ownerClassFqn, propName)` (PMC:697).
    pub fn find_association_end(
        &self,
        owner_class_fqn: &JStr,
        prop_name: &JStr,
    ) -> Option<&Arc<AssociationDefinitionAssociationEndDefinition>> {
        self.index.find_association_end(owner_class_fqn, prop_name)
    }

    /// `findFunctionDefinitions(fqn)` (PMC:713): every model function definition at `fqn` (no native), in
    /// declaration order.
    pub fn find_function_definitions(&self, fqn: &JStr) -> Vec<&Arc<FunctionDefinition>> {
        self.index
            .find_function(fqn)
            .iter()
            .filter_map(|f| match f {
                Function::FunctionDefinition(fd) => Some(fd),
                Function::NativeFunctionDefinition(_) => None,
            })
            .collect()
    }

    /// `findFunctionDefinition(fqn)` (PMC:725): the first of them.
    pub fn find_function_definition(&self, fqn: &JStr) -> Option<&Arc<FunctionDefinition>> {
        self.index.find_function(fqn).iter().find_map(|f| match f {
            Function::FunctionDefinition(fd) => Some(fd),
            Function::NativeFunctionDefinition(_) => None,
        })
    }

    /// `findOwnTableDefinition(dbFqn, name)` (PMC:736): the table `db` itself declares.
    pub fn find_own_table_definition(
        &self,
        db_fqn: &JStr,
        name: &JStr,
    ) -> Option<&Arc<DatabaseDefinitionTableDefinition>> {
        self.index.find_own_table_definition(db_fqn, name)
    }

    /// `findView(dbFqn, name)` (PMC:742): the view reached from `db` (include closure; `SCHEMA.NAME` or bare).
    pub fn find_view(&self, db_fqn: &JStr, name: &JStr) -> Option<&Arc<DatabaseDefinitionViewDefinition>> {
        self.index.find_view(db_fqn, name)
    }

    /// `viewMainTable(dbFqn, view)` (PMC:747): the index's one rule, loud when the view has no single root.
    pub fn view_main_table(&self, db_fqn: &JStr, view: &DatabaseDefinitionViewDefinition) -> Result<JStr, Thrown> {
        self.index.view_main_table(db_fqn, view)
    }

    /// `findView(dbFqn, schema, name)` (PMC:752): by schema (`default` or none: the bare spelling) and name.
    pub fn find_view_in_schema(
        &self,
        db_fqn: &JStr,
        schema: Option<&JStr>,
        name: &JStr,
    ) -> Option<&Arc<DatabaseDefinitionViewDefinition>> {
        self.index.find_view_in_schema(db_fqn, schema, name)
    }

    /// `findViewFunction(dbFqn, name)` (PMC:758): the view's lifted function (`<owner>$view$<spelling>`), the first
    /// overload at its name.
    pub fn find_view_function(&self, db_fqn: &JStr, name: &JStr) -> Result<Option<&Arc<TypedFunction<'m>>>, Thrown> {
        let Some(lift) = self.index.view_lift(db_fqn, name) else {
            return Ok(None);
        };
        Ok(self.find_function(&lift.fqn()?)?.first())
    }

    /// `findTable(dbFqn, name)` (PMC:764): the table reached from `db`, as its row struct (`table_schema`, in
    /// `arena`).
    pub fn find_table<'a, A: Alloc>(
        &self,
        arena: &'a A,
        db_fqn: &JStr,
        name: &JStr,
    ) -> Result<Option<&'a RelationType<'a>>, Thrown> {
        self.index
            .find_table_definition(db_fqn, name)
            .map(|table| store_compiler::table_schema(arena, table))
            .transpose()
    }

    /// `findTableMilestoning(dbFqn, name)` (PMC:771): the table's milestoning block, its own (the table `db`
    /// declares) first, then each include's, each database once.
    pub fn find_table_milestoning(
        &self,
        db_fqn: &JStr,
        name: &JStr,
    ) -> Option<&Arc<DatabaseDefinitionTableDefinitionMilestoning>> {
        unimplemented!("lite-element skeleton: PureModelContext.findTableMilestoning")
    }

    /// `milestoningWithIncludes(db, name, seen)` (PMC:783).
    fn milestoning_with_includes(
        &self,
        db: &DatabaseDefinition,
        name: &JStr,
        seen: &mut HashSet<JStr>,
    ) -> Option<&Arc<DatabaseDefinitionTableDefinitionMilestoning>> {
        unimplemented!("lite-element skeleton: PureModelContext.milestoningWithIncludes")
    }

    /// `isDatabase(fqn)` (PMC:864).
    pub fn is_database(&self, fqn: &JStr) -> bool {
        self.index.find_database(fqn).is_some()
    }

    /// `findDatabase(dbFqn)` (PMC:869): none for none.
    pub fn find_database(&self, db_fqn: Option<&JStr>) -> Option<&Arc<DatabaseDefinition>> {
        db_fqn.and_then(|d| self.index.find_database(d))
    }

    /// `findJoinDefinition(dbFqn, joinName)` (PMC:875): include-closure aware, own first.
    pub fn find_join_definition(
        &self,
        db_fqn: Option<&JStr>,
        join_name: &JStr,
    ) -> Option<&Arc<DatabaseDefinitionJoinDefinition>> {
        self.index.find_join(db_fqn, join_name)
    }

    /// `findTableDefinition(dbFqn, name)` (PMC:881): include-closure aware, own first.
    pub fn find_table_definition(&self, db_fqn: &JStr, name: &JStr) -> Option<&Arc<DatabaseDefinitionTableDefinition>> {
        self.index.find_table_definition(db_fqn, name)
    }
}

/// `TRACKED_CLASSIFIERS`' association metaclass (PMC:808), spelled where Java spells it (no platform constant).
const ASSOCIATION_METACLASS: &str = "meta::pure::metamodel::relationship::Association";

/// `ModelContext extends StoreLookups`: the model answers the store lookups every reader of store facts shares.
impl StoreLookups for Model<'_> {
    fn find_database(&self, db_fqn: Option<&JStr>) -> Option<&Arc<DatabaseDefinition>> {
        Model::find_database(self, db_fqn)
    }

    fn find_table_definition(&self, db_fqn: &JStr, name: &JStr) -> Option<&Arc<DatabaseDefinitionTableDefinition>> {
        Model::find_table_definition(self, db_fqn, name)
    }

    fn find_own_table_definition(&self, db_fqn: &JStr, name: &JStr) -> Option<&Arc<DatabaseDefinitionTableDefinition>> {
        Model::find_own_table_definition(self, db_fqn, name)
    }

    fn find_view(&self, db_fqn: &JStr, name: &JStr) -> Option<&Arc<DatabaseDefinitionViewDefinition>> {
        Model::find_view(self, db_fqn, name)
    }

    /// `findJoin` (ModelContext:341): `findJoinDefinition`, relayed.
    fn find_join(&self, db_fqn: Option<&JStr>, join_name: &JStr) -> Option<&Arc<DatabaseDefinitionJoinDefinition>> {
        Model::find_join_definition(self, db_fqn, join_name)
    }
}

/// The resolver's universe over a compiled model (`Compiler.resolveQuery`, :678, asks `contains` alone).
impl Universe for Model<'_> {
    fn contains(&self, name: &JStr) -> bool {
        self.resolution_universe_contains(name)
    }
}

/// The execution overlay (PMC:596's constructor, ruling 11; census ASK 5 (a)): a view of a model that resolves the
/// driver-supplied runtime and connection (a harness's per-test context, a service's ambient runtime), everything
/// else the model's, shared -- its typed cells, its reach, its tables. It derefs to the model; it overrides the three
/// methods that read the two elements (`find_runtime`, `find_connection`, `is_execution_context_element`), which no
/// other method of the model calls.
pub struct Overlay<'a, 'm> {
    base: &'a Model<'m>,
    runtime: Arc<RuntimeDefinition>,
    connection: Arc<ConnectionDefinition>,
}

impl<'a, 'm> Overlay<'a, 'm> {
    /// `findRuntime(fqn)` (PMC:567): the overlay's runtime by its name, else the model's.
    pub fn find_runtime(&self, fqn: &JStr) -> Option<&Arc<RuntimeDefinition>> {
        if self.runtime.qualified_name == *fqn {
            return Some(&self.runtime);
        }
        self.base.find_runtime(fqn)
    }

    /// `findConnection(fqn)` (PMC:666): the overlay's connection by its name, else the model's.
    pub fn find_connection(&self, fqn: &JStr) -> Option<&Arc<ConnectionDefinition>> {
        if self.connection.qualified_name == *fqn {
            return Some(&self.connection);
        }
        self.base.find_connection(fqn)
    }

    /// `isExecutionContextElement(fqn)` (PMC:703), the overlay's runtime and connection counted.
    pub fn is_execution_context_element(&self, fqn: &JStr) -> bool {
        let index = self.base.index();
        index.find_mapping(fqn).is_some()
            || index.find_legacy_mapping(fqn).is_some()
            || self.find_runtime(fqn).is_some()
            || self.find_connection(fqn).is_some()
            || index.find_database(fqn).is_some()
    }
}

impl<'m> Deref for Overlay<'_, 'm> {
    type Target = Model<'m>;

    fn deref(&self) -> &Model<'m> {
        self.base
    }
}
