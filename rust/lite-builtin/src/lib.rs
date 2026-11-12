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

#![forbid(unsafe_code)]

pub mod canonical;
pub mod door;
#[rustfmt::skip] // generated, and held byte for byte to its generator: //rust/lite-builtin:update_generated_test
pub mod dyna_fn;
#[rustfmt::skip] // generated, and held byte for byte to its generator: //rust/lite-builtin:update_generated_test
pub mod natives;
