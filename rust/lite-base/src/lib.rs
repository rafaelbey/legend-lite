// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The foundation every crate of phase 3 stands on (docs/rust-port/PHASE3_CRATES.md): the layer `base` of
//! tools/deps/core-layers.txt, which every layer of the file lists, so that every crate may use it.
//!
//! - [`arena`]: the per-query bump arena with its keep pool, and the model's arena, shared between threads
//!   (docs/rust-port/PHASE3_DESIGN.md §1.6, the arena ruling; PHASE3_SHIM_DESIGN.md §7.3 c, §10 rulings 9 and 10).
//!   The crate's one module that may hold `unsafe` (with lite-ffi's handle, the port's only), each block with its
//!   `SAFETY:` comment; every other crate of phase 3 forbids it (`lite-guards` holds both).
//! - [`identity`]: Java's `==` for the arena's records and the `Arc` tiers, the maps and sets keyed by it, and
//!   `Origin` (PHASE3_DESIGN.md §1.4 item 2, as §1.6 changed it): the one module that turns a pointer into an address.

#![deny(unsafe_code)]
#![deny(clippy::undocumented_unsafe_blocks)]

#[allow(unsafe_code)]
pub mod arena;
pub mod identity;

pub use arena::{Alloc, ModelArena, QueryArena};
pub use identity::{Addr, IdMap, IdSet, Identity, Origin, same};
