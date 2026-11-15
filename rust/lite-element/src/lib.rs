// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's element compiler, ported from core's `com.legend.compiler.element` (pipeline step F; the second part
//! of the compiler's split, `compiler/element` of rust/lite-guards/crate-layers.txt: docs/rust-port/PHASE3_CRATES.md
//! §2; docs/rust-port/PHASE3.md, the element slice, 2026-11-15; what to port and how: docs/rust-port/EF_CENSUS.md §2,
//! §3, §4, §9, with the owner's rulings 27-31 of PHASE3_SHIM_DESIGN.md §10).
//!
//! **What F is.** [`Model::from`] is `PureModelContext.from`: E's output (`NormalizedModel`, lite-normalizer's) and
//! the index E read (lite-names' [`ModelIndex`](lite_names::ModelIndex), Java's `ModelBuilder`) in, the concrete model
//! out -- after THE eager integrity pass ([`model_integrity`]: every reference every element makes, checked once,
//! whole-model, its first failure `compileErrors`' answer, or in a tolerant build every failing element's wall) and
//! the freeze of its Knowledge. Every later layer reads the model through [`Model`]'s methods (Java's
//! `ModelContext`): the kinds of names, the typed elements compiled on demand and memoized ([`typed`]: `TypedClass`,
//! `Property`, `TypedConstraint`, `TypedEnum`, `TypedParameter`, `TypedFunction`), subtyping over compiled classes,
//! the platform's declaration and implementation tables, E's stamped mapping facts, the store lookups. F never
//! triggers G: this crate reaches no spec crate (a compile error, not a convention: core/README.md invariant 7).
//!
//! **Where Java's classes went** (each function's doc line names its Java method and line, at 18feff3fe, the
//! slice's last Java change; the census cites 7c438f7b3's lines):
//! - `PureModelContext` and `ModelContext` -> [`model`] ([`Model`], [`CheckedLayer`], [`Walked`], [`Overlay`]).
//! - `ModelIntegrity` -> [`model_integrity`]; `GraphEdges` (GE1, GE2) -> [`graph_edges`].
//! - `TypeClassifier` -> [`type_classifier`]; `FunctionCompiler` -> [`function_compiler`]; `ClassCompiler` ->
//!   [`class_compiler`] (with the one shared `realized_fqn`, MI's too).
//! - `StoreCompiler.tableSchema` -> [`store_compiler`]; `EqualityKeys.isKeyStereotype` -> [`equality_keys`].
//! - `TypedClass`, `Property`, `TypedConstraint`, `TypedEnum`, `TypedParameter`, `TypedFunction` -> [`typed`].
//! - Elsewhere already: `type/` is lite-types (with `Multiplicity.from` and `PlatformTypes.eraseTdsRow` since this
//!   slice's step 3); `RelationalTypeInference`, `ViewSignatures`, `StoreCompiler.columnType` and
//!   `MilestoningStrategy` are lite-names' (the owner's ruling of 2026-11-12); `ModelIntegrity.refuseDuplicates` is
//!   `ModelIndex::refuse_duplicates`; `NormalizationFacts`' keyed lookups are lite-model's (`element_methods.rs`).
//! - With their first caller instead, not here (census §3, ruling 27): `ClassLayouts` (I), `RelationalOpRows` (H or
//!   the planner), `Temporal` (G, H, lineage, the planner), `EqualityKeys` beyond `isKeyStereotype` (I, the planner,
//!   testplan), `StoreCompiler.declaredType` (G), `ModelContext.isDeclaredSubtype` (G½: a distinct relation, ASK 16).
//!
//! **DON'T PORT** (census §3): PMC's public constructors (:61, :68) and its wall-sink-only `from` (:169); MI's
//! 3-argument `check` (:38); FC's `DecisionProbe` hook (:58-60); GE3, the Cypher view (the Cypher wave);
//! `ModelContext`'s 25 overridden default bodies; `TypedNominal`, `TypedElement`; `TypedFunction`'s 8- and 9-argument
//! constructors and its null `definition`/`id`; `TypedConstraint.externalId` and `TypedConstraint.of`.
//!
//! **RE-EXPRESS** (census §4): `ModelContext` (one implementation) is [`Model`]'s inherent methods; PMC's `derived`
//! is named fields (only `Reach` is F's); `packages`, `declarations`, `implementations` are built at freeze;
//! `resolutionUniverse` is a `contains` predicate plus an ordered list (its order declared: [`model`]'s docs);
//! `Walked`'s rethrown exception is the class cell's cached refusal; TC:47 reads `lite_types::platform::ENUM`, TC:111
//! classifies the name directly; MI:444 (the census's :434) is a direct bounds test with the same text, MI:513 (:486)
//! a match, MI:580 (:553) the literal names `Other` and `Distinct`; `TypedConstraint` has no `externalId`,
//! `TypedFunction` a non-optional `definition` and `id`.
//!
//! **The design** (the lead's, from the rulings; [`model`]'s docs have each in full):
//! - The model BORROWS its arena (`&'m ModelArena`: no `unsafe` here, so it cannot own what its `Type<'m>`s point
//!   into; the owning handle is the FFI's) and OWNS its index. It is `Send + Sync`, checked at compile time.
//! - The classifier and the function compiler are views made per call (the model owns the index they borrow).
//! - The element memos are `OnceLock` cells per FQN known at freeze, a refusal cached; an unknown FQN answers empty
//!   with no cell; one `Arc` per overload. `Reach` is a `Mutex<HashMap>` computed outside its lock.
//! - The boot layer is a [`CheckedLayer`]: the boot's `Arc`s by identity, one `IdSet` per kind, and its ids; the
//!   graph's index holds the boot's very `Arc`s (`ModelIndex::add` clones `Arc`s, never records).
//! - The overlay (ruling 11) is a view, [`Overlay`], that derefs to the model.
//! - A method that makes a `Type` takes the arena it allocates in: the classifier's, [`Model::find_type`],
//!   [`Model::find_property`] (an association end's type, classified at each lookup as Java does, goes in the
//!   caller's arena, not the model's), [`Model::find_table`].
//! - FC:77's swallow is a typed `Result` (census ASK 7 (a), ruling 30): an `Err` drops the overload, all `Err` the
//!   first, a panic propagates.
//! - The error type is lite-names' [`Thrown`]: a `com.legend.error` class (`Compile`), or a raw Java exception Java
//!   lets escape (`Java { class, message }`: a bound's `IllegalArgumentException`, an `IllegalStateException`), so
//!   the differential writes it as Java does.
//!
//! **The skeleton** (2026-11-15): every module, type and cross-file signature in place; a body not yet ported is
//! `unimplemented!("lite-element skeleton: <Java method>")`, each file self-contained for its fill. Written in full:
//! [`typed`], the trivial delegations, the element cells' plumbing and the overlay's three overrides.

#![forbid(unsafe_code)]
// skeleton: remove when every body is ported (a stub neither reads its parameters nor reaches its private helpers)
#![allow(unused_variables, dead_code, unused_imports)]

pub mod class_compiler;
pub mod equality_keys;
pub mod function_compiler;
pub mod graph_edges;
pub mod model;
pub mod model_integrity;
pub mod store_compiler;
pub mod type_classifier;
pub mod typed;

pub use function_compiler::FunctionCompiler;
pub use lite_names::Thrown;
pub use model::{CheckedLayer, Model, Overlay, Walked};
pub use type_classifier::TypeClassifier;
pub use typed::{
    EnforcementLevel, Property, PropertyDerived, PropertyStored, TypedClass, TypedConstraint, TypedEnum, TypedFunction,
    TypedParameter,
};

/// The cross-file calls the fill will make, as its callers will write them, so that the compiler holds the fixed
/// signatures to composing on day one (their bodies are stubs: nothing here runs). Deleted once every call below has
/// its real site.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
mod seams {
    use std::collections::HashSet;
    use std::sync::Arc;

    use lite_base::{ModelArena, QueryArena};
    use lite_front::records as p;
    use lite_model::linked::LinkedMap;
    use lite_model::{
        AssociationDefinition, ClassDefinition, ConnectionDefinition, DatabaseDefinitionTableDefinition, FunctionId,
        JStr, NormalizedModel, RuntimeDefinition,
    };
    use lite_names::ModelIndex;
    use lite_platform::Registrations;

    use crate::function_compiler::FunctionCompiler;
    use crate::type_classifier::TypeClassifier;
    use crate::{Model, Thrown, class_compiler, equality_keys, graph_edges, model_integrity, store_compiler};

    /// model_integrity::check's calls into the classifier, the function compiler, the class compiler's shared rule
    /// and the graph-edge rule.
    fn integrity(
        model: &ModelIndex,
        functions: &FunctionCompiler<'_>,
        cd: &ClassDefinition,
        dp: &p::DerivedPropertyDefinition,
        association: &AssociationDefinition,
        site: &JStr,
        walls: &mut LinkedMap<JStr, JStr>,
    ) -> Result<(), Thrown> {
        let scratch = QueryArena::new();
        let classifier: TypeClassifier<'_> = functions.classifier();
        classifier.classify(&scratch, &dp.r#type, &cd.type_params)?;
        classifier.require_arguments(&dp.r#type, &cd.type_params)?;
        let head = TypeClassifier::head_fqn(&cd.super_classes[0])?;
        let _ = classifier.find_type(&scratch, head);
        let _ = classifier.class_def(head).is_some() || classifier.is_class_fqn(head);
        model_integrity::require_valid_bounds(&dp.multiplicity, site)?;
        let end_site = model_integrity::association_end_site(&association.property1, association);
        model_integrity::require_valid_bounds(&association.property1.multiplicity, &end_site)?;
        let fqn = class_compiler::realized_fqn(
            &dp.realization,
            lite_names::synth_fqn::prop(&cd.qualified_name, &dp.name)?,
        );
        functions.require_function(&fqn, site)?;
        functions.require_shape(
            &fqn,
            FunctionCompiler::returns_boolean_one,
            site,
            "returning Boolean[1]",
        )?;
        functions.require_shape(
            &fqn,
            |f| f.parameters().is_empty() && functions.returns_class_many(f),
            site,
            "of the form (): Class[*]",
        )?;
        let source = graph_edges::source_of(model);
        let index = graph_edges::Index::of(&source);
        graph_edges::read_one(&source, &index, cd)?;
        model_integrity::check(model, &classifier, functions, Some(walls), None)?;
        model_integrity::check(model, &classifier, functions, None, None)
    }

    /// The class compiler's calls: the classifier in the model's arena, the key stereotype.
    fn class_compile<'m>(
        classifier: &TypeClassifier<'_>,
        arena: &'m ModelArena,
        cd: &ClassDefinition,
        pd: &lite_model::ClassDefinitionPropertyDefinition,
    ) -> Result<(), Thrown> {
        let _ = pd.stereotypes.iter().any(|st| equality_keys::is_key_stereotype(st));
        let t = classifier.classify(arena, &pd.r#type, &cd.type_params)?;
        let m = TypeClassifier::multiplicity(arena, &pd.multiplicity)?;
        let typed: crate::TypedClass<'m> = class_compiler::compile(classifier, arena, cd)?;
        let _ = (t, m, typed);
        Ok(())
    }

    /// The model's: the gate, the boot layer, the overlay through its deref, the lookups that take an arena.
    fn model_calls<'m>(
        arena: &'m ModelArena,
        normalized: &NormalizedModel,
        boot_index: ModelIndex,
        index: ModelIndex,
        registrations: &Registrations,
        runtime: Arc<RuntimeDefinition>,
        connection: Arc<ConnectionDefinition>,
        table: &DatabaseDefinitionTableDefinition,
        fqn: &JStr,
    ) -> Result<(), Thrown> {
        let boot = Model::check_layer(normalized, boot_index, registrations)?;
        let adopted: HashSet<FunctionId> = HashSet::new();
        let prior = boot.without(&adopted);
        let mut walls: LinkedMap<JStr, JStr> = LinkedMap::new();
        let model: Model<'m> = Model::from(arena, normalized, index, Some(&mut walls), Some(&prior), registrations)?;
        model_integrity::check(
            model.index(),
            &model.classifier(),
            &model.function_compiler(),
            None,
            Some(&prior),
        )?;
        let overlay = model.with_execution_overlay(runtime, connection)?;
        let _ = overlay.find_class(fqn)?;
        let _ = overlay.find_runtime(fqn).is_some() && overlay.is_execution_context_element(fqn);
        let _: &Model<'m> = &overlay;
        let query = QueryArena::new();
        let _ = model.find_property(&query, fqn, fqn)?;
        let _ = model.find_type(&query, fqn);
        let _ = model.find_table(&query, fqn, fqn)?;
        let _ = store_compiler::table_schema(&query, table)?;
        let _ = model.is_subtype(fqn, fqn)?;
        let _: &[Arc<crate::TypedFunction<'m>>] = model.find_function(fqn)?;
        let _ = model.find_function_by_id(fqn)?;
        let _ = model.find_enum(fqn);
        let _ = model.classifier_instances(fqn);
        let _ = model_integrity::require_valid_bounds_at(
            &p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
                lower_bound: 1,
                upper_bound: Some(1),
            })),
            fqn,
            fqn,
        );
        Ok(())
    }
}
