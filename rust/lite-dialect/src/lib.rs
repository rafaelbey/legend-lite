// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's SQL dialects, ported from `core/src/main/java/com/legend/sql/dialect` (docs/rust-port/PHASE1.md,
//! step 3): a `lite_sql` statement rendered to SQL exactly as the JVM renders it, held to Java's own answers by the
//! golden dump (`tests/duckdb_differential.rs`, `PARITY.md`).
//!
//! The shape (docs/rust-port/TRIAGE.md D3):
//!
//! - [`AnsiSqlRenderer`] is Java's abstract base as a trait: each protected hook a method whose default is the base's
//!   body, written once as a free function of [`ansi`], so an override's `super.x(...)` is `ansi::x(self, ...)`; the
//!   base's `final` helpers are `ansi`'s free functions alone. [`DuckDb`] is a struct implementing it.
//! - [`SqlWriter`] is Java's one place SQL is written: text in order, each bound parameter recorded where its
//!   placeholder lands, the per-key regions; [`RenderedStatement`] is what it hands back, every record's `Display`
//!   Java's record `toString`.
//! - Every Java `throw` is a [`Refusal`]: the class Java threw and its exact message.
//! - The MIR passes ([`rewriter`], [`passes`], [`carrier`], [`scan_order`]) run at render entry in Java's order, each
//!   its Java class ported; a node they build carries the type fact Java's constructor computes ([`build`] over
//!   [`typing`], Java's `SqlTyping`).
//! - [`SqlDialect`] is Java's interface: every method, its defaults Java's, so [`DuckDb`] overrides what `DuckDb.java`
//!   overrides -- the catalog decisions ([`catalog_model`]), the host operations ([`host_ops`]), the session facts.
//! - [`dispatch`] is the seam the JVM calls through (`lite-ffi`'s `lite_dialect`): one op of a dialect, JSON in and
//!   JSON out, every op and form written down in `SEAM.md`.
//!
//! A statement nests deep: render it on a thread with a big stack (the deepest tree of the dump needs more than 1 MB
//! in a debug build).

pub mod ansi;
pub mod build;
pub mod carrier;
pub mod catalog;
pub mod catalog_model;
pub mod dialect;
pub mod duckdb;
pub mod duckdb_spellings;
pub mod float_text;
pub mod host_ops;
pub mod java;
pub mod json;
pub mod passes;
pub mod raw_sql;
pub mod refusal;
pub mod renderer;
pub mod rewriter;
pub mod scan_order;
pub mod seam;
pub mod statement;
pub mod typing;
pub mod wire_names;
pub mod writer;

pub use dialect::{KeysSnapshot, SqlDialect};
pub use duckdb::DuckDb;
pub use refusal::Refusal;
pub use renderer::{AnsiSqlRenderer, AnsiState, Lexicon, PlanParam};
pub use rewriter::SqlRewriter;
pub use seam::dispatch;
pub use statement::{Bind, Binding, Digits, Region, RenderedStatement, TypeHole, TypeSpelling};
pub use writer::SqlWriter;

#[cfg(test)]
mod pass_tests;
#[cfg(test)]
mod tests;
