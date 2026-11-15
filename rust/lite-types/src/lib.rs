// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's type family, ported from core's `compiler_element_type` layer
//! (core/src/main/java/com/legend/compiler/element/type/: `Type`, `PlatformTypes`, `Multiplicity`, `ExprType`;
//! docs/rust-port/PHASE3.md, slice 1a, 2026-11-11).
//!
//! [`Type<'a>`](Type) is one `Copy` family with a lifetime (PHASE3_SHIM_DESIGN.md §7.3 c, the owner's ruling 9): its
//! records live in an arena -- the model's [`lite_base::ModelArena`] for a model's types, a query's
//! [`lite_base::QueryArena`] for a query's -- and since the family is covariant, a model's types are a query's with
//! no conversion. Every operation is Java's, held to Java's answers by `tests/type_differential.rs` over
//! `rust/oracles/types/type-facts.tsv`, frozen from the Java before it is deleted. A refusal is Java's exception and
//! message ([`TypeError`]).
//!
//! The two operations F needs that read a later layer (2026-11-15, the element slice, step 3; docs/rust-port/
//! PHASE3_CRATES.md §4, bucket (b)): [`Multiplicity::from`] (the parser's protocol records, lite-front) and
//! [`platform::erase_tds_row`] (`builtin`'s `NativeFn.RowGetter`, lite-builtin), both on the layer's own line. What the
//! layer holds that this crate does not yet, for the slice that first calls it: `handleRowClass` and `isStatementOnly`
//! (`model`'s `FunctionId`, `builtin`'s `NativeFn`). Subtyping is not a type's operation, in Java or here: the lattice
//! is the classes' `extends` chain, which the model walks (`ModelContext.isSubtype`).

#![forbid(unsafe_code)]

mod decimal;
mod error;
mod multiplicity;
pub mod platform;
mod primitive;
mod ty;

pub use decimal::PrecisionDecimal;
pub use error::{Thrown, TypeError};
pub use multiplicity::{Bounded, Multiplicity, Var};
pub use primitive::{Family, PRECISE_PRIMITIVES, Primitive, TypeVariable};
pub use ty::{
    Column, ExprType, FunctionType, GenericType, LATE_BOUND_WILDCARD, Op, PIVOT_SEPARATOR, Param, RelationType,
    SchemaAlgebra, Type,
};

#[cfg(test)]
mod tests;
