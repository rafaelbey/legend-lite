// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's model normalizer, ported from core's `com.legend.normalizer` (pipeline step E; the layer `normalizer`
//! of tools/deps/core-layers.txt; docs/rust-port/PHASE3.md, the normalizer slice, 2026-11-14; what to port and how:
//! docs/rust-port/EF_CENSUS.md §1, §3, §4, §9, with the owner's rulings 27-31 of PHASE3_SHIM_DESIGN.md §10).
//!
//! [`normalize`] is `ModelNormalizer.normalize(parsed, model, wallSink)`: a name-resolved `ParsedModel` (lite-names'
//! output) and its index ([`ModelIndex`], Java's `ModelBuilder`) in, a [`NormalizedModel`] out -- the structural
//! elements, then every lifted function (the mappings' realizing functions and predicates, `$prop$`,
//! `$constraint$`, `$constraintMsg$`, `$default$`, PRE/POST, `$query`, the views' `$view$`), the canonical binding
//! tables with their `NormalizationFacts`, and in a tolerant build the walls (lite-names' wall map).
//!
//! **The skeleton (2026-11-14).** One module per Java class (`MappingNormalizer` a directory, one file per family
//! group), every function with its final signature. Ported now, because every fill uses them: [`error`] (the throws
//! and the catches' predicates), [`pipeline`], [`mapping_ledger`], [`mapping_facts`], [`resolved_mapping`],
//! [`set_key_facts`], [`milestoning_facts`], [`lifted_views`] and [`mapping_closures`] (their memos), [`dyna_fn`]
//! (Java's `DynaFnArms` as one exhaustive match), and the small callees those reach. Every other body answers
//! `Err(Thrown::Unported("Class.method"))` (or, in a function Java cannot fail in, panics with `UNPORTED`) until it
//! is ported; an unported body is caught by no catch, so it always reaches the top. The map of files, Java line
//! ranges and re-expressions is port-scratch's `normalizer/skel/MAP.md`.
//!
//! [`ModelIndex`]: lite_names::ModelIndex
//! [`NormalizedModel`]: lite_model::NormalizedModel

#![forbid(unsafe_code)]
// skeleton: remove when every body is ported (a stub neither reads its parameters nor pushes onto a `&mut Vec`)
#![allow(unused_variables, dead_code, unused_imports, clippy::ptr_arg)]
// Java's signatures, argument for argument (a fill keeps them; a bundle struct would be a second shape to learn)
#![allow(clippy::too_many_arguments)]

pub mod aggregate_view_lift;
pub mod association_synthesis;
pub mod declared_coercions;
pub mod dyna_fn;
pub mod error;
pub mod group_by_synthesis;
pub mod implicit_inheritance;
pub mod jdk;
pub mod join_chain_emission;
pub mod lifted_views;
pub mod m2m_route_guards;
pub mod mapping_closures;
pub mod mapping_facts;
pub mod mapping_ledger;
pub mod mapping_normalizer;
pub mod mapping_pre_pass;
pub mod mapping_validation;
pub mod milestoning_facts;
pub mod model_join_nesting;
pub mod model_normalizer;
pub mod pipeline;
pub mod rel_op_translator;
pub mod relation_reads;
pub mod resolved_mapping;
pub mod set_key_facts;
pub mod store_substitution_rewrite;
#[cfg(test)]
mod test_support;
pub mod union_synthesis;
pub mod view_relation;
pub mod xstore_pure_ends;

pub use error::Thrown;
pub use model_normalizer::normalize;
