// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `SqlWriter`, THE ONE PLACE A DIALECT WRITES SQL: text in order, each bound parameter recorded where its
//! placeholder is written, and the per-key regions. Java's writer holds its dialect's `expr` as a callback; here the
//! dialect is passed to the writer's expression methods (`w.expr(d, e, 0)?` is Java's `writer.expr(e, 0)`), so a
//! sub-expression is still written in place, in the dialect's spelling, its parameters bound where they land.

use crate::refusal::Refusal;
use crate::renderer::AnsiSqlRenderer;
use crate::statement::{Bind, Binding, Region, RenderedStatement, TypeHole, TypeSpelling, list_text, utf16_len};
use lite_sql::{SqlExpr, ValueKind};

/// A piece of SQL that writes itself (Java's `SqlWriter.Piece`): written where a helper writes it, as often as it
/// does, its parameters with it.
pub type Piece<'a> = &'a dyn Fn(&mut SqlWriter) -> Result<(), Refusal>;

/// A statement being written.
#[derive(Debug, Default)]
pub struct SqlWriter {
    sql: String,
    binds: Vec<Bind>,
    regions: Vec<Region>,
    /// the per-key region being written (none, or the one: a region holds no other)
    open: Vec<OpenRegion>,
    /// how deep the writer is inside an argument the database evaluates when it plans
    planned: usize,
    /// `sql`'s length in UTF-16 units up to `counted` bytes (Java's positions are UTF-16 indexes)
    counted: usize,
    counted16: usize,
}

#[derive(Debug)]
struct OpenRegion {
    start: usize,
    first_bind: usize,
    counters: Vec<usize>,
}

impl SqlWriter {
    pub fn new() -> SqlWriter {
        SqlWriter::default()
    }

    /// The text written so far.
    pub fn sql(&self) -> &str {
        &self.sql
    }

    pub fn append(&mut self, text: &str) -> &mut SqlWriter {
        self.sql.push_str(text);
        self
    }

    pub fn push(&mut self, c: char) -> &mut SqlWriter {
        self.sql.push(c);
        self
    }

    /// Java's `append(long)`.
    pub fn append_long(&mut self, n: i64) -> &mut SqlWriter {
        self.sql.push_str(&n.to_string());
        self
    }

    /// Writes `e` here, in `d`'s spelling, at `parent_prec` (Java's `writer.expr(e, parentPrec)`).
    pub fn expr<D: AnsiSqlRenderer>(
        &mut self,
        d: &D,
        e: &SqlExpr,
        parent_prec: i32,
    ) -> Result<&mut SqlWriter, Refusal> {
        d.expr(self, e, parent_prec)?;
        Ok(self)
    }

    /// Writes the expressions joined by `separator`, each at `parent_prec`.
    pub fn join_exprs<D: AnsiSqlRenderer>(
        &mut self,
        d: &D,
        es: &[SqlExpr],
        separator: &str,
        parent_prec: i32,
    ) -> Result<&mut SqlWriter, Refusal> {
        for (i, e) in es.iter().enumerate() {
            if i > 0 {
                self.sql.push_str(separator);
            }
            d.expr(self, e, parent_prec)?;
        }
        Ok(self)
    }

    /// Writes the expressions, comma-separated.
    pub fn list<D: AnsiSqlRenderer>(&mut self, d: &D, es: &[SqlExpr]) -> Result<&mut SqlWriter, Refusal> {
        self.join_exprs(d, es, ", ", 0)
    }

    /// Writes `name(args...)`, the arguments comma-separated.
    pub fn function<D: AnsiSqlRenderer>(
        &mut self,
        d: &D,
        name: &str,
        args: &[SqlExpr],
    ) -> Result<&mut SqlWriter, Refusal> {
        self.append(name).append("(").list(d, args)?.append(")");
        Ok(self)
    }

    /// Writes each item as `each` writes it, joined by `separator`.
    pub fn join<T>(
        &mut self,
        items: &[T],
        separator: &str,
        mut each: impl FnMut(&mut SqlWriter, &T) -> Result<(), Refusal>,
    ) -> Result<&mut SqlWriter, Refusal> {
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.sql.push_str(separator);
            }
            each(self, item)?;
        }
        Ok(self)
    }

    /// Writes `piece` here.
    pub fn piece(&mut self, piece: Piece<'_>) -> Result<&mut SqlWriter, Refusal> {
        piece(self)?;
        Ok(self)
    }

    /// Writes `piece` as an argument the database evaluates when it plans.
    pub fn planned(&mut self, piece: Piece<'_>) -> Result<&mut SqlWriter, Refusal> {
        self.planned += 1;
        let written = piece(self);
        self.planned -= 1;
        written?;
        Ok(self)
    }

    /// Whether the writer is inside an argument the database evaluates when it plans.
    pub fn in_planned(&self) -> bool {
        self.planned > 0
    }

    /// Writes a placeholder and records the parameter it binds, at this place in the statement.
    pub fn bind(&mut self, bind: Bind) -> &mut SqlWriter {
        self.bind_as("?", bind)
    }

    /// Writes `placeholder` as this parameter's place and records the bind (a target whose statement is not SQL).
    pub fn bind_as(&mut self, placeholder: &str, bind: Bind) -> &mut SqlWriter {
        self.sql.push_str(placeholder);
        self.binds.push(bind);
        self
    }

    /// How many parameters are bound so far.
    pub fn bound(&self) -> usize {
        self.binds.len()
    }

    /// The text's length so far in UTF-16 units: Java's `sql.length()`.
    fn position(&mut self) -> usize {
        self.counted16 += utf16_len(&self.sql[self.counted..]);
        self.counted = self.sql.len();
        self.counted16
    }

    /// Writes `body` as a per-key region: text the runner writes once per key, `lead` before the first copy and
    /// `separator` between copies, `empty` in its place with no key (the statement refused when that is absent).
    pub fn region(
        &mut self,
        lead: &str,
        separator: &str,
        empty: Option<&str>,
        body: impl FnOnce(&mut SqlWriter) -> Result<(), Refusal>,
    ) -> Result<&mut SqlWriter, Refusal> {
        if !self.open.is_empty() {
            return Err(Refusal::illegal_state("a per-key region inside another per-key region"));
        }
        let start = self.position();
        self.open.push(OpenRegion { start, first_bind: self.binds.len(), counters: Vec::new() });
        body(self)?;
        let r = self.open.pop().expect("the region pushed above");
        let end = self.position();
        let region = Region::new(
            r.start,
            end,
            lead.to_owned(),
            separator.to_owned(),
            empty.map(str::to_owned),
            r.first_bind,
            self.binds.len() - r.first_bind,
            r.counters,
        )?;
        self.regions.push(region);
        Ok(self)
    }

    /// The key's position in the probe, written here in each copy of the per-key region being written.
    pub fn counter(&mut self) -> Result<&mut SqlWriter, Refusal> {
        if self.open.is_empty() {
            return Err(Refusal::illegal_state("a per-key name outside a per-key region: no key to name it by"));
        }
        let at = self.position();
        self.open.last_mut().expect("checked above").counters.push(at);
        Ok(self)
    }

    /// A placeholder binding the probe row's `cell` of each copy's key; refused outside a per-key region.
    pub fn bind_key(&mut self, cell: i32) -> Result<&mut SqlWriter, Refusal> {
        self.bind_key_as("?", cell)
    }

    /// `bind_key` with the placeholder spelled in the target's own language.
    pub fn bind_key_as(&mut self, placeholder: &str, cell: i32) -> Result<&mut SqlWriter, Refusal> {
        if self.open.is_empty() {
            return Err(Refusal::illegal_state("a key's cell outside a per-key region: no key to bind"));
        }
        self.sql.push_str(placeholder);
        self.binds.push(Bind { parameter: String::new(), binding: Binding::key(cell)? });
        Ok(self)
    }

    /// The placeholder bound last is typed by its value, its type written here when the value is known.
    pub fn type_hole(
        &mut self,
        types: Vec<(ValueKind, TypeSpelling)>,
        absent: ValueKind,
    ) -> Result<&mut SqlWriter, Refusal> {
        if !self.open.is_empty() {
            return Err(Refusal::illegal_state(
                "a value-typed placeholder inside a per-key region: the runner fills a type hole once, and a region \
                 is written once per key",
            ));
        }
        let at = self.position();
        match self.binds.last() {
            Some(Bind { binding: Binding::One { hole: None, .. }, .. }) => {}
            _ => {
                return Err(Refusal::illegal_state(format!(
                    "a type hole follows a placeholder of one value with none: {}",
                    list_text(&self.binds)
                )));
            }
        }
        let hole = TypeHole::new(at, types, absent)?;
        if let Some(Bind { binding: Binding::One { hole: slot, .. }, .. }) = self.binds.last_mut() {
            *slot = Some(hole);
        }
        Ok(self)
    }

    /// The statement written, with its parameters in placeholder order.
    pub fn into_statement(self) -> Result<RenderedStatement, Refusal> {
        RenderedStatement::new(self.sql, self.binds, self.regions)
    }

    /// The text written, for a caller that takes SQL as text: refused when a parameter was bound or a region written.
    pub fn into_text(self) -> Result<String, Refusal> {
        if !self.regions.is_empty() {
            return Err(Refusal::illegal_state(
                "a statement with per-key regions was asked for as text: render it as a statement, which the runner \
                 writes once per key",
            ));
        }
        if !self.binds.is_empty() {
            return Err(Refusal::illegal_state(format!(
                "a statement with bound parameters {} was asked for as text: render it as a statement",
                list_text(&self.binds)
            )));
        }
        Ok(self.sql)
    }
}
