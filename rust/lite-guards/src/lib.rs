// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! No library: this crate is its tests (`tests/guards.rs`, docs/rust-port/PHASE3_CRATES.md §3), which hold the
//! Rust port's crate graph to `rust/crate-layers.txt` and that file to tools/deps/core-layers.txt, and phase 3's
//! sources to the arena ruling's two rules (its one `unsafe` module; pointer addresses in `identity` alone).

#![forbid(unsafe_code)]
