// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's platform declarations and the text's door, ported from core's `com.legend.builtin` and
//! `com.legend.door` (the layers `builtin` and `door` of tools/deps/core-layers.txt, merged: docs/rust-port/PHASE3_CRATES.md
//! §1; docs/rust-port/PHASE3.md, slice 1b, 2026-11-12).
//!
//! - [`door`]: `ModelFromText`, a text as the compiler's model (rust/lite-front's parser, then lite-model's door).
//! - [`canonical`]: the model differential's canonical text of the door's answer.
//! - The generated tables, written by the generators that write the Java beside them from the same pinned sources
//!   (spec/BUILD.bazel, "THE GENERATORS"): [`natives`] (`Pure.java`'s native catalog and overload groups) and
//!   [`dyna_fn`] (`DynaFn.java`'s engine dynafunction registry).
//! - The hand-written declarations over them: [`pure`] (the catalog parsed, its indexes, the lite vocabulary),
//!   [`dyna_fn_decisions`] (the platform's decision on each dynafunction), [`engine_handlers`] (the engine's bare-name
//!   surface joined to the platform's declarations), [`prelude`] (the generated prelude module), [`native_fn`] (the
//!   native function families), [`system_metamodel`] (the system metamodel store), [`subsumed`] (`Subsumed` and
//!   `TdsLegacy`).
//!
//! Every table is held to Java's by `tests/builtin_tables.rs` over rust/oracles/builtin/builtin-tables.tsv. Not
//! ported: `DecisionProbe`, the untangle's measurement probe (a `ServiceLoader` hook, a no-op unless `LL_SHADOW` is set,
//! "deleted at step 4").

#![forbid(unsafe_code)]

pub mod canonical;
pub mod door;
#[rustfmt::skip] // generated, and held byte for byte to its generator: //rust/lite-builtin:update_generated_test
pub mod dyna_fn;
#[rustfmt::skip] // generated, and held byte for byte to its generator: //rust/lite-builtin:update_generated_test
pub mod natives;
pub mod dyna_fn_decisions;
pub mod engine_handlers;
pub mod native_fn;
pub mod prelude;
pub mod pure;
pub mod subsumed;
pub mod system_metamodel;
