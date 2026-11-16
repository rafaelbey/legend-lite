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

use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Deref;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};

use lite_base::{Alloc, IdSet, ModelArena};
use lite_builtin::pure::catalog;
use lite_error::Phase;
use lite_model::linked::LinkedMap;
use lite_model::{
    AssociationDefinition, AssociationDefinitionAssociationEndDefinition, ClassDefinition, ConnectionDefinition,
    DataDefinition, DataSpaceDefinition, DatabaseDefinition, DatabaseDefinitionJoinDefinition,
    DatabaseDefinitionTableDefinition, DatabaseDefinitionTableDefinitionMilestoning, DatabaseDefinitionViewDefinition,
    EnumDefinition, Function, FunctionDefinition, FunctionId, JStr, KeyThread, LegacyMappingDefinition,
    MappingDefinition, MeasureDefinition, NativeFunctionDefinition, NormalizedModel, PoisonKey,
    PoisonKeyForAssociation, PoisonKeyForClass, PoisonKeyForSet, ProfileDefinition, RuntimeDefinition, cat,
};
use lite_names::resolver::platform_fqns;
use lite_names::{IndexUniverse, ModelIndex, StoreLookups, Thrown, Universe};
use lite_platform::{DeclarationTable, ImplementationTable, Registrations};
use lite_types::{RelationType, Type, platform};

use crate::class_compiler;
use crate::function_compiler::FunctionCompiler;
use crate::model_integrity;
use crate::store_compiler;
use crate::type_classifier::TypeClassifier;
use crate::typed::{Property, PropertyStored, TypedClass, TypedEnum, TypedFunction};

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
        let index = gate(normalized, index, wall_sink, prior)?;
        Ok(Model::freeze(arena, index, registrations))
    }

    /// `checkLayer(normalized, index, registrations)` (PMC:132): the layer checked on its own, strictly (a failure is
    /// a platform bug), over a local [`ModelArena`]; its classes, functions, associations, enums, databases and
    /// mappings by identity, its functions' ids. The caller (`Compiler.boot`, :371) reads nothing of `index` after.
    pub fn check_layer(
        normalized: &NormalizedModel,
        index: ModelIndex,
        registrations: &Registrations,
    ) -> Result<CheckedLayer, Thrown> {
        let arena = ModelArena::new();
        let alone = Model::from(&arena, normalized, index, None, None, registrations)?;
        Ok(layer_of(alone.index()))
    }

    /// The freeze (2026-11-15; census §4, Knowledge built at freeze, ruling 4): what Java's constructor (PMC:74-93)
    /// and its lazy fields leave to first use, built once over the checked index -- the declaration table
    /// (`declarations`, PMC:645: the catalog's natives, then the model's functions) and the implementation table
    /// (`implementations`, PMC:656), the element FQNs (`elementFqns`, PMC:345) and the resolution universe's
    /// iteration in the module's declared order, the packages (`isPackage`, PMC:477), and the element cells: a class
    /// cell per FQN `classDef` answers (the index's classes, the catalog's native classes), an enum cell per FQN
    /// `enumDef` answers (the index's enums, the catalog's), a function cell per FQN with a native or a model
    /// function. Nothing here throws (neither table build does: they collect their dangling and conflicting ids).
    fn freeze(arena: &'m ModelArena, index: ModelIndex, registrations: &Registrations) -> Model<'m> {
        let natives = catalog();
        let declarations = DeclarationTable::of(
            natives
                .all()
                .iter()
                .map(|n| Function::NativeFunctionDefinition(n.clone()))
                .chain(index.functions().cloned()),
        );
        let implementations = ImplementationTable::build(&declarations, registrations);
        let element_fqns = element_fqns_of(&index);
        let packages = packages_of(&element_fqns);
        let universe = universe_of(&element_fqns);
        let class_cells = index
            .classes()
            .map(|c| &c.qualified_name)
            .chain(natives.native_classes().iter().map(|c| &c.qualified_name))
            .map(|fqn| (fqn.clone(), OnceLock::new()))
            .collect();
        let enum_cells = index
            .enums()
            .map(|e| &e.qualified_name)
            .chain(natives.native_enums().iter().map(|e| &e.qualified_name))
            .map(|fqn| (fqn.clone(), OnceLock::new()))
            .collect();
        let function_cells = natives
            .all()
            .iter()
            .map(|n| &n.qualified_name)
            .chain(index.functions().map(Function::qualified_name))
            .map(|fqn| (fqn.clone(), OnceLock::new()))
            .collect();
        Model {
            arena,
            index,
            declarations,
            implementations,
            packages,
            element_fqns: Arc::from(element_fqns),
            universe: Arc::from(universe),
            class_cells,
            enum_cells,
            function_cells,
            reach: Mutex::new(HashMap::new()),
        }
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
        if child_fqn == parent_fqn {
            return Ok(true);
        }
        let r = self.reach(child_fqn)?;
        // Nil is the BOTTOM type -- a subtype of every type (ModelContext's walk: reaching it answers true whatever
        // the parent)
        if r.contains(parent_fqn) || r.contains(&NIL) {
            return Ok(true);
        }
        // a NO walked everything reachable, so a class that failed to compile on the way is this question's failure
        if let Some(failure) = r.failure() {
            return Err(failure.clone());
        }
        Ok(false)
    }

    /// `reach(cls)` (PMC:240): the class's reach, breadth first through each class's compiled supers (its cell),
    /// `Nil` not walked past; memoized in `reach`, computed outside the lock. PMC:257 catches a
    /// `LegendCompileException` alone: such a refusal of a class on the way is the walk's first failure (that class a
    /// dead end), anything else (a bound's raw `IllegalArgumentException`, a `NotImplementedException`) escapes the
    /// walk and nothing is memoized, as Java's throw leaves `Reach` unwritten (2026-11-15: so this returns a
    /// `Result`, where the skeleton's did not).
    fn reach(&self, cls: &JStr) -> Result<Arc<Walked>, Thrown> {
        if let Some(known) = self.reach_memo().get(cls) {
            return Ok(known.clone());
        }
        let mut classes: LinkedMap<JStr, ()> = LinkedMap::new();
        let mut failure: Option<Thrown> = None;
        let mut work: VecDeque<JStr> = VecDeque::from([cls.clone()]);
        while let Some(cur) = work.pop_front() {
            if !classes.put_if_absent(cur.clone(), ()) || cur == *NIL {
                continue;
            }
            match self.find_class(&cur) {
                Ok(Some(tc)) => work.extend(tc.super_class_fqns.iter().cloned()),
                Ok(None) => {}
                Err(e) if is_legend_compile_exception(&e) => {
                    if failure.is_none() {
                        failure = Some(e);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        let walked = Arc::new(Walked { classes, failure });
        self.reach_memo().entry(cls.clone()).or_insert_with(|| walked.clone());
        Ok(walked)
    }

    /// The `Reach` memo's lock (`derived(Reach.class, ...)`, PMC:204-207, :241; a panic is never raised while it is
    /// held: nothing is computed under it).
    fn reach_memo(&self) -> MutexGuard<'_, HashMap<JStr, Arc<Walked>>> {
        self.reach.lock().unwrap_or_else(PoisonError::into_inner)
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
        let mut defs: Vec<Function> = self.index.find_function_by_id(qualified_id).to_vec();
        if let Some(n) = catalog().native_function_by_id(qualified_id) {
            defs.push(Function::NativeFunctionDefinition(n.def().clone()));
        }
        let mut out = Vec::with_capacity(defs.len());
        for d in &defs {
            for tf in self.find_function(d.qualified_name())? {
                // `d.equals(tf.definition())`: Java's record equality, which leaves the protocol records' source
                // positions out (`Function::java_equals`; the derived `PartialEq` reads them: a graph's copy of a
                // catalog native, parsed elsewhere, was another function -- the element differential, 2026-11-15)
                if d.java_equals(&tf.definition) {
                    out.push(tf.clone());
                }
            }
        }
        Ok(out)
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
        self.find_property_along(arena, class_fqn, name, &mut Vec::new())
    }

    /// `findProperty(classFqn, name, path)` (PMC:428): the lookup along `path`, the classes it is inside, its start
    /// first (Java's `LinkedHashSet`, which the walk adds to and removes from at its end only: a stack here). A class
    /// met again on its own path is the "Inheritance cycle" refusal (a `ModelException`, MODEL, attributed to the
    /// path's first class); a class met twice on two paths (a diamond) is walked again. The order: the class's own
    /// declarations, then each supertype's whole lookup (its association ends included), then the ends injected onto
    /// the class itself -- the target classified first, then its association (none is Java's
    /// `IllegalStateException`), then its bounds (`require_valid_bounds_at`), then its multiplicity.
    fn find_property_along<'a, A: Alloc>(
        &'a self,
        arena: &'a A,
        class_fqn: &JStr,
        name: &JStr,
        path: &mut Vec<JStr>,
    ) -> Result<Option<Property<'a>>, Thrown> {
        if path.contains(class_fqn) {
            let mut joined: Vec<u16> = Vec::new();
            for (i, c) in path.iter().enumerate() {
                if i > 0 {
                    joined.extend(" -> ".encode_utf16());
                }
                joined.extend_from_slice(&c.0);
            }
            return Err(Thrown::model(
                Phase::Model,
                cat!("Inheritance cycle: ", joined, " -> ", class_fqn),
                Some(&path[0]),
            ));
        }
        path.push(class_fqn.clone());
        let Some(tc) = self.find_class(class_fqn)? else {
            path.pop();
            return Ok(None);
        };
        if let Some(p) = tc.properties.iter().find(|p| p.name() == name) {
            return Ok(Some(p.clone()));
        }
        for super_fqn in tc.super_class_fqns.iter() {
            if let Some(inherited) = self.find_property_along(arena, super_fqn, name, path)? {
                return Ok(Some(inherited));
            }
        }
        path.pop();
        // the contract's third leg: ASSOCIATION-INJECTED navigation properties resolve at lookup time from the
        // association index, never stored on the typed class; a superclass's are found through the recursion above
        let Some(end) = self.index.find_association_end(class_fqn, name) else {
            return Ok(None);
        };
        let target = self.classifier().classify(arena, &end.target_class, &[])?;
        // a tolerant build keeps a walled association whose end's bounds are inverted: the integrity check's refusal,
        // in its words and under its element (PMC:456-465)
        let association = self.index.find_association_of(class_fqn, name)?.ok_or_else(|| {
            Thrown::illegal_state(cat!("an association end with no association: ", class_fqn, ".", name))
        })?;
        model_integrity::require_valid_bounds_at(
            &end.multiplicity,
            &model_integrity::association_end_site(end, association),
            &association.qualified_name,
        )?;
        Ok(Some(Property::Stored(PropertyStored::new(
            end.property_name.clone(),
            target,
            TypeClassifier::multiplicity(arena, &end.multiplicity)?,
        ))))
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
        let mut seen: HashSet<&JStr> = HashSet::new();
        self.index
            .functions()
            .map(Function::qualified_name)
            .filter(|fqn| seen.insert(*fqn))
            .cloned()
            .collect()
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
        // ADD, never SHADOW: an overlay must not redefine an element the MODEL declares
        if self.index.find_runtime(&runtime.qualified_name).is_some()
            || self.index.find_connection(&connection.qualified_name).is_some()
        {
            return Err(Thrown::illegal_argument(cat!(
                "execution overlay would shadow a model-declared element: '",
                &runtime.qualified_name,
                "' / '",
                &connection.qualified_name,
                "'"
            )));
        }
        // EAGER ref check: a typo'd mapping fails HERE by name, never mid-query
        for m in runtime.mappings.iter() {
            if self.find_mapping(m).is_none() && self.find_legacy_mapping(m).is_none() {
                return Err(Thrown::illegal_argument(cat!(
                    "execution overlay runtime '",
                    &runtime.qualified_name,
                    "' names unknown mapping '",
                    m,
                    "'"
                )));
            }
        }
        Ok(Overlay {
            base: self,
            runtime,
            connection,
        })
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
        // the registry table: tracked classifiers answer their extent, everything else none (a user class: the store
        // lane owns it); the tracked set (`tracks_classifier`) IS this dispatch's domain
        let natives = catalog();
        let mut fqns: Vec<JStr> = if classifier_fqn.eq_str(platform::CLASS_METACLASS) {
            self.index
                .classes()
                .map(|c| c.qualified_name.clone())
                .chain(natives.native_classes().iter().map(|c| c.qualified_name.clone()))
                .collect()
        } else if classifier_fqn.eq_str(platform::ENUMERATION) {
            self.index
                .enums()
                .map(|e| e.qualified_name.clone())
                .chain(natives.native_enums().iter().map(|e| e.qualified_name.clone()))
                .collect()
        } else if classifier_fqn.eq_str(ASSOCIATION_METACLASS) {
            self.index.associations().map(|a| a.qualified_name.clone()).collect()
        } else if classifier_fqn.eq_str(platform::MAPPING) {
            self.index.mappings().map(|m| m.qualified_name.clone()).collect()
        } else if classifier_fqn.eq_str(platform::DATABASE) {
            // the store extent (metamodel-store tables seed from it)
            self.index.databases().map(|d| d.qualified_name.clone()).collect()
        } else {
            return None;
        };
        // `.distinct().sorted()`: Java's `String` order is its UTF-16 units', `JStr`'s `Ord`
        fqns.sort();
        fqns.dedup();
        Some(fqns)
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
        let db = self.index.find_database(db_fqn)?;
        self.milestoning_with_includes(db, name, &mut HashSet::new())
    }

    /// `milestoningWithIncludes(db, name, seen)` (PMC:783): the milestoning of the table `db` itself declares, when it
    /// declares one; else each include not yet in `seen` (which starts without the database asked), depth first, the
    /// first found. A table of `db`'s own with no milestoning block goes on to the includes, as Java's `Optional.map`
    /// of a null block is empty.
    fn milestoning_with_includes(
        &self,
        db: &DatabaseDefinition,
        name: &JStr,
        seen: &mut HashSet<JStr>,
    ) -> Option<&Arc<DatabaseDefinitionTableDefinitionMilestoning>> {
        if let Some(own) = self
            .index
            .find_own_table_definition(&db.qualified_name, name)
            .and_then(|t| t.milestoning.as_ref())
        {
            return Some(own);
        }
        for include in db.includes.iter() {
            if !seen.insert(include.clone()) {
                continue;
            }
            let inc = self
                .index
                .find_database(include)
                .and_then(|d| self.milestoning_with_includes(d, name, seen));
            if inc.is_some() {
                return inc;
            }
        }
        None
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

/// `PlatformTypes.NIL` (PMC:229, :252), the bottom type, as the reach's sets hold names.
static NIL: LazyLock<JStr> = LazyLock::new(|| JStr::from(platform::NIL));

/// The F gate (`from`, PMC:184-189, then the constructor's PMC:92): the index E read gains E's products (`index.add`
/// of `normalized.elements`: the pass-through structural elements are already there, by identity, and skipped), then
/// retains each pre-Door-1 mapping surface as an analysis archive, in the surfaces' order; then THE integrity pass
/// over it, strict without a sink, tolerant with one, `prior` the already-checked layer.
fn gate(
    normalized: &NormalizedModel,
    mut index: ModelIndex,
    wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
    prior: Option<&CheckedLayer>,
) -> Result<ModelIndex, Thrown> {
    index.add(&normalized.elements)?;
    for (_, surface) in normalized.legacy_surfaces.iter() {
        index.retain_legacy_surface(surface);
    }
    let classifier = TypeClassifier::new(&index);
    let functions = FunctionCompiler::new(&index, classifier);
    model_integrity::check(&index, &classifier, &functions, wall_sink, prior)?;
    Ok(index)
}

/// `checkLayer`'s layer (PMC:135-145): the index's classes, functions (each also by its `FunctionId`), associations,
/// enums, databases and mappings, by identity (the index's very `Arc`s, which the layer holds).
fn layer_of(index: &ModelIndex) -> CheckedLayer {
    let mut elements = LayerElements {
        classes: IdSet::new(),
        functions: IdSet::new(),
        native_functions: IdSet::new(),
        associations: IdSet::new(),
        enums: IdSet::new(),
        databases: IdSet::new(),
        mappings: IdSet::new(),
    };
    let mut signature_keys: HashSet<FunctionId> = HashSet::new();
    for cd in index.classes() {
        elements.classes.insert(cd.clone());
    }
    for f in index.functions() {
        match f {
            Function::FunctionDefinition(fd) => elements.functions.insert(fd.clone()),
            Function::NativeFunctionDefinition(nf) => elements.native_functions.insert(nf.clone()),
        };
        signature_keys.insert(FunctionId::of(f));
    }
    for a in index.associations() {
        elements.associations.insert(a.clone());
    }
    for e in index.enums() {
        elements.enums.insert(e.clone());
    }
    for db in index.databases() {
        elements.databases.insert(db.clone());
    }
    for md in index.mappings() {
        elements.mappings.insert(md.clone());
    }
    CheckedLayer {
        elements: Arc::new(elements),
        signature_keys,
    }
}

/// `elementFqns()` (PMC:345), ordered (the module's docs): the index's classes, enums, measures, associations,
/// mappings, legacy mappings, databases, runtimes and functions, then its primitive extensions, then the catalog's
/// native enums -- each FQN once, at its first place.
fn element_fqns_of(index: &ModelIndex) -> Vec<JStr> {
    let mut out: LinkedMap<JStr, ()> = LinkedMap::new();
    let mut add = |fqn: &JStr| {
        out.put_if_absent(fqn.clone(), ());
    };
    index.classes().for_each(|e| add(&e.qualified_name));
    index.enums().for_each(|e| add(&e.qualified_name));
    index.measures().for_each(|e| add(&e.qualified_name));
    index.associations().for_each(|e| add(&e.qualified_name));
    index.mappings().for_each(|e| add(&e.qualified_name));
    index.legacy_mappings().for_each(|e| add(&e.qualified_name));
    index.databases().for_each(|e| add(&e.qualified_name));
    index.runtimes().for_each(|e| add(&e.qualified_name));
    index.functions().for_each(|f| add(f.qualified_name()));
    // primitive extensions (Primitive X extends Integer): without them a simple name under a wildcard import never
    // qualifies (leg 7b R0)
    index.primitive_extension_fqns().for_each(&mut add);
    // platform-native enums resolve like parsed ones (`findEnum` reads the native catalog too)
    catalog().native_enums().iter().for_each(|e| add(&e.qualified_name));
    out.iter().map(|(fqn, ())| fqn.clone()).collect()
}

/// `resolutionUniverse()`'s iteration (PMC:329), ordered (the module's docs, census ASK 6 (b)): the element FQNs, then
/// the platform's names not among them, in Java's `String` order (UTF-16 units: `JStr`'s `Ord`).
fn universe_of(element_fqns: &[JStr]) -> Vec<JStr> {
    let members: HashSet<&JStr> = element_fqns.iter().collect();
    let mut platform_names: Vec<JStr> = platform_fqns()
        .iter()
        .filter(|fqn| !members.contains(fqn))
        .cloned()
        .collect();
    platform_names.sort();
    let mut out = element_fqns.to_vec();
    out.extend(platform_names);
    out
}

/// `isPackage`'s index (PMC:478-490, built at freeze): for each element FQN, its parts (`el.split("::")`), and every
/// proper prefix of them joined by `::`.
fn packages_of(element_fqns: &[JStr]) -> HashSet<JStr> {
    let mut packages: HashSet<JStr> = HashSet::new();
    for el in element_fqns {
        let segs = split_colons(&el.0);
        let mut prefix: Vec<u16> = Vec::new();
        for (i, seg) in segs.iter().take(segs.len().saturating_sub(1)).enumerate() {
            if i > 0 {
                prefix.extend("::".encode_utf16());
            }
            prefix.extend_from_slice(seg);
            packages.insert(JStr(prefix.clone()));
        }
    }
    packages
}

/// Java's `s.split("::")` (PMC:480, no limit): the parts between the non-overlapping `::`s found from the left, a leading empty
/// part kept (the match at 0 has width), the trailing empty parts dropped; a text with no `::` is its one part, an
/// empty text included.
fn split_colons(s: &[u16]) -> Vec<&[u16]> {
    const COLON: u16 = b':' as u16;
    let mut parts: Vec<&[u16]> = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i + 1 < s.len() {
        if s[i] == COLON && s[i + 1] == COLON {
            parts.push(&s[start..i]);
            i += 2;
            start = i;
        } else {
            i += 1;
        }
    }
    if parts.is_empty() {
        return vec![s];
    }
    parts.push(&s[start..]);
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    parts
}

/// PMC:257's `catch (LegendCompileException e)`: a `com.legend.error` class that is a `LegendCompileException` (not a
/// `NotImplementedException` and its kin, not a raw JDK exception).
fn is_legend_compile_exception(e: &Thrown) -> bool {
    matches!(e, Thrown::Compile(c) if c.kind.is_compile_error())
}

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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lite_base::{QueryArena, same};
    use lite_error::CompileError;
    use lite_front::doors::Dialect;
    use lite_model::linked::{empty, list};
    use lite_model::{PackageableElement, ParsedModel};
    use lite_types::{Multiplicity, Primitive};

    use super::*;

    fn j(s: &str) -> JStr {
        JStr::from(s)
    }

    /// `rust/lite-normalizer/src/test_support.rs`' `model` (copied): a model text parsed and its names resolved
    /// (strict), as E receives it.
    fn parsed(text: &str) -> ParsedModel {
        let units: Vec<u16> = text.encode_utf16().collect();
        let parsed = lite_builtin::door::parse(&units, Dialect::LegendLite).expect("the model parses");
        lite_names::resolver::resolve(&parsed).expect("the model's names resolve")
    }

    /// `PureModelContextTest.asNormalized` (:44): the parsed elements as E's output (no body site to lift, no legacy
    /// surface), so E would be a no-op.
    fn as_normalized(parsed: &ParsedModel) -> NormalizedModel {
        NormalizedModel {
            elements: parsed.elements.clone(),
            imports: parsed.imports.clone(),
            legacy_surfaces: empty(),
        }
    }

    fn registrations() -> Registrations {
        Registrations::of_catalog(catalog().all().to_vec())
    }

    /// The model frozen over the text's index with no integrity pass (the gate's check is `model_integrity`'s, a
    /// sibling family): what the model's own lookups answer, apart from the gate.
    fn frozen<'m>(arena: &'m ModelArena, text: &str) -> Model<'m> {
        let p = parsed(text);
        Model::freeze(
            arena,
            ModelIndex::from(&p).expect("the model indexes"),
            &registrations(),
        )
    }

    /// `Phases.context(asNormalized(parsed))`: the strict F gate over the text's own index.
    fn context<'m>(arena: &'m ModelArena, text: &str) -> Result<Model<'m>, Thrown> {
        let p = parsed(text);
        Model::from(
            arena,
            &as_normalized(&p),
            ModelIndex::from(&p)?,
            None,
            None,
            &registrations(),
        )
    }

    /// The tolerant F gate over the text's own index, its walls in `walls`.
    fn tolerant<'m>(arena: &'m ModelArena, text: &str, walls: &mut LinkedMap<JStr, JStr>) -> Result<Model<'m>, Thrown> {
        let p = parsed(text);
        Model::from(
            arena,
            &as_normalized(&p),
            ModelIndex::from(&p)?,
            Some(walls),
            None,
            &registrations(),
        )
    }

    /// A typed class as the class compiler makes one for `Class fqn extends supers.. { props..: String[1]; }`, by
    /// hand: these tests hold the model's walks over its cells, not the class compiler that fills them.
    fn typed_class<'m>(arena: &'m ModelArena, fqn: &str, supers: &[&str], props: &[&str]) -> TypedClass<'m> {
        TypedClass {
            qualified_name: j(fqn),
            type_parameters: empty(),
            super_class_fqns: list(supers.iter().map(|s| j(s)).collect()),
            super_types: list(supers.iter().map(|s| Type::ClassType(arena.alloc_str(s))).collect()),
            properties: list(
                props
                    .iter()
                    .map(|p| {
                        Property::Stored(PropertyStored::new(
                            j(p),
                            Type::Primitive(Primitive::String),
                            Multiplicity::ONE,
                        ))
                    })
                    .collect(),
            ),
            constraints: empty(),
            is_native: false,
            multiplicity_parameters: empty(),
        }
    }

    /// The class cell of `fqn` set to `outcome`, as its first ask would set it.
    fn prefill<'m>(model: &Model<'m>, fqn: &str, outcome: Result<TypedClass<'m>, Thrown>) {
        model
            .class_cells
            .get(&j(fqn))
            .expect("a class cell")
            .set(outcome.map(Arc::new))
            .expect("an unset cell");
    }

    fn class_cell_is_set(model: &Model<'_>, fqn: &str) -> bool {
        model.class_cells.get(&j(fqn)).expect("a class cell").get().is_some()
    }

    fn refusal(message: &str) -> Thrown {
        Thrown::model(Phase::Model, j(message), None)
    }

    fn compile_error(t: &Thrown) -> &CompileError {
        match t {
            Thrown::Compile(e) => e,
            Thrown::Java { .. } => panic!("a com.legend.error class, got {t:?}"),
        }
    }

    const STRING: &str = "meta::pure::metamodel::type::String";
    const INTEGER: &str = "meta::pure::metamodel::type::Integer";
    const NUMBER: &str = "meta::pure::metamodel::type::Number";
    const DATE_TIME_FORMAT: &str = "meta::pure::functions::date::DateTimeFormat";

    /// `PureModelContextTest.fixture` (:27).
    fn fixture_text() -> String {
        format!(
            "Class model::Address {{ street: {STRING}[1]; }}\n\
             Class model::Person {{\n  name: {STRING}[1];\n  age: {INTEGER}[1];\n  home: model::Address[0..1];\n}}\n\
             Enum model::Color {{ RED, GREEN }}\n"
        )
    }

    // ---- P4: the enum cells (no sibling family needed) ----

    #[test]
    fn an_enum_cell_is_made_on_first_demand_and_holds_its_names_and_values() {
        let arena = ModelArena::new();
        let model = frozen(&arena, &fixture_text());
        let color = j("model::Color");
        let native = j(DATE_TIME_FORMAT);
        assert!(model.enum_cells[&color].get().is_none());
        let typed = model.find_enum(&color).expect("the user enum");
        assert_eq!(typed.qualified_name, color);
        assert_eq!(typed.values.to_vec(), vec![j("RED"), j("GREEN")]);
        // asked once: its cell is set; the native enum, unasked, is not
        assert!(model.enum_cells[&color].get().is_some());
        assert!(model.enum_cells[&native].get().is_none());
        assert!(model.find_enum(&native).is_some());
        // memoized: the same instance again
        assert!(same(&model.find_enum(&color).unwrap(), &typed));
        // an unknown FQN answers none and has no cell
        assert!(model.find_enum(&j("model::Nope")).is_none());
        assert!(!model.enum_cells.contains_key(&j("model::Nope")));
    }

    // ---- P9: the metamodel registry ----

    #[test]
    fn classifier_instances_are_the_tracked_extents_sorted_and_distinct() {
        let arena = ModelArena::new();
        let model = frozen(&arena, &fixture_text());
        // PureModelContextTest.classifierInstancesClassExtent (:381)
        let classes = model.classifier_instances(&j(platform::CLASS_METACLASS)).unwrap();
        assert!(classes.contains(&j("model::Person")) && classes.contains(&j("model::Address")));
        assert!(classes.contains(&catalog().native_classes()[0].qualified_name));
        assert!(classes.is_sorted() && classes.windows(2).all(|w| w[0] != w[1]));
        assert_eq!(
            classes.len(),
            2 + catalog()
                .native_classes()
                .iter()
                .map(|c| &c.qualified_name)
                .collect::<HashSet<_>>()
                .len()
        );
        // classifierInstancesEnumExtent (:400)
        let enums = model.classifier_instances(&j(platform::ENUMERATION)).unwrap();
        assert!(enums.contains(&j("model::Color")) && enums.contains(&j(DATE_TIME_FORMAT)));
        assert!(enums.is_sorted());
        // classifierInstancesUntrackedIsNull (:412): untracked none, tracked but empty the empty list
        assert_eq!(model.classifier_instances(&j("model::Person")), None);
        assert_eq!(model.classifier_instances(&j(ASSOCIATION_METACLASS)), Some(vec![]));
        assert_eq!(model.classifier_instances(&j(platform::MAPPING)), Some(vec![]));
        assert_eq!(model.classifier_instances(&j(platform::DATABASE)), Some(vec![]));
        for tracked in [
            platform::CLASS_METACLASS,
            platform::ENUMERATION,
            ASSOCIATION_METACLASS,
            platform::MAPPING,
            platform::DATABASE,
        ] {
            assert!(model.tracks_classifier(&j(tracked)));
        }
        assert!(!model.tracks_classifier(&j("model::Person")));
    }

    #[test]
    fn an_extent_is_in_javas_string_order_utf16_units() {
        // `String.compareTo`: by unit, case-sensitive (no collation): `B` (0x42) < `_` (0x5F) < `a` (0x61)
        let arena = ModelArena::new();
        let model = frozen(&arena, "Class w::a {}\nClass w::_x {}\nClass w::B {}\nClass w::Ba {}\n");
        let ours: Vec<JStr> = model
            .classifier_instances(&j(platform::CLASS_METACLASS))
            .unwrap()
            .into_iter()
            .filter(|c| c.lossy().starts_with("w::"))
            .collect();
        assert_eq!(ours, vec![j("w::B"), j("w::Ba"), j("w::_x"), j("w::a")]);
        // by UTF-16 unit, not by code point (the parser's names are ASCII, so the units are shown on `JStr` itself):
        // U+10400 (0xD801 0xDC00) before U+FF21 (0xFF21)
        let mut names = vec![j("w::\u{FF21}"), j("w::\u{10400}")];
        names.sort();
        assert_eq!(names, vec![j("w::\u{10400}"), j("w::\u{FF21}")]);
    }

    // ---- P5: the name universe ----

    const UNIVERSE_TEXT: &str = "Class w::P { name: String[1]; }\n\
        Class w::Q { }\n\
        Enum w::E { X }\n\
        Association w::PQ { p: w::P[1]; q: w::Q[*]; }\n\
        Primitive w::Ext extends Integer\n\
        function w::f(x: String[1]): String[1] { $x }\n\
        function w::f(x: Integer[1]): String[1] { 'n' }\n\
        function w::g(): String[1] { 'g' }\n";

    #[test]
    fn the_element_fqns_are_javas_kinds_in_order_each_once() {
        let arena = ModelArena::new();
        let model = frozen(&arena, UNIVERSE_TEXT);
        let mut expected = vec![
            j("w::P"),
            j("w::Q"),
            j("w::E"),
            j("w::PQ"),
            j("w::f"),
            j("w::g"),
            j("w::Ext"),
        ];
        let mut seen: HashSet<JStr> = expected.iter().cloned().collect();
        for e in catalog().native_enums() {
            if seen.insert(e.qualified_name.clone()) {
                expected.push(e.qualified_name.clone());
            }
        }
        assert_eq!(model.element_fqns().to_vec(), expected);
    }

    #[test]
    fn the_universe_is_the_element_fqns_then_the_platforms_names_sorted() {
        let arena = ModelArena::new();
        let model = frozen(&arena, UNIVERSE_TEXT);
        let universe = model.resolution_universe();
        let elements = model.element_fqns();
        assert_eq!(&universe[..elements.len()], elements);
        let rest = &universe[elements.len()..];
        assert!(rest.is_sorted());
        let mut platform: Vec<&JStr> = platform_fqns().iter().filter(|f| !elements.contains(f)).collect();
        platform.sort();
        assert_eq!(rest.iter().collect::<Vec<_>>(), platform);
        // contains: a platform name, a primitive extension, an element, a function's signature id -- which is a
        // member and is never iterated (Java's contains and iteration disagree, kept)
        let id = FunctionId::of(&model.index().find_function(&j("w::g"))[0]).qualified;
        for member in [platform_fqns().iter().next().unwrap(), &j("w::Ext"), &j("w::P"), &id] {
            assert!(model.resolution_universe_contains(member), "{member:?}");
            assert!(Universe::contains(&model, member));
        }
        assert!(!universe.contains(&id));
        assert!(!model.resolution_universe_contains(&j("absent::Name")));
    }

    #[test]
    fn a_package_is_every_proper_prefix_of_an_element_fqn() {
        let arena = ModelArena::new();
        let model = frozen(&arena, &fixture_text());
        for package in ["model", "meta", "meta::pure", "meta::pure::functions::date"] {
            assert!(model.is_package(&j(package)), "{package}");
        }
        for not in ["model::Person", "absent", "", DATE_TIME_FORMAT, "mod"] {
            assert!(!model.is_package(&j(not)), "{not}");
        }
    }

    #[test]
    fn a_package_split_is_javas_string_split() {
        let parts = |s: &str| {
            split_colons(&j(s).0)
                .into_iter()
                .map(|p| String::from_utf16(p).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(parts("a::b::C"), ["a", "b", "C"]);
        assert_eq!(parts("C"), ["C"]);
        assert_eq!(parts(""), [""]);
        // a leading empty part is kept, the trailing empty ones dropped, matches are non-overlapping from the left
        assert_eq!(parts("::a"), ["", "a"]);
        assert_eq!(parts("a::"), ["a"]);
        assert_eq!(parts("::"), Vec::<String>::new());
        assert_eq!(parts("a::::b"), ["a", "", "b"]);
        assert_eq!(parts("a:::b"), ["a", ":b"]);
        let packages = |fqns: &[&str]| {
            let mut out: Vec<String> = packages_of(&fqns.iter().map(|f| j(f)).collect::<Vec<_>>())
                .into_iter()
                .map(|p| p.lossy())
                .collect();
            out.sort();
            out
        };
        assert_eq!(packages(&["a::b::C"]), ["a", "a::b"]);
        assert_eq!(packages(&["a::::b"]), ["a", "a::"]);
        assert_eq!(packages(&["::a"]), [""]);
        assert_eq!(packages(&["a::", "x", ""]), Vec::<String>::new());
    }

    #[test]
    fn the_function_fqns_are_each_model_function_once_in_index_order() {
        let arena = ModelArena::new();
        let model = frozen(&arena, UNIVERSE_TEXT);
        assert_eq!(model.function_fqns(), vec![j("w::f"), j("w::g")]);
    }

    // ---- P7: the overlay ----

    const OVERLAY_MODEL: &str = "###Relational\nDatabase w::S ( Table T (ID INTEGER) )\n\
        ###Mapping\nMapping w::M ( )\n\
        ###Connection\n\
        RelationalDatabaseConnection w::C { store: w::S; type: H2; specification: LocalH2 {}; auth: DefaultH2; }\n\
        ###Runtime\nRuntime w::R { mappings: [w::M]; connections: [ w::S: [ id1: w::C ] ]; }\n";

    /// The driver-supplied connections (`o::C`, and `w::C`, a model's name): parsed from a text of their own.
    fn supplied_connection(fqn: &str) -> Arc<ConnectionDefinition> {
        let p = parsed(
            "###Relational\nDatabase o::S ( Table T (ID INTEGER) )\n###Connection\n\
             RelationalDatabaseConnection o::C { store: o::S; type: H2; specification: LocalH2 {}; auth: DefaultH2; }\n\
             RelationalDatabaseConnection w::C { store: o::S; type: H2; specification: LocalH2 {}; auth: DefaultH2; }\n",
        );
        p.elements
            .iter()
            .find_map(|e| match e {
                PackageableElement::ConnectionDefinition(c) if c.qualified_name == j(fqn) => Some(c.clone()),
                _ => None,
            })
            .expect("the connection")
    }

    /// A driver-supplied runtime of `mappings`.
    fn supplied_runtime(fqn: &str, mappings: &[&str]) -> Arc<RuntimeDefinition> {
        Arc::new(RuntimeDefinition {
            qualified_name: j(fqn),
            mappings: list(mappings.iter().map(|m| j(m)).collect()),
            connection_bindings: empty(),
            json_connections: empty(),
            inline_connections: empty(),
            connection_ids: empty(),
        })
    }

    #[test]
    fn the_overlay_resolves_its_two_elements_and_everything_else_the_models() {
        let arena = ModelArena::new();
        let model = frozen(&arena, OVERLAY_MODEL);
        let runtime = supplied_runtime("o::R", &["w::M"]);
        let connection = supplied_connection("o::C");
        let overlay = model
            .with_execution_overlay(runtime.clone(), connection.clone())
            .expect("an overlay that adds");
        // its three overrides
        assert!(same(overlay.find_runtime(&j("o::R")).unwrap(), &runtime));
        assert!(same(overlay.find_connection(&j("o::C")).unwrap(), &connection));
        assert!(overlay.is_execution_context_element(&j("o::R")));
        assert!(overlay.is_execution_context_element(&j("o::C")));
        // the model's elements through it, and the model itself unchanged
        assert!(same(
            overlay.find_runtime(&j("w::R")).unwrap(),
            model.find_runtime(&j("w::R")).unwrap()
        ));
        assert!(overlay.find_connection(&j("w::C")).is_some());
        for fqn in ["w::M", "w::R", "w::C", "w::S"] {
            assert!(overlay.is_execution_context_element(&j(fqn)), "{fqn}");
            assert!(model.is_execution_context_element(&j(fqn)), "{fqn}");
        }
        assert!(model.find_runtime(&j("o::R")).is_none() && model.find_connection(&j("o::C")).is_none());
        assert!(!model.is_execution_context_element(&j("o::R")));
        assert!(!overlay.is_execution_context_element(&j("w::Nope")));
        // it derefs to the model: its lookups and its cells are the model's
        let base: &Model<'_> = &overlay;
        assert!(same(&base, &&model));
        assert!(same(
            &overlay.find_enum(&j(DATE_TIME_FORMAT)).unwrap(),
            &model.find_enum(&j(DATE_TIME_FORMAT)).unwrap()
        ));
    }

    #[test]
    fn an_overlay_that_would_shadow_or_names_an_unknown_mapping_is_refused() {
        let arena = ModelArena::new();
        let model = frozen(&arena, OVERLAY_MODEL);
        let refused = |runtime: Arc<RuntimeDefinition>, connection: Arc<ConnectionDefinition>| match model
            .with_execution_overlay(runtime, connection)
        {
            Err(Thrown::Java { class, message }) => (class, message.unwrap().lossy()),
            Err(e) => panic!("an IllegalArgumentException, got {e:?}"),
            Ok(_) => panic!("an overlay refused"),
        };
        let iae = lite_names::error::ILLEGAL_ARGUMENT;
        assert_eq!(
            refused(supplied_runtime("w::R", &["w::M"]), supplied_connection("o::C")),
            (
                iae,
                "execution overlay would shadow a model-declared element: 'w::R' / 'o::C'".to_string()
            )
        );
        assert_eq!(
            refused(supplied_runtime("o::R", &["w::M"]), supplied_connection("w::C")),
            (
                iae,
                "execution overlay would shadow a model-declared element: 'o::R' / 'w::C'".to_string()
            )
        );
        assert_eq!(
            refused(
                supplied_runtime("o::R", &["w::M", "w::Missing"]),
                supplied_connection("o::C")
            ),
            (
                iae,
                "execution overlay runtime 'o::R' names unknown mapping 'w::Missing'".to_string()
            )
        );
    }

    // ---- P10: the milestoning delegation ----

    #[test]
    fn a_tables_milestoning_is_its_own_else_its_includes_each_database_once() {
        let arena = ModelArena::new();
        let model = frozen(
            &arena,
            "###Relational\n\
             Database w::Base ( Table T ( milestoning( business(BUS_FROM = FROM_Z, BUS_THRU = THRU_Z) ) \
               ID INTEGER PRIMARY KEY, FROM_Z DATE, THRU_Z DATE ) Table U ( ID INTEGER ) )\n\
             Database w::Top ( include w::Base Table U ( ID INTEGER ) Table V ( ID INTEGER ) )\n\
             Database w::X ( include w::Y Table P ( ID INTEGER ) )\n\
             Database w::Y ( include w::X )\n",
        );
        let own = model
            .find_own_table_definition(&j("w::Base"), &j("T"))
            .and_then(|t| t.milestoning.as_ref())
            .expect("the base table's block");
        assert!(same(model.find_table_milestoning(&j("w::Base"), &j("T")).unwrap(), own));
        // through the include
        assert!(same(model.find_table_milestoning(&j("w::Top"), &j("T")).unwrap(), own));
        // an own table with no block goes on to the includes, which have none either
        assert!(model.find_table_milestoning(&j("w::Top"), &j("U")).is_none());
        assert!(model.find_table_milestoning(&j("w::Top"), &j("V")).is_none());
        // an include cycle ends; an unknown database is none
        assert!(model.find_table_milestoning(&j("w::X"), &j("Q")).is_none());
        assert!(model.find_table_milestoning(&j("w::Y"), &j("P")).is_none());
        assert!(model.find_table_milestoning(&j("absent::Db"), &j("T")).is_none());
    }

    // ---- P2: subtyping, over cells set by hand ----

    const HIERARCHY: &str = "Class w::Child {}\nClass w::Bad {}\nClass w::Ok {}\nClass w::Child2 {}\n\
        Class w::Bad2 {}\nClass w::Child3 {}\nClass w::Mid {}\nClass w::Raw {}\nClass w::RawChild {}\n\
        Class w::Todo {}\nClass w::TodoChild {}\nClass w::N {}\nClass w::Late {}\n";

    fn hierarchy(arena: &ModelArena) -> Model<'_> {
        let model = frozen(arena, HIERARCHY);
        prefill(
            &model,
            "w::Child",
            Ok(typed_class(arena, "w::Child", &["w::Bad", "w::Ok"], &[])),
        );
        prefill(&model, "w::Bad", Err(refusal("Bad is walled")));
        prefill(&model, "w::Ok", Ok(typed_class(arena, "w::Ok", &[], &[])));
        prefill(
            &model,
            "w::Child2",
            Ok(typed_class(arena, "w::Child2", &["w::Bad", "w::Bad2"], &[])),
        );
        prefill(&model, "w::Bad2", Err(refusal("Bad2 is walled")));
        prefill(
            &model,
            "w::Child3",
            Ok(typed_class(arena, "w::Child3", &["w::Mid", "w::Bad2"], &[])),
        );
        prefill(&model, "w::Mid", Ok(typed_class(arena, "w::Mid", &["w::Bad"], &[])));
        prefill(
            &model,
            "w::Raw",
            Err(Thrown::illegal_argument(j("upper (1) must be >= lower (2)"))),
        );
        prefill(
            &model,
            "w::RawChild",
            Ok(typed_class(arena, "w::RawChild", &["w::Ok", "w::Raw"], &[])),
        );
        prefill(
            &model,
            "w::Todo",
            Err(Thrown::Compile(CompileError::not_implemented(j("not built").0))),
        );
        prefill(
            &model,
            "w::TodoChild",
            Ok(typed_class(arena, "w::TodoChild", &["w::Todo"], &[])),
        );
        // Nil's own cell (a catalog class) is never asked: not walked past
        prefill(&model, "w::N", Ok(typed_class(arena, "w::N", &[platform::NIL], &[])));
        model
    }

    #[test]
    fn subtyping_is_the_same_name_or_a_class_on_the_reach_or_nil() {
        let arena = ModelArena::new();
        let model = hierarchy(&arena);
        // the same name, asked of nothing (even an unknown one)
        assert_eq!(model.is_subtype(&j("absent::A"), &j("absent::A")), Ok(true));
        assert_eq!(model.is_subtype(&j("w::Ok"), &j("w::Ok")), Ok(true));
        assert_eq!(model.is_subtype(&j("w::Child"), &j("w::Ok")), Ok(true));
        // a class with no cell is a dead end, a no
        assert_eq!(model.is_subtype(&j("absent::A"), &j("w::Ok")), Ok(false));
        assert_eq!(model.is_subtype(&j("w::Ok"), &j("w::Child")), Ok(false));
        // Nil, the bottom: reaching it answers yes whatever the parent
        assert_eq!(model.is_subtype(&j("w::N"), &j("w::Anything")), Ok(true));
        assert_eq!(model.is_subtype(&j("w::N"), &j(platform::NIL)), Ok(true));
    }

    #[test]
    fn a_refusal_on_the_reach_answers_a_no_and_a_raw_throw_escapes_the_walk() {
        let arena = ModelArena::new();
        let model = hierarchy(&arena);
        // a yes stands: the walled class is on the reach (added before its cell is asked), and so is its sibling
        assert_eq!(model.is_subtype(&j("w::Child"), &j("w::Ok")), Ok(true));
        assert_eq!(model.is_subtype(&j("w::Child"), &j("w::Bad")), Ok(true));
        // a no is the walk's first failure
        assert_eq!(
            model.is_subtype(&j("w::Child"), &j("w::Other")),
            Err(refusal("Bad is walled"))
        );
        assert_eq!(
            model.is_subtype(&j("w::Child2"), &j("w::Other")),
            Err(refusal("Bad is walled"))
        );
        // breadth first: Child3 -> Mid, Bad2 -> Bad, so Bad2's refusal is met first
        assert_eq!(
            model.is_subtype(&j("w::Child3"), &j("w::Other")),
            Err(refusal("Bad2 is walled"))
        );
        // anything but a LegendCompileException escapes the walk, a yes or a no, and nothing is memoized
        let raw = Err(Thrown::illegal_argument(j("upper (1) must be >= lower (2)")));
        assert_eq!(model.is_subtype(&j("w::RawChild"), &j("w::Ok")), raw);
        assert_eq!(model.is_subtype(&j("w::RawChild"), &j("w::Other")), raw);
        assert!(!model.reach_memo().contains_key(&j("w::RawChild")));
        let todo = model.is_subtype(&j("w::TodoChild"), &j("w::Todo"));
        assert_eq!(
            compile_error(&todo.unwrap_err()).kind,
            lite_error::ErrorKind::NotImplemented
        );
        assert!(!model.reach_memo().contains_key(&j("w::TodoChild")));
    }

    #[test]
    fn a_reach_is_walked_once_and_an_unasked_class_cell_stays_unset() {
        let arena = ModelArena::new();
        let model = hierarchy(&arena);
        let first = model.reach(&j("w::Child3")).unwrap();
        let again = model.reach(&j("w::Child3")).unwrap();
        assert!(same(&first, &again));
        // the walk's order: breadth first, itself included
        assert_eq!(
            first.classes.iter().map(|(c, ())| c.clone()).collect::<Vec<_>>(),
            vec![j("w::Child3"), j("w::Mid"), j("w::Bad2"), j("w::Bad")]
        );
        assert_eq!(first.failure(), Some(&refusal("Bad2 is walled")));
        // Late was never asked: its cell is unset
        assert!(!class_cell_is_set(&model, "w::Late"));
    }

    // ---- P4: findProperty, over cells set by hand ----

    /// `ElementRefusalsTest.CYCLE` (:24).
    const CYCLE: &str = "Class w::A extends w::B { a: String[1]; }\n\
        Class w::B extends w::A { b: String[1]; }\n\
        Class w::C extends w::A { c: String[1]; }\n";

    fn cycle(arena: &ModelArena) -> Model<'_> {
        let model = frozen(arena, CYCLE);
        prefill(&model, "w::A", Ok(typed_class(arena, "w::A", &["w::B"], &["a"])));
        prefill(&model, "w::B", Ok(typed_class(arena, "w::B", &["w::A"], &["b"])));
        prefill(&model, "w::C", Ok(typed_class(arena, "w::C", &["w::A"], &["c"])));
        model
    }

    fn found(model: &Model<'_>, class: &str, name: &str) -> Option<String> {
        let query = QueryArena::new();
        model
            .find_property(&query, &j(class), &j(name))
            .expect("a lookup")
            .map(|p| p.name().lossy())
    }

    #[test]
    fn find_property_on_a_walled_cyclic_class_is_refused_by_name() {
        // ElementRefusalsTest.findPropertyOnAWalledCyclicClassIsRefusedByName (:30), its lookups
        let arena = ModelArena::new();
        let model = cycle(&arena);
        assert_eq!(found(&model, "w::A", "a").as_deref(), Some("a"));
        assert_eq!(found(&model, "w::A", "b").as_deref(), Some("b"));
        assert_eq!(found(&model, "w::C", "a").as_deref(), Some("a"));
        let query = QueryArena::new();
        let e = model.find_property(&query, &j("w::A"), &j("nope")).unwrap_err();
        let e = compile_error(&e);
        assert_eq!((e.kind, e.phase), (lite_error::ErrorKind::Model, Some(Phase::Model)));
        assert_eq!(
            JStr(e.message.clone().unwrap()).lossy(),
            "Inheritance cycle: w::A -> w::B -> w::A"
        );
        assert_eq!(e.element.clone().map(|u| JStr(u).lossy()).as_deref(), Some("w::A"));
        let from_c = model.find_property(&query, &j("w::C"), &j("nope")).unwrap_err();
        let from_c = compile_error(&from_c);
        assert_eq!(
            JStr(from_c.message.clone().unwrap()).lossy(),
            "Inheritance cycle: w::C -> w::A -> w::B -> w::A"
        );
        assert_eq!(from_c.element.clone().map(|u| JStr(u).lossy()).as_deref(), Some("w::C"));
        // and the reach over the same cycle terminates with a no
        assert_eq!(model.is_subtype(&j("w::A"), &j("w::B")), Ok(true));
        assert_eq!(model.is_subtype(&j("w::A"), &j(platform::ANY)), Ok(false));
    }

    #[test]
    fn a_diamond_is_no_cycle_and_the_most_derived_declaration_wins() {
        // ElementRefusalsTest.aDiamondIsNoCycle (:121), its lookups
        let arena = ModelArena::new();
        let model = frozen(
            &arena,
            "Class w::Top { t: String[1]; }\nClass w::L extends w::Top { }\nClass w::R extends w::Top { }\n\
             Class w::D extends w::L, w::R { }\nClass w::S extends w::Top { t: String[1]; s: String[1]; }\n",
        );
        prefill(&model, "w::Top", Ok(typed_class(&arena, "w::Top", &[], &["t"])));
        prefill(&model, "w::L", Ok(typed_class(&arena, "w::L", &["w::Top"], &[])));
        prefill(&model, "w::R", Ok(typed_class(&arena, "w::R", &["w::Top"], &[])));
        prefill(&model, "w::D", Ok(typed_class(&arena, "w::D", &["w::L", "w::R"], &[])));
        prefill(
            &model,
            "w::S",
            Ok(typed_class(&arena, "w::S", &["w::Top"], &["s", "t"])),
        );
        assert_eq!(found(&model, "w::D", "nope"), None);
        assert_eq!(found(&model, "w::D", "t").as_deref(), Some("t"));
        // an unknown class is none, its ends unasked
        assert_eq!(found(&model, "absent::K", "t"), None);
        // the class's own declaration, not the supertype's
        let query = QueryArena::new();
        let own = model.find_property(&query, &j("w::S"), &j("t")).unwrap().unwrap();
        let class_s = model.find_class(&j("w::S")).unwrap().unwrap();
        assert_eq!(own, class_s.properties[1]);
    }

    // ---- P4: the function cells, set by hand ----

    /// A typed function over `definition`, as the function compiler makes one, by hand.
    fn typed_function(definition: &Function) -> TypedFunction<'static> {
        TypedFunction {
            qualified_name: definition.qualified_name().clone(),
            type_parameters: empty(),
            multiplicity_parameters: empty(),
            parameters: empty(),
            return_type: Type::Primitive(Primitive::String),
            return_multiplicity: Multiplicity::ONE,
            body: None,
            is_native: matches!(definition, Function::NativeFunctionDefinition(_)),
            id: FunctionId::of(definition),
            definition: definition.clone(),
        }
    }

    fn prefill_functions<'m>(model: &Model<'m>, fqn: &JStr, outcome: Result<Vec<TypedFunction<'m>>, Thrown>) {
        model
            .function_cells
            .get(fqn)
            .expect("a function cell")
            .set(outcome.map(|v| v.into_iter().map(Arc::new).collect()))
            .expect("an unset cell");
    }

    #[test]
    fn the_function_cell_hands_out_one_arc_per_overload_and_by_id_the_same() {
        let arena = ModelArena::new();
        let model = frozen(&arena, UNIVERSE_TEXT);
        let f = j("w::f");
        let defs = model.index().find_function(&f).to_vec();
        assert_eq!(defs.len(), 2);
        prefill_functions(&model, &f, Ok(defs.iter().map(typed_function).collect()));
        let first = model.find_function(&f).unwrap();
        let again = model.find_function(&f).unwrap();
        assert_eq!(first.len(), 2);
        assert!(first.iter().zip(again).all(|(a, b)| same(a, b)));
        // by id: the overload whose definition is equal, the cell's very Arc
        let by_id = model.find_function_by_id(&FunctionId::of(&defs[1]).qualified).unwrap();
        assert_eq!(by_id.len(), 1);
        assert!(same(&by_id[0], &first[1]));
        // a native by its id
        let native_id = FunctionId::of(&Function::NativeFunctionDefinition(catalog().all()[0].clone())).qualified;
        let native = Function::NativeFunctionDefinition(
            catalog()
                .native_function_by_id(&native_id)
                .expect("a native")
                .def()
                .clone(),
        );
        prefill_functions(&model, native.qualified_name(), Ok(vec![typed_function(&native)]));
        let natives = model.find_function_by_id(&native_id).unwrap();
        assert_eq!(natives.len(), 1);
        assert!(same(
            &natives[0],
            &model.find_function(native.qualified_name()).unwrap()[0]
        ));
        // an unknown FQN is empty and has no cell; an unknown id is empty
        assert!(model.find_function(&j("w::nope")).unwrap().is_empty());
        assert!(!model.function_cells.contains_key(&j("w::nope")));
        assert!(model.find_function_by_id(&j("w::nope__String_1_")).unwrap().is_empty());
        // a refusal is cached and returned again
        let g = j("w::g");
        let raw = Thrown::illegal_argument(j("upper (1) must be >= lower (2)"));
        prefill_functions(&model, &g, Err(raw.clone()));
        assert_eq!(model.find_function(&g).unwrap_err(), raw);
        assert_eq!(model.find_function(&g).unwrap_err(), raw);
        let g_id = FunctionId::of(&model.index().find_function(&g)[0]).qualified;
        assert_eq!(model.find_function_by_id(&g_id).unwrap_err(), raw);
    }

    // ---- P1: the checked layer ----

    #[test]
    fn the_layer_holds_its_elements_by_identity_and_its_ids() {
        let p = parsed(UNIVERSE_TEXT);
        let index = ModelIndex::from(&p).unwrap();
        let layer = layer_of(&index);
        let cd = index.find_class(&j("w::P")).unwrap();
        assert!(layer.contains_class(cd));
        // an equal copy is no member: the layer is provenance, by identity
        assert!(!layer.contains_class(&Arc::new((**cd).clone())));
        let f = &index.find_function(&j("w::g"))[0];
        assert!(layer.contains_function(f));
        assert!(layer.contains_association(index.find_association(&j("w::PQ")).unwrap()));
        assert!(layer.contains_enum(index.find_enum(&j("w::E")).unwrap()));
        let ids: HashSet<FunctionId> = index.functions().map(FunctionId::of).collect();
        assert_eq!(layer.signature_keys(), &ids);
        assert_eq!(ids.len(), 3);
        // without: the adopted ids go, the elements stay
        let adopted: HashSet<FunctionId> = [FunctionId::of(f)].into_iter().collect();
        let without = layer.without(&adopted);
        assert_eq!(without.signature_keys().len(), 2);
        assert!(!without.signature_keys().contains(&FunctionId::of(f)));
        assert!(without.contains_function(f) && without.contains_class(cd));
        assert_eq!(layer.without(&HashSet::new()).signature_keys(), &ids);
    }

    // ---- threads ----

    /// Every question the model answers without a sibling family's fill, as text.
    fn answers(model: &Model<'_>) -> Vec<String> {
        let query = QueryArena::new();
        let mut out = vec![
            format!("{:?}", model.element_fqns()),
            format!("{}", model.resolution_universe().len()),
            format!("{:?}", model.function_fqns()),
            format!("{:?}", model.classifier_instances(&j(platform::CLASS_METACLASS))),
            format!("{:?}", model.classifier_instances(&j(platform::ENUMERATION))),
            format!("{:?}", model.find_enum(&j(DATE_TIME_FORMAT))),
        ];
        for class in ["w::A", "w::B", "w::C", "absent::K"] {
            for parent in ["w::A", "w::B", "w::C", platform::ANY, platform::NIL] {
                out.push(format!(
                    "{class} {parent} {:?}",
                    model.is_subtype(&j(class), &j(parent))
                ));
            }
            for name in ["a", "b", "c", "nope"] {
                let p = model.find_property(&query, &j(class), &j(name));
                out.push(format!("{class}.{name} {p:?}"));
            }
            out.push(format!(
                "{class} {} {}",
                model.is_package(&j(class)),
                model.resolution_universe_contains(&j(class))
            ));
        }
        out
    }

    #[test]
    fn n_threads_asking_every_question_answer_as_one_thread() {
        let one_arena = ModelArena::new();
        let one = cycle(&one_arena);
        let expected = answers(&one);
        let shared_arena = ModelArena::new();
        let shared = cycle(&shared_arena);
        let got: Vec<Vec<String>> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8).map(|_| s.spawn(|| answers(&shared))).collect();
            handles.into_iter().map(|h| h.join().expect("a thread")).collect()
        });
        for g in got {
            assert_eq!(g, expected);
        }
    }

    // ---- the sibling families' fills needed: ignored until they land ----

    #[test]
    fn find_type_classifies_every_kind_and_an_unknown_fqn_is_none() {
        // PureModelContextTest.findTypeClassifiesEveryKind (:52), findTypeUnknownIsEmpty (:73)
        let arena = ModelArena::new();
        let model = frozen(&arena, &fixture_text());
        let query = QueryArena::new();
        assert!(matches!(
            model.find_type(&query, &j("model::Person")),
            Some(Type::ClassType(_))
        ));
        assert!(matches!(
            model.find_type(&query, &j("model::Color")),
            Some(Type::EnumType(_))
        ));
        assert_eq!(
            model.find_type(&query, &j(STRING)),
            Some(Type::Primitive(Primitive::String))
        );
        assert_eq!(
            model.find_type(&query, &j(INTEGER)),
            Some(Type::Primitive(Primitive::Integer))
        );
        assert!(matches!(
            model.find_type(&query, &j(DATE_TIME_FORMAT)),
            Some(Type::EnumType(_))
        ));
        assert_eq!(model.find_type(&query, &j("model::DoesNotExist")), None);
    }

    #[test]
    fn a_class_cell_is_compiled_on_first_demand_memoized_and_the_unasked_stay_unset() {
        // PureModelContextTest.findClassMaterializesProperties (:83), findClassMemoized (:100),
        // findPropertyResolvesOnClass (:115)
        let arena = ModelArena::new();
        let model = frozen(&arena, &fixture_text());
        let person = model.find_class(&j("model::Person")).unwrap().unwrap();
        assert_eq!(person.properties.len(), 3);
        assert_eq!(person.properties[0].name(), &j("name"));
        assert_eq!(person.properties[0].r#type(), Type::Primitive(Primitive::String));
        assert_eq!(person.properties[0].multiplicity(), Multiplicity::ONE);
        assert_eq!(person.properties[2].name(), &j("home"));
        assert_eq!(person.properties[2].r#type(), Type::ClassType("model::Address"));
        assert_eq!(person.properties[2].multiplicity(), Multiplicity::ZERO_ONE);
        assert!(same(&person, &model.find_class(&j("model::Person")).unwrap().unwrap()));
        // the referent is not loaded: the Address cell is unset
        assert!(!class_cell_is_set(&model, "model::Address"));
        assert_eq!(found(&model, "model::Person", "home").as_deref(), Some("home"));
        assert!(!class_cell_is_set(&model, "model::Address"));
        assert!(model.find_class(&j("model::Nope")).unwrap().is_none());
    }

    #[test]
    fn subtyping_walks_the_native_class_chains() {
        // PureModelContextTest.isSubtypeWalksNativeLattice (:126), its fixture half (the booted half is the
        // planner's: Compiler.buildModel)
        let arena = ModelArena::new();
        let model = frozen(&arena, &fixture_text());
        assert_eq!(model.is_subtype(&j(INTEGER), &j(NUMBER)), Ok(true));
    }

    #[test]
    fn an_unknown_property_type_fails_eagerly_at_the_gate() {
        // PureModelContextTest.unknownPropertyTypeThrows (:144)
        let arena = ModelArena::new();
        let e = context(&arena, "Class model::Bad { x: not::a::RealType[1]; }")
            .err()
            .expect("refused");
        let message = e.message().unwrap().lossy();
        assert!(e.is_model() && message.contains("Unknown type") && message.contains("not::a::RealType"));
    }

    #[test]
    fn the_strict_gate_builds_the_fixture() {
        let arena = ModelArena::new();
        let model = context(&arena, &fixture_text()).expect("the fixture compiles");
        assert!(model.find_class(&j("model::Person")).unwrap().is_some());
        assert_eq!(model.find_enum(&j("model::Color")).unwrap().values.len(), 2);
        assert_eq!(model.classifier_instances(&j(ASSOCIATION_METACLASS)), Some(vec![]));
    }

    #[test]
    fn a_walled_cyclic_class_of_a_tolerant_build_is_refused_by_name_at_the_lookup() {
        // ElementRefusalsTest.findPropertyOnAWalledCyclicClassIsRefusedByName (:30)
        let arena = ModelArena::new();
        let mut walls: LinkedMap<JStr, JStr> = LinkedMap::new();
        let model = tolerant(&arena, CYCLE, &mut walls).expect("a tolerant build answers");
        let mut got: Vec<(String, String)> = walls.iter().map(|(k, v)| (k.lossy(), v.lossy())).collect();
        got.sort();
        assert_eq!(
            got,
            vec![
                (
                    "w::A".to_string(),
                    "Inheritance cycle: w::A -> w::B -> w::A".to_string()
                ),
                (
                    "w::B".to_string(),
                    "Inheritance cycle: w::B -> w::A -> w::B".to_string()
                ),
                (
                    "w::C".to_string(),
                    "Inheritance cycle: w::C -> w::A -> w::B -> w::A".to_string()
                ),
            ]
        );
        assert_eq!(found(&model, "w::A", "a").as_deref(), Some("a"));
        assert_eq!(found(&model, "w::A", "b").as_deref(), Some("b"));
        assert_eq!(found(&model, "w::C", "a").as_deref(), Some("a"));
        let query = QueryArena::new();
        let e = model.find_property(&query, &j("w::A"), &j("nope")).unwrap_err();
        assert_eq!(e.message().unwrap().lossy(), "Inheritance cycle: w::A -> w::B -> w::A");
        assert_eq!(
            compile_error(&e).element.clone().map(|u| JStr(u).lossy()).as_deref(),
            Some("w::A")
        );
        let from_c = model.find_property(&query, &j("w::C"), &j("nope")).unwrap_err();
        assert_eq!(
            from_c.message().unwrap().lossy(),
            "Inheritance cycle: w::C -> w::A -> w::B -> w::A"
        );
        assert_eq!(
            compile_error(&from_c)
                .element
                .clone()
                .map(|u| JStr(u).lossy())
                .as_deref(),
            Some("w::C")
        );
    }

    /// `ElementRefusalsTest.ASSOCIATION_END` (:55).
    const ASSOCIATION_END: &str = "Class w::A { name: String[1]; }\nClass w::B { name: String[1]; }\n\
        Association w::AB { a: w::A[2..1]; b: w::B[*]; }\n";

    #[test]
    fn a_walled_association_ends_inverted_bounds_are_refused_at_the_lookup() {
        // ElementRefusalsTest.aWalledAssociationEndsInvertedBoundsAreRefusedAtTheLookup (:60)
        let arena = ModelArena::new();
        let mut walls: LinkedMap<JStr, JStr> = LinkedMap::new();
        let model = tolerant(&arena, ASSOCIATION_END, &mut walls).expect("a tolerant build answers");
        let query = QueryArena::new();
        let e = model.find_property(&query, &j("w::B"), &j("a")).unwrap_err();
        assert!(e.is_model());
        assert_eq!(
            e.message().unwrap().lossy(),
            "property 'a' of association w::AB: invalid multiplicity \u{2014} upper (1) must be >= lower (2)"
        );
        assert_eq!(
            compile_error(&e).element.clone().map(|u| JStr(u).lossy()).as_deref(),
            Some("w::AB")
        );
        assert_eq!(found(&model, "w::A", "b").as_deref(), Some("b"));
    }

    #[test]
    fn a_tolerant_builds_broken_overload_is_dropped_and_an_all_broken_set_rethrows() {
        // ElementRefusalsTest.aBrokenOverloadIsDroppedAndAnAllBrokenSetRethrows (:105)
        let arena = ModelArena::new();
        let mut walls: LinkedMap<JStr, JStr> = LinkedMap::new();
        let model = tolerant(
            &arena,
            "function w::h(x: String[1]): String[1]\n{\n  $x\n}\n\
             function w::h(x: Integer[2..1]): String[1]\n{\n  'n'\n}\n\
             function w::k(x: Integer[2..1]): String[1]\n{\n  'n'\n}\n",
            &mut walls,
        )
        .expect("a tolerant build answers");
        let mut keys: Vec<String> = walls.iter().map(|(k, _)| k.lossy()).collect();
        keys.sort();
        assert_eq!(keys, ["w::h_Integer_$2_1$__String_1_", "w::k_Integer_$2_1$__String_1_"]);
        assert_eq!(model.find_function(&j("w::h")).unwrap().len(), 1);
        // the integrity pass's refusal since 2026-11-16 (ruling 29's policy 2, the typer slice's stage 1a item 5)
        assert_eq!(
            model.find_function(&j("w::k")).unwrap_err(),
            Thrown::model(
                lite_error::Phase::Model,
                j("parameter 'x' of function w::k: invalid multiplicity \u{2014} upper (1) must be >= lower (2)"),
                Some(&j("w::k"))
            )
        );
    }

    #[test]
    fn a_diamond_of_a_strict_build_is_no_cycle() {
        // ElementRefusalsTest.aDiamondIsNoCycle (:121)
        let arena = ModelArena::new();
        let model = context(
            &arena,
            "Class w::Top { t: String[1]; }\nClass w::L extends w::Top { }\nClass w::R extends w::Top { }\n\
             Class w::D extends w::L, w::R { }\n",
        )
        .expect("a diamond compiles");
        assert_eq!(found(&model, "w::D", "nope"), None);
        assert_eq!(found(&model, "w::D", "t").as_deref(), Some("t"));
    }

    #[test]
    fn the_function_cell_hands_out_the_same_arcs_twice() {
        let arena = ModelArena::new();
        let model = frozen(&arena, UNIVERSE_TEXT);
        let f = j("w::f");
        let first = model.find_function(&f).unwrap();
        let again = model.find_function(&f).unwrap();
        assert_eq!(first.len(), 2);
        assert!(first.iter().zip(again).all(|(a, b)| same(a, b)));
        let defs = model.index().find_function(&f);
        for (i, d) in defs.iter().enumerate() {
            let by_id = model.find_function_by_id(&FunctionId::of(d).qualified).unwrap();
            assert_eq!(by_id.len(), 1);
            assert!(same(&by_id[0], &first[i]));
        }
    }

    #[test]
    fn a_checked_layer_holds_the_gates_own_elements() {
        let p = parsed(UNIVERSE_TEXT);
        let index = ModelIndex::from(&p).unwrap();
        let cd = index.find_class(&j("w::P")).unwrap().clone();
        let layer = Model::check_layer(&as_normalized(&p), index, &registrations()).expect("the layer checks");
        assert!(layer.contains_class(&cd));
        assert_eq!(layer.signature_keys().len(), 3);
    }

    #[test]
    fn n_threads_on_a_gated_model_answer_as_one_thread() {
        let questions = |model: &Model<'_>| {
            let query = QueryArena::new();
            let mut out = Vec::new();
            for class in ["model::Person", "model::Address", INTEGER, "absent::K"] {
                out.push(format!("{:?}", model.find_class(&j(class)).map(|c| c.cloned())));
                for parent in ["model::Person", "model::Address", NUMBER, platform::ANY] {
                    out.push(format!("{:?}", model.is_subtype(&j(class), &j(parent))));
                }
                for name in ["name", "home", "nope"] {
                    out.push(format!("{:?}", model.find_property(&query, &j(class), &j(name))));
                }
            }
            out.push(format!(
                "{:?}",
                model.find_function(&j("meta::pure::functions::string::toUpper"))
            ));
            out
        };
        let one_arena = ModelArena::new();
        let expected = questions(&context(&one_arena, &fixture_text()).unwrap());
        let shared_arena = ModelArena::new();
        let shared = context(&shared_arena, &fixture_text()).unwrap();
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..8).map(|_| s.spawn(|| questions(&shared))).collect();
            for h in handles {
                assert_eq!(h.join().expect("a thread"), expected);
            }
        });
    }
}
