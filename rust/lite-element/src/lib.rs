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
//! **The port** (2026-11-15): a skeleton first (every module, type and cross-file signature; [`typed`], the trivial
//! delegations, the cells' plumbing and the overlay written in full), then five families filled in parallel on
//! disjoint files, each line for line against 18feff3fe -- the type kernel ([`type_classifier`], [`store_compiler`],
//! [`equality_keys`]), the element compilers ([`function_compiler`], [`class_compiler`]), the integrity pass
//! ([`model_integrity`]), GraphEdges' rule ([`graph_edges`]) and the model ([`model`]). Held byte for byte to the live
//! Java and its frozen answers by `tests/element_differential.rs` (rust/oracles/element: 74,418 of 74,418 asks).

#![forbid(unsafe_code)]

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
