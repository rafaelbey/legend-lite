// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's model, ported from core's `com.legend.model` (the layer `model` of tools/deps/core-layers.txt:
//! docs/rust-port/PHASE3_CRATES.md; docs/rust-port/PHASE3.md, slice 1b, 2026-11-12).
//!
//! - [`records`]: the model's records, generated from the Java records (`//core:model_records_rs`, FrontCodegen's
//!   model mode) with the lossless codec's writer, so that the model differential's canonical text is written from one
//!   schema on both sides. The model tier is `Arc` (PHASE3_DESIGN.md §6 ruling 1): a record held as `Arc<R>`, a list
//!   `Arc<[T]>`, a map `Arc<[(K, V)]>` in Java's iteration order ([`linked`]). No model record writes its own
//!   `equals` in Java (searched 2026-11-12), so every one derives its equality, as a record's is.
//! - The converters from the parser's protocol records (rust/lite-front's) to the model, the one door a model given as
//!   records and one parsed from text both go through: [`model_from_protocol`] (`ModelFromProtocol`), with
//!   [`from_protocol`] (`FromProtocol`), [`mapping_from_protocol`], [`rel_op_from_protocol`] and
//!   [`mongo_declarations`]. Each throws what Java throws ([`error`]), by class and message.
//! - The model's own rules the converters and the platform call: an element's identity ([`elements`],
//!   [`function_id`]: `FunctionId` and `SignatureMangle`), `ParsedModel`'s keys and positions and `ImportScope`
//!   ([`parsed_model`]), relational identifiers, join types, set ids ([`names`]).
//!
//! - The model's reads the names layer calls (the names slice, 2026-11-12): `RelationalOperation`'s children, walk,
//!   `navigatesJoin` and `tables` ([`relational_operation`]), `ClassMapping.Pure.PropertyBinding.withExpression`; and
//!   subtyping's one walk over class declarations, in the shapes of Java's two relations ([`subtyping`]).
//! - The model's methods and convenience constructors the normalizer calls (the normalizer slice's step 3,
//!   2026-11-14, docs/rust-port/EF_CENSUS.md §8): `RelationalOperation`'s `withChildren`/`mapChildren`
//!   ([`relational_operation`]); `ClassMapping`'s `aggregationAwareMain`, `RelationFunction.Col`'s `isEmbedded` and
//!   `bindSrc`, and the short forms of `RelationFunction`, `Col` and `PropertyMapping` ([`class_mapping`]);
//!   `FunctionDefinition`'s `isSynthesized`, `withSynthesizedFrom` and short forms, `Synthesized`'s
//!   ([`function_definition`]); `MappingDefinition`'s constructors, the `NONE`s, `LegacyMappingDefinition`'s
//!   `withClassMappings` and `KeyThread`'s short form ([`mapping_definition`]).
//!
//! What `com.legend.model` holds that is not here yet, for the slice that first calls it: the lookups on
//! `MappingDefinition`'s bindings (`ClassBindings`, `AssociationBindings`: the resolver's; `position` re-expressed as a
//! binding's index) and its facts' keyed reads, `ClassMapping`'s witness-column names, the `RuntimeDefinition`
//! helpers, and the identity-keyed lookups of `DatabaseDefinition` beyond its default-schema tables and views.

#![forbid(unsafe_code)]

pub mod class_mapping;
pub mod elements;
pub mod error;
pub mod from_protocol;
pub mod function_definition;
pub mod function_id;
pub mod linked;
pub mod mapping_definition;
pub mod mapping_from_protocol;
pub mod model_from_protocol;
pub mod mongo_declarations;
pub mod names;
pub mod parsed_model;
#[rustfmt::skip] // generated, and held byte for byte to its generator: //rust/lite-model:update_generated_test
pub mod records;
mod database;
pub mod rel_op_from_protocol;
pub mod relational_operation;
pub mod subtyping;
pub mod text;

pub use error::{ModelError, Thrown, Unrepresentable};
pub use records::*;

/// The lossless codec's writer the records implement.
pub use lite_front::codec;
/// The Java values the model's records hold (rust/lite-front's), for the crates above the model that do not reach the
/// protocol layer (the platform: tools/deps/core-layers.txt).
pub use lite_front::jtypes::{BigDecimal, JStr, JValue};
/// The protocol records the model's records hold (rust/lite-front's), likewise.
pub use lite_front::records as protocol;
