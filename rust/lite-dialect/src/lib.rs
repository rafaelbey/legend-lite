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
//! - The MIR passes ([`rewriter`], [`passes`]) run at render entry in Java's order; in this step every pass is a named
//!   stub that hands the statement back unchanged, for the next step to fill in.
//!
//! A statement nests deep: render it on a thread with a big stack (the deepest tree of the dump needs more than 1 MB
//! in a debug build).

pub mod ansi;
pub mod build;
pub mod catalog;
pub mod duckdb;
pub mod duckdb_spellings;
pub mod float_text;
pub mod java;
pub mod passes;
pub mod raw_sql;
pub mod refusal;
pub mod renderer;
pub mod rewriter;
pub mod statement;
pub mod writer;

pub use duckdb::DuckDb;
pub use refusal::Refusal;
pub use renderer::{AnsiSqlRenderer, AnsiState, Lexicon, PlanParam};
pub use rewriter::SqlRewriter;
pub use statement::{Bind, Binding, Digits, Region, RenderedStatement, TypeHole, TypeSpelling};
pub use writer::SqlWriter;

use lite_sql::{SqlDdl, SqlDml, SqlQuery};

/// A dialect as its callers use it (Java's `SqlDialect`, the interface the JVM shim keeps): the renders and the
/// naming helpers, object-safe so a handle can hold any dialect.
pub trait SqlDialect {
    /// Java's `render(SqlQuery)`: the statement as text, refused when it binds a parameter.
    fn render(&self, query: SqlQuery) -> Result<String, Refusal>;
    /// Java's `renderInline(SqlQuery)`: `render` in single-line mode.
    fn render_inline(&self, query: SqlQuery) -> Result<String, Refusal>;
    /// Java's `renderStatement(SqlQuery)`.
    fn render_statement(&self, query: SqlQuery) -> Result<RenderedStatement, Refusal>;
    /// Java's `render(SqlDdl)`.
    fn render_ddl(&self, ddl: &SqlDdl) -> Result<String, Refusal>;
    /// Java's `render(SqlDml)`.
    fn render_dml(&self, dml: &SqlDml) -> Result<String, Refusal>;
    /// Java's `renderStatement(SqlDml)`.
    fn render_dml_statement(&self, dml: &SqlDml) -> Result<RenderedStatement, Refusal>;
    /// Java's `raw(statement, writtenFor)`: authored raw SQL in this session's language.
    fn raw(&self, statement: &str, written_for: Option<&str>) -> Result<String, Refusal>;
    /// Java's `storedName(name, declaredQuoted)`.
    fn stored_name(&self, name: &str, declared_quoted: bool) -> String;
    /// Java's `physicalName(name)`.
    fn physical_name(&self, name: &str) -> String;
    /// Java's `insertColumnName(name, declaredQuoted)`.
    fn insert_column_name(&self, name: &str, declared_quoted: bool) -> String;
}

impl<T: AnsiSqlRenderer> SqlDialect for T {
    fn render(&self, query: SqlQuery) -> Result<String, Refusal> {
        AnsiSqlRenderer::render(self, query)
    }

    fn render_inline(&self, query: SqlQuery) -> Result<String, Refusal> {
        ansi::render_inline(self, query)
    }

    fn render_statement(&self, query: SqlQuery) -> Result<RenderedStatement, Refusal> {
        AnsiSqlRenderer::render_statement(self, query)
    }

    fn render_ddl(&self, ddl: &SqlDdl) -> Result<String, Refusal> {
        AnsiSqlRenderer::render_ddl(self, ddl)
    }

    fn render_dml(&self, dml: &SqlDml) -> Result<String, Refusal> {
        AnsiSqlRenderer::render_dml(self, dml)
    }

    fn render_dml_statement(&self, dml: &SqlDml) -> Result<RenderedStatement, Refusal> {
        AnsiSqlRenderer::render_dml_statement(self, dml)
    }

    fn raw(&self, statement: &str, written_for: Option<&str>) -> Result<String, Refusal> {
        ansi::raw(self, statement, written_for)
    }

    fn stored_name(&self, name: &str, declared_quoted: bool) -> String {
        AnsiSqlRenderer::stored_name(self, name, declared_quoted)
    }

    fn physical_name(&self, name: &str) -> String {
        AnsiSqlRenderer::physical_name(self, name)
    }

    fn insert_column_name(&self, name: &str, declared_quoted: bool) -> String {
        AnsiSqlRenderer::insert_column_name(self, name, declared_quoted)
    }
}

#[cfg(test)]
mod tests;
