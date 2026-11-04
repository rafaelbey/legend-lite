// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `SqlWriter.Piece` as the dialects past DuckDB and the H2 family compose it (Postgres, SQLite, Trino,
//! Databricks, ClickHouse, BigQuery, Snowflake): [`P`], a shared closure that writes itself, and the three ways a
//! helper makes one -- [`pc`] from a closure, [`text`] from fixed text, and [`Ex::ex`] from an expression written at a
//! precedence. A helper composes pieces exactly as Java's lambdas do: what Java evaluates when a piece is made (a
//! type's name, a fresh name, a string literal) the helper evaluates then too, and what Java evaluates when a piece is
//! written (an expression, a nested piece) the closure evaluates when it runs.

use crate::refusal::Refusal;
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::SqlExpr;
use std::rc::Rc;

/// Java's `SqlWriter.Piece`: SQL that writes itself, shareable among the helpers that write it.
pub type P<'a> = Rc<dyn Fn(&mut SqlWriter) -> Result<(), Refusal> + 'a>;

/// A piece from a closure (Java's `w -> ...`).
pub fn pc<'a>(f: impl Fn(&mut SqlWriter) -> Result<(), Refusal> + 'a) -> P<'a> {
    Rc::new(f)
}

/// A piece of fixed text.
pub fn text<'a>(s: impl Into<String>) -> P<'a> {
    let s = s.into();
    pc(move |w| {
        w.append(&s);
        Ok(())
    })
}

/// An expression as a piece, for every dialect (Java's `w -> w.expr(e, prec)`).
pub trait Ex: AnsiSqlRenderer {
    /// `e`, written at `prec`, as a piece.
    fn ex<'a>(&'a self, e: &'a SqlExpr, prec: i32) -> P<'a> {
        pc(move |w| {
            w.expr(self, e, prec)?;
            Ok(())
        })
    }
}

impl<D: AnsiSqlRenderer> Ex for D {}
