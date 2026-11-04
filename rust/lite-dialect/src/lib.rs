// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's SQL dialects, ported from `core/src/main/java/com/legend/sql/dialect` (docs/rust-port/PHASE1.md,
//! steps 3 to 6): a `lite_sql` statement rendered to SQL exactly as the JVM renders it, held to Java's own answers by
//! the golden oracles (`tests/dialect_differential.rs`, `PARITY.md`).
//!
//! The shape (docs/rust-port/TRIAGE.md D3):
//!
//! - [`AnsiSqlRenderer`] is Java's abstract base as a trait: each protected hook a method whose default is the base's
//!   body, written once as a free function of [`ansi`], so an override's `super.x(...)` is `ansi::x(self, ...)`; the
//!   base's `final` helpers are `ansi`'s free functions alone. [`DuckDb`], [`H2`] (H2Modern is its `modern`
//!   profile), [`EngineStyle`] (legend-engine's own text: `EngineStyleH2`, `EngineStyleDB2` and
//!   `EngineStyleComposite` are its profiles), [`Postgres`], [`Sqlite`] (Java's `SQLite`), [`Trino`], [`Databricks`],
//!   [`ClickHouse`] and [`BigQuery`] are structs implementing it.
//! - [`SqlWriter`] is Java's one place SQL is written: text in order, each bound parameter recorded where its
//!   placeholder lands, the per-key regions; [`RenderedStatement`] is what it hands back, every record's `Display`
//!   Java's record `toString`.
//! - Every Java `throw` is a [`Refusal`]: the class Java threw and its exact message.
//! - The MIR passes ([`rewriter`], [`passes`], [`carrier`], [`scan_order`], [`h2_passes`], [`shared_passes`],
//!   [`explosion_markers`], [`common_subexpressions`]) run at render entry in
//!   Java's order, each its Java class ported; a node they build carries the type fact Java's constructor computes
//!   ([`build`] over [`typing`], Java's `SqlTyping`).
//! - [`SqlDialect`] is Java's interface: every method, its defaults Java's, so [`DuckDb`] overrides what `DuckDb.java`
//!   overrides -- the catalog decisions ([`catalog_model`]), the host operations ([`host_ops`]), the session facts --
//!   [`H2`] what `H2.java` and `H2Modern.java` override, and [`EngineStyle`] what the engine-style three override,
//!   with their [`dialect::EngineTextDialect`] surface (Java's `EngineTextDialect`).
//! - [`dispatch`] is the seam the JVM calls through (`lite-ffi`'s `lite_dialect`): one op of a dialect, JSON in and
//!   JSON out, every op and form written down in `SEAM.md`.
//!
//! A statement nests deep: render it on a thread with a big stack (the deepest tree of the oracles needs more than 2 MB
//! in a debug build).

pub mod ansi;
pub mod bigquery;
pub mod build;
pub mod carrier;
pub mod catalog;
pub mod catalog_model;
pub mod clickhouse;
pub mod common_subexpressions;
pub mod databricks;
pub mod decode_shapes;
pub mod dialect;
pub mod duckdb;
pub mod duckdb_spellings;
pub mod engine_style;
pub mod explosion_markers;
pub mod float_text;
pub mod h2;
pub mod h2_passes;
pub mod host_ops;
pub mod java;
pub mod java_time;
pub mod json;
pub mod passes;
pub mod postgres;
pub mod raw_sql;
pub mod refusal;
pub mod renderer;
pub mod rewriter;
pub mod scan_order;
pub mod seam;
pub mod shared_passes;
pub mod sqlite;
pub mod statement;
pub mod trino;
pub mod typing;
pub mod wire_names;
pub mod writer;

pub use bigquery::BigQuery;
pub use clickhouse::ClickHouse;
pub use databricks::Databricks;
pub use dialect::{EngineTextDialect, KeysSnapshot, SqlDialect};
pub use duckdb::DuckDb;
pub use engine_style::EngineStyle;
pub use h2::H2;
pub use postgres::Postgres;
pub use refusal::Refusal;
pub use sqlite::Sqlite;
pub use trino::Trino;
pub use renderer::{AnsiSqlRenderer, AnsiState, Lexicon, PlanParam};
pub use rewriter::SqlRewriter;
pub use seam::dispatch;
pub use statement::{Bind, Binding, Digits, Region, RenderedStatement, TypeHole, TypeSpelling};
pub use writer::SqlWriter;

#[cfg(test)]
mod pass_tests;
#[cfg(test)]
mod tests;
