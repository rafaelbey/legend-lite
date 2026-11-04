// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `SQLite` (`core/src/main/java/com/legend/sql/dialect/SQLite.java`), the SQLite execution dialect: the base
//! renderer over SQLite's lexicon, every value as its storage class holds it -- a boolean 1 or 0, a date and a
//! date-time their ISO text, a decimal a REAL (a root decimal its text at its scale), a list, a struct, a map and a
//! Variant JSON text (JSON1) -- each list operation a correlated subquery over `json_each`, a fold a correlated `WITH
//! RECURSIVE`, every explosion a marker placed in FROM ([`crate::explosion_markers`]), no VALUES alias list, no
//! quantified comparison, no QUALIFY, no LATERAL; a literal regular expression compiled here to its automaton's table,
//! which a recursive query walks ([`regex`]), the digests written in SQLite's SQL ([`digests`]), base64, Levenshtein and
//! Jaro-Winkler recursive walks; a large repeated operand bound once (`__b<n>`) and a large repeated subexpression
//! written once ([`crate::common_subexpressions`]), as a statement is capped at 1,000,000 bytes.
//!
//! **Per-render state.** Java's dialect object keeps five pieces of state while it writes a statement, each a field
//! here behind a `RefCell` (the renderer takes `&self`), pushed and popped around the same calls Java's are (a guard
//! pops on a refusal too, Java's `finally`): `laterals` and `rowLaterals`, the explosion markers and the lateral
//! subqueries in scope of the select being written; `roots`, the statement's labels as planned (its root's outputs
//! before the passes), and `rootSelects`, the statement's root selects once its root is written; and `binds`, the
//! fresh names' counter, started over at each outermost query render (the user's ruling, 2026-11-03). Java keys
//! `rootSelects` by identity; its last pass, `CommonSubexpressions`, hands every equal select the first one's rewrite,
//! so over the written tree Java's identity is structural equality, and Rust keeps the root selects by content. A
//! select written with a marker or a lateral subquery in scope is never a root select's identity in Java (it is
//! respelled first): Rust delivers its values only when there is none, as Java does.
//!
//! Java's `SqlWriter.Piece` is [`P`], a shared closure, as Postgres's: the helpers compose pieces exactly as Java's
//! lambdas do, and what Java evaluates when a piece is made (a refusal among it) Rust evaluates then too.

mod aggregates;
pub mod digests;
mod functions;
pub mod regex;

use crate::ansi::{self, query_outputs};
use crate::build;
use crate::dialect::KeysSnapshot;
use crate::explosion_markers::{self, exploded_read, is_marker, is_marker_select, marker_column, null_list, unnest_list};
use crate::java::{big_decimal_plain, sql_type_text};
use crate::refusal::{Refusal, arg};
use crate::renderer::{AnsiSqlRenderer, AnsiState, Lexicon};
use crate::rewriter::SqlRewriter;
use crate::statement::{Digits, RenderedStatement, TypeSpelling};
use crate::typing::{children, source_alias};
use crate::writer::SqlWriter;
use lite_sql::{
    AggFn, Column, ColumnKind, ColumnType, Cte, DateFmt, JoinKind, JsonArrayAggKey, JsonKind, Lambda, NameQuoting,
    Origin, OutputCol, Projection, Quantifier, Reducer, Scalar, SqlDdl, SqlDml, SqlExpr, SqlFn, SqlQuery, SqlSelect,
    SqlSource, SqlType, StructField, StructLitField, TypeFact, ValueKind,
};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::LazyLock;

/// SQLite's reserved words (lang_keywords), Java's `LEXICON`'s.
pub const RESERVED: &[&str] = &[
    "abort",
    "action",
    "add",
    "after",
    "all",
    "alter",
    "analyze",
    "and",
    "as",
    "asc",
    "attach",
    "autoincrement",
    "before",
    "begin",
    "between",
    "by",
    "cascade",
    "case",
    "cast",
    "check",
    "collate",
    "column",
    "commit",
    "conflict",
    "constraint",
    "create",
    "cross",
    "current_date",
    "current_time",
    "current_timestamp",
    "database",
    "default",
    "deferrable",
    "deferred",
    "delete",
    "desc",
    "detach",
    "distinct",
    "drop",
    "each",
    "else",
    "end",
    "escape",
    "except",
    "exclusive",
    "exists",
    "explain",
    "fail",
    "for",
    "foreign",
    "from",
    "full",
    "glob",
    "group",
    "having",
    "if",
    "ignore",
    "immediate",
    "in",
    "index",
    "indexed",
    "initially",
    "inner",
    "insert",
    "instead",
    "intersect",
    "into",
    "is",
    "isnull",
    "join",
    "key",
    "left",
    "like",
    "limit",
    "match",
    "natural",
    "no",
    "not",
    "notnull",
    "null",
    "of",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "plan",
    "pragma",
    "primary",
    "query",
    "raise",
    "recursive",
    "references",
    "regexp",
    "reindex",
    "release",
    "rename",
    "replace",
    "restrict",
    "right",
    "rollback",
    "row",
    "savepoint",
    "select",
    "set",
    "table",
    "temp",
    "temporary",
    "then",
    "to",
    "transaction",
    "trigger",
    "union",
    "unique",
    "update",
    "using",
    "vacuum",
    "values",
    "view",
    "virtual",
    "when",
    "where",
    "with",
    "without",
];

/// Java's `SQLite.LEXICON`: `"` and [`RESERVED`].
static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| Lexicon::new('"', RESERVED));

/// Java's `SQLite.CAPS`: the portable carrier strategies over its JSON lists (`json_each` correlates); FULL OUTER is
/// native (3.39).
pub const CAPS: crate::carrier::Caps =
    crate::carrier::Caps { native_lists: false, correlated_explode: true, json_carrier: true, native_full_outer: true };

/// One select's explosion markers (Java's `LinkedHashMap<String, Map<String, SqlExpr>>`): each marker's alias, then
/// each exploded column's list, in insertion order.
type Markers = Vec<(String, Vec<(String, SqlExpr)>)>;

/// One select's lateral subqueries read as JSON rows (Java's `LinkedHashMap<String, List<OutputCol>>`).
type RowLaterals = Vec<(String, Vec<OutputCol>)>;

/// The SQLite dialect: one per render, as the seam makes it (Java's `new SQLite()`), its state the statement's.
#[derive(Debug, Default)]
pub struct Sqlite {
    ansi: AnsiState,
    /// the explosion markers in scope of the select being written: alias, then each exploded column's list
    laterals: RefCell<Vec<Markers>>,
    /// the lateral subqueries in scope of the select being written, read as JSON rows: alias, then its columns
    row_laterals: RefCell<Vec<RowLaterals>>,
    /// the labels of the statement being rendered, as planned
    roots: RefCell<Vec<Vec<OutputCol>>>,
    /// the statement's root selects once its root is written; none while the root is still to come
    root_selects: RefCell<Vec<Option<Vec<SqlSelect>>>>,
    /// the fresh names' counter (`__b<n>`), started over by each outermost render
    binds: Cell<u64>,
}

impl Sqlite {
    /// Java's `new SQLite()`.
    pub fn new() -> Sqlite {
        Sqlite::default()
    }
}

// ==================================================================
// Pieces and the type facts the arms read
// ==================================================================

/// Java's `SqlWriter.Piece`: SQL that writes itself, shareable among the helpers that write it.
pub type P<'a> = Rc<dyn Fn(&mut SqlWriter) -> Result<(), Refusal> + 'a>;

pub(crate) fn pc<'a>(f: impl Fn(&mut SqlWriter) -> Result<(), Refusal> + 'a) -> P<'a> {
    Rc::new(f)
}

/// A piece of fixed text.
fn text<'a>(s: impl Into<String>) -> P<'a> {
    let s = s.into();
    pc(move |w| {
        w.append(&s);
        Ok(())
    })
}

/// Java's `typeOf(e)`: the type a node is typed as, or none.
fn type_of(e: &SqlExpr) -> Option<&SqlType> {
    fact_type(e.r#type())
}

fn fact_type(f: &TypeFact) -> Option<&SqlType> {
    match f {
        TypeFact::Typed { r#type, .. } => Some(r#type),
        _ => None,
    }
}

/// Java's `listElement(e)`: the element type of a list, or none for a value that is no list.
fn list_element(e: &SqlExpr) -> Option<&SqlType> {
    fact_list_element(e.r#type())
}

fn fact_list_element(f: &TypeFact) -> Option<&SqlType> {
    match fact_type(f) {
        Some(SqlType::Array { element }) => Some(element),
        _ => None,
    }
}

fn scalar_is(t: Option<&SqlType>, s: Scalar) -> bool {
    matches!(t, Some(SqlType::Scalar(x)) if *x == s)
}

/// A list, a struct, a map or a Variant: a value held as JSON text.
fn nested(t: Option<&SqlType>) -> bool {
    matches!(t, Some(SqlType::Array { .. } | SqlType::Struct { .. } | SqlType::Map { .. }))
        || scalar_is(t, Scalar::Json)
}

fn is_json(e: &SqlExpr) -> bool {
    scalar_is(type_of(e), Scalar::Json)
}

fn is_text(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::StringLit { .. }) || scalar_is(type_of(e), Scalar::Varchar)
}

fn is_double(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::FloatLit { .. }) || scalar_is(type_of(e), Scalar::Double)
}

fn is_integer(e: &SqlExpr) -> bool {
    let t = type_of(e);
    matches!(e, SqlExpr::IntLit { .. })
        || scalar_is(t, Scalar::Integer)
        || scalar_is(t, Scalar::Bigint)
        || scalar_is(t, Scalar::Hugeint)
}

fn json_type() -> SqlType {
    SqlType::Scalar(Scalar::Json)
}

/// A type's `toString` as a refusal names it (`null` for none).
fn type_text(t: Option<&SqlType>) -> String {
    t.map_or_else(|| "null".to_owned(), sql_type_text)
}

/// The element read by `json_each`'s columns as a value of type `t`, of the `json_each` aliased `alias` (Java's
/// `elementRead`): a Variant element is its JSON text, anything else the `value` column.
fn element_read(t: Option<&SqlType>, alias: &str) -> String {
    let q = if alias.is_empty() { String::new() } else { format!("{alias}.") };
    if scalar_is(t, Scalar::Json) {
        format!(
            "CASE {q}type WHEN 'true' THEN 'true' WHEN 'false' THEN 'false' WHEN 'null' THEN 'null' WHEN 'array' THEN \
             {q}value WHEN 'object' THEN {q}value ELSE json_quote({q}value) END"
        )
    } else {
        format!("{q}value")
    }
}

/// A value of type `t` as a JSON container holds it (Java's `jsonOf`): a boolean as JSON true or false, a list,
/// struct, map or Variant as its JSON, anything else as it is.
fn json_of<'a>(v: P<'a>, t: Option<&SqlType>) -> P<'a> {
    if scalar_is(t, Scalar::Boolean) {
        return pc(move |w| {
            w.append("json(CASE WHEN ");
            v(w)?;
            w.append(" THEN 'true' WHEN NOT (");
            v(w)?;
            w.append(") THEN 'false' END)");
            Ok(())
        });
    }
    if nested(t) {
        return pc(move |w| {
            w.append("json(");
            v(w)?;
            w.append(")");
            Ok(())
        });
    }
    v
}

/// A refusal of a list plan parameter: SQLite binds no array (Java's `listParameter`).
fn list_parameter(name: &str) -> Refusal {
    Refusal::capability(format!("list plan parameter '{name}' reached SQLite, which binds no array"))
}

fn wall(f: SqlFn, why: &str) -> Refusal {
    Refusal::capability(format!("{f} reached SQLite: {why}"))
}

/// A string literal's value, refused by `what` when it is none (Java's `literal`).
fn literal<'e>(e: &'e SqlExpr, what: &str) -> Result<&'e str, Refusal> {
    match e {
        SqlExpr::StringLit { value, .. } => Ok(value),
        _ => Err(Refusal::capability(format!("{what} that is not a literal reached SQLite"))),
    }
}

/// `e`'s size as written, counted up to `cap` (Java's `expandedSize`).
fn expanded_size(e: &SqlExpr, cap: usize) -> usize {
    let mut n = 1;
    for k in children(e) {
        if n >= cap {
            return n;
        }
        n += expanded_size(&k, cap - n);
    }
    n
}

/// An aggregate, a window or a lambda anywhere (Java's `grouped`).
fn grouped(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::Reducer(_) | SqlExpr::WindowCall { .. } | SqlExpr::Lambda(_))
        || children(e).iter().any(|k| grouped(k))
}

/// A large operand (a dozen nodes or more) with no lambda, aggregate or window: one a repeating spelling binds once
/// (Java's `bindable`).
fn bindable(x: &SqlExpr) -> bool {
    !matches!(x, SqlExpr::Lambda(_) | SqlExpr::Column(_)) && expanded_size(x, 12) >= 12 && !grouped(x)
}

/// The calls whose spelling writes an operand more than once (Java's `REPEATING`).
const REPEATING: &[SqlFn] = &[
    SqlFn::Lpad,
    SqlFn::Rpad,
    SqlFn::Left,
    SqlFn::Right,
    SqlFn::StartsWith,
    SqlFn::EndsWith,
    SqlFn::Split,
    SqlFn::SplitPart,
    SqlFn::PureSplitPart,
    SqlFn::ParseInt,
    SqlFn::Round,
    SqlFn::RoundHalfUp,
    SqlFn::Greatest,
    SqlFn::Least,
    SqlFn::Mod,
    SqlFn::Rem,
    SqlFn::Cbrt,
    SqlFn::AsciiCode,
    SqlFn::Acos,
    SqlFn::Asin,
    SqlFn::BoolToText,
    SqlFn::ListGet,
    SqlFn::ListSlice,
    SqlFn::ListInit,
    SqlFn::ListTail,
    SqlFn::ListDistinct,
    SqlFn::ListSort,
    SqlFn::ListSortDesc,
    SqlFn::ListReverse,
    SqlFn::ListFlatten,
    SqlFn::ListFilter,
    SqlFn::ListTransform,
    SqlFn::ListReduce,
    SqlFn::MapKeys,
    SqlFn::MapValues,
    SqlFn::MapExtract,
    SqlFn::EpochSeconds,
    SqlFn::EpochMs,
    SqlFn::AddInterval,
    SqlFn::AddIntervalTemporal,
    SqlFn::DateDiff,
    SqlFn::TimeBucket,
    SqlFn::VariantGet,
    SqlFn::MakeTimestamp,
];

/// A double's shortest round-trip text, of `__t.v` (Java's `SHORTEST`).
const SHORTEST: &str = "CASE WHEN __t.v IS NULL THEN NULL WHEN CAST(printf('%!.15g', __t.v) AS REAL) = __t.v THEN \
                        printf('%!.15g', __t.v) WHEN CAST(printf('%!.16g', __t.v) AS REAL) = __t.v THEN printf('%!.16g', \
                        __t.v) ELSE printf('%!.17g', __t.v) END";

// ==================================================================
// The statement's state
// ==================================================================

/// The root's state pushed for one statement, popped when it is written or refused (Java's `finally`).
struct RootScope<'a>(&'a Sqlite);

impl Drop for RootScope<'_> {
    fn drop(&mut self) {
        self.0.roots.borrow_mut().pop();
        self.0.root_selects.borrow_mut().pop();
    }
}

/// The laterals pushed for one select, popped when it is written or refused.
struct LateralScope<'a>(&'a Sqlite);

impl Drop for LateralScope<'_> {
    fn drop(&mut self) {
        self.0.laterals.borrow_mut().pop();
        self.0.row_laterals.borrow_mut().pop();
    }
}

/// The statement's root selects (the root, a union's branches, a WITH's body), or a select of nothing when there is
/// none (Java's `selectsOf`).
fn selects_of(q: &SqlQuery) -> Vec<SqlSelect> {
    fn collect(q: &SqlQuery, into: &mut Vec<SqlSelect>) {
        match q {
            SqlQuery::Select(s) => into.push(s.as_ref().clone()),
            SqlQuery::Union(u) => u.branches.iter().for_each(|b| collect(b, into)),
            SqlQuery::With(w) => collect(&w.body, into),
        }
    }
    let mut out = Vec::new();
    collect(q, &mut out);
    out
}

impl Sqlite {
    /// A statement's root state, pushed (Java's `roots.push`, `rootSelects.push(new IdentityHashMap<>())`).
    fn enter(&self, labels: Vec<OutputCol>) -> RootScope<'_> {
        self.roots.borrow_mut().push(labels);
        self.root_selects.borrow_mut().push(None);
        RootScope(self)
    }

    /// An outermost render starts the statement's fresh names over.
    fn start_names(&self) {
        if self.roots.borrow().is_empty() {
            self.binds.set(0);
        }
    }

    /// Whether `s` is one of the statement's root selects (Java's `TRUE.equals(top.get(s))`).
    fn is_root_select(&self, s: &SqlSelect) -> bool {
        match self.root_selects.borrow().last() {
            Some(Some(roots)) => roots.iter().any(|r| r == s),
            _ => false,
        }
    }

    // ==================================================================
    // Structure: the root's values, the markers, the lateral rows
    // ==================================================================

    /// A root select's values as their labels deliver them (Java's `delivered`): a decimal as its text at its scale, a
    /// Float as a REAL, a big integer raised past 64 bits; a sort key naming a rewritten column its original
    /// expression.
    fn delivered<'s>(&self, s: &'s SqlSelect) -> Result<Cow<'s, SqlSelect>, Refusal> {
        let labels = self.roots.borrow().last().cloned();
        let Some(labels) = labels else {
            return Ok(Cow::Borrowed(s));
        };
        if !self.is_root_select(s) || labels.len() != s.projections.len() {
            return Ok(Cow::Borrowed(s));
        }
        let mut ps = Vec::with_capacity(s.projections.len());
        let mut moved: Vec<(String, SqlExpr)> = Vec::new();
        for (i, p) in s.projections.iter().enumerate() {
            match delivered_value(&p.expr, &labels[i].r#type) {
                Some(value) => {
                    let name = output_name(p);
                    if let Some(n) = &name {
                        match moved.iter_mut().find(|(k, _)| k == n) {
                            Some(slot) => slot.1 = p.expr.clone(),
                            None => moved.push((n.clone(), p.expr.clone())),
                        }
                    }
                    ps.push(build::projection(value, name, p.out.clone())?);
                }
                None => ps.push(p.clone()),
            }
        }
        if moved.is_empty() {
            return Ok(Cow::Borrowed(s));
        }
        let order_by = s
            .order_by
            .iter()
            .map(|k| match &k.expr {
                SqlExpr::Column(c) if c.table.is_none() => match moved.iter().find(|(n, _)| *n == c.name) {
                    Some((_, original)) => {
                        build::sort_key(original.clone(), k.ascending, k.null_order, k.output_name.clone())
                    }
                    None => k.clone(),
                },
                _ => k.clone(),
            })
            .collect();
        Ok(Cow::Owned(build::select(SqlSelect { projections: ps, order_by, ..s.clone() })?))
    }

    /// The select `s` written: its markers and lateral subqueries in scope, a star over one spelled out, a root
    /// select's values delivered, an OFFSET without a LIMIT given `LIMIT -1`.
    fn select_written(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        let mut scope = Vec::new();
        laterals_of(&s.from, &mut scope)?;
        let mut rows = Vec::new();
        row_laterals_of(&s.from, &mut rows);
        let spelled: Cow<'_, SqlSelect> =
            if scope.is_empty() && rows.is_empty() { self.delivered(s)? } else { Cow::Owned(stars_spelled(s)?) };
        self.laterals.borrow_mut().push(scope);
        self.row_laterals.borrow_mut().push(rows);
        let _scope = LateralScope(self);
        if spelled.offset.is_some() && spelled.limit.is_none() {
            let limited = build::with_limit(spelled.into_owned(), Some(-1))?;
            ansi::select(self, w, &limited, depth)
        } else {
            ansi::select(self, w, &spelled, depth)
        }
    }

    /// A column of a lateral subquery read as JSON rows: its table and whether it reads a Variant (`->`), or none for
    /// any other column (Java's `rowLateralField`).
    fn row_lateral_field(&self, c: &Column) -> Option<(String, bool)> {
        let table = c.table.as_ref()?;
        let row_laterals = self.row_laterals.borrow();
        for scope in row_laterals.iter().rev() {
            if let Some((_, outs)) = scope.iter().find(|(alias, _)| alias == table) {
                let t = outs.iter().find(|oc| oc.name == c.name).map(|oc| &oc.r#type).or_else(|| type_of_column(c));
                return Some((table.clone(), scalar_is(t, Scalar::Json)));
            }
        }
        None
    }

    /// A lateral subquery's rows, for a database with no LATERAL (Java's `lateralRows`): the subquery, correlated,
    /// aggregated as a JSON array of objects, and `json_each` over it.
    fn lateral_rows<'a>(&'a self, inner: &'a SqlQuery, depth: usize) -> Result<P<'a>, Refusal> {
        let outs = query_outputs(inner);
        if outs.is_empty() {
            return Err(Refusal::capability("a lateral subquery of undeclared columns reached SQLite"));
        }
        Ok(pc(move |w| {
            w.append("json_each((SELECT json_group_array(json_object(");
            for (i, oc) in outs.iter().enumerate() {
                let name = self.lit(&oc.name);
                w.append(if i > 0 { ", " } else { "" }).append(&name).append(", ");
                let column = ansi::column_of(Some("__l"), oc);
                let reference = self.column_ref(&column)?;
                json_of(text(reference), Some(&oc.r#type))(w)?;
            }
            w.append(")) FROM (");
            self.nl(w, depth + 1);
            self.query(w, inner, depth + 1)?;
            self.nl(w, depth);
            w.append(") AS __l))");
            Ok(())
        }))
    }

    /// An exploded column's list and the marker's alias, or none for any other column (Java's `explodedColumn`).
    fn exploded_column(&self, c: &Column) -> Option<(SqlExpr, String)> {
        let laterals = self.laterals.borrow();
        for scope in laterals.iter().rev() {
            match &c.table {
                Some(table) => {
                    if let Some((_, cols)) = scope.iter().find(|(alias, _)| alias == table)
                        && let Some((_, list)) = cols.iter().find(|(name, _)| *name == c.name)
                    {
                        return Some((list.clone(), table.clone()));
                    }
                }
                None => {
                    let owners: Vec<&(String, Vec<(String, SqlExpr)>)> =
                        scope.iter().filter(|(_, cols)| cols.iter().any(|(name, _)| *name == c.name)).collect();
                    if owners.len() == 1 {
                        let (alias, cols) = owners[0];
                        let list = cols.iter().find(|(name, _)| *name == c.name).map(|(_, l)| l.clone());
                        return list.map(|l| (l, alias.clone()));
                    }
                }
            }
        }
        None
    }

    /// `xs[position]`: the list's element at the marker's 0-based position (NULL past its end; Java's `element`).
    fn element<'a>(&'a self, xs: SqlExpr, position: &str) -> P<'a> {
        if null_list(&xs) {
            return text("NULL");
        }
        let index = text(format!("{}.key", self.alias_ident(position)));
        self.element_at(Cow::Owned(xs), index)
    }

    /// The element of `xs` at a 0-based `index` (NULL past either end): a Variant's as JSON (`->`), anything else's
    /// as its value (Java's `elementAt`).
    fn element_at<'a>(&'a self, xs: Cow<'a, SqlExpr>, index: P<'a>) -> P<'a> {
        let variant = scalar_is(list_element(&xs), Scalar::Json);
        pc(move |w| {
            w.append(if variant { "(" } else { "json_extract(" }).expr(self, &xs, if variant { 9 } else { 0 })?;
            w.append(if variant { " -> ('$[' || (" } else { ", '$[' || (" });
            index(w)?;
            w.append(if variant { ") || ']'))" } else { ") || ']')" });
            Ok(())
        })
    }

    /// A marker's positions: `json_each` over its longest list (Java's `positions`).
    fn positions<'a>(&'a self, m: &'a SqlSelect) -> Result<P<'a>, Refusal> {
        let mut lists = Vec::new();
        for p in &m.projections {
            let xs = unnest_list(&p.expr)?;
            if !null_list(xs) {
                lists.push(xs);
            }
        }
        Ok(pc(move |w| {
            w.append("json_each(");
            if lists.is_empty() {
                w.append("NULL");
            } else if lists.len() == 1 {
                w.expr(self, lists[0], 0)?;
            } else {
                // the longest list (its positions are every list's)
                w.append("CASE");
                for i in 0..lists.len() - 1 {
                    let xs = lists[i];
                    w.append(" WHEN ");
                    for (j, other) in lists.iter().enumerate() {
                        if j != i {
                            w.append(if j > if i == 0 { 1 } else { 0 } { " AND " } else { "" });
                            w.append("coalesce(json_array_length(").expr(self, xs, 0)?;
                            w.append("), 0) >= coalesce(json_array_length(").expr(self, other, 0)?.append("), 0)");
                        }
                    }
                    w.append(" THEN ").expr(self, xs, 0)?;
                }
                w.append(" ELSE ").expr(self, lists[lists.len() - 1], 0)?.append(" END");
            }
            w.append(")");
            Ok(())
        }))
    }

    // ==================================================================
    // Lexical
    // ==================================================================

    /// Java's `stringLit`: a NUL cannot stand in a literal, so it is `char(0)` between segments.
    fn lit(&self, value: &str) -> String {
        if value.contains('\0') {
            let mut sb = String::from("(");
            for (i, part) in value.split('\0').enumerate() {
                if i > 0 {
                    sb.push_str(" || char(0) || ");
                }
                sb.push('\'');
                sb.push_str(&part.replace('\'', "''"));
                sb.push('\'');
            }
            sb.push(')');
            return sb;
        }
        format!("'{}'", value.replace('\'', "''"))
    }

    /// `v cmp ANY|ALL (subquery)`, its values counted in SQL's three-valued logic (Java's `quantified`).
    fn quantified<'a>(
        &'a self,
        value: &SqlExpr,
        comparison: SqlFn,
        quantifier: Quantifier,
        subquery: &'a SqlQuery,
    ) -> P<'a> {
        let cmp = build::call(comparison, vec![value.clone(), build::derived(None, "__qc")]);
        let any = quantifier == Quantifier::Any;
        pc(move |w| {
            w.append("(WITH __q(__qc) AS (");
            ansi::inline(self, w, subquery)?;
            w.append(") SELECT CASE WHEN ");
            if any {
                w.append("count(CASE WHEN ").expr(self, &cmp, 0)?;
                w.append(" THEN 1 END) > 0 THEN 1 WHEN count(*) = 0 THEN 0");
            } else {
                w.append("count(*) = 0 THEN 1 WHEN count(CASE WHEN NOT (").expr(self, &cmp, 0)?;
                w.append(") THEN 1 END) > 0 THEN 0");
            }
            w.append(" WHEN count(CASE WHEN (").expr(self, &cmp, 0)?;
            w.append(") IS NULL THEN 1 END) > 0 THEN NULL ELSE ").append(if any { "0" } else { "1" });
            w.append(" END FROM __q)");
            Ok(())
        })
    }

    // ==================================================================
    // JSON: how a list, a struct, a map and a Variant hold a value
    // ==================================================================

    /// `e`, written at `prec`, as a piece.
    fn ex<'a>(&'a self, e: &'a SqlExpr, prec: i32) -> P<'a> {
        pc(move |w| {
            w.expr(self, e, prec)?;
            Ok(())
        })
    }

    /// `a`'s `i`-th argument, written at `prec`, read when written (Java's `w -> w.expr(a.get(i), prec)`).
    fn arg_ex<'a>(&'a self, a: &'a [SqlExpr], i: usize, prec: i32) -> P<'a> {
        pc(move |w| {
            w.expr(self, arg(a, i)?, prec)?;
            Ok(())
        })
    }

    /// An expression the piece owns, written at `prec`.
    fn ex_cow<'a>(&'a self, e: Cow<'a, SqlExpr>, prec: i32) -> P<'a> {
        pc(move |w| {
            w.expr(self, &e, prec)?;
            Ok(())
        })
    }

    /// A value as a JSON container holds it (Java's `json`).
    fn json<'a>(&'a self, x: &'a SqlExpr) -> P<'a> {
        self.json_cow(Cow::Borrowed(x))
    }

    fn json_cow<'a>(&'a self, x: Cow<'a, SqlExpr>) -> P<'a> {
        let mut t = type_of(&x).cloned();
        if t.is_none() && matches!(x.as_ref(), SqlExpr::ArrayLit { .. } | SqlExpr::StructLit { .. }) {
            return pc(move |w| {
                w.append("json(").expr(self, &x, 0)?.append(")");
                Ok(())
            });
        }
        if t.is_none() && matches!(x.as_ref(), SqlExpr::BoolLit { .. }) {
            t = Some(SqlType::Scalar(Scalar::Boolean));
        }
        json_of(self.ex_cow(x, 0), t.as_ref())
    }

    /// A lambda parameter (or an element's or index's name) as its references spell it (Java's `param`).
    fn param(&self, name: &str) -> Result<String, Refusal> {
        self.column_ref(&Column { table: None, name: name.to_owned(), r#type: TypeFact::Unknown, origin: Some(Origin::Derived) })
    }

    /// `(SELECT <element> AS e, key AS o [, key + 1 AS i] FROM json_each(xs)) AS e__t` (Java's `elements`).
    fn elements<'a>(&'a self, xs: &'a SqlExpr, e: &str, o: &str, i: Option<&str>) -> Result<P<'a>, Refusal> {
        let read = element_read(list_element(xs), "");
        let pe = self.param(e)?;
        let po = self.param(o)?;
        let pi = match i {
            Some(i) => Some(self.param(i)?),
            None => None,
        };
        let pt = self.param(&format!("{e}__t"))?;
        Ok(pc(move |w| {
            w.append("(SELECT ").append(&read).append(" AS ").append(&pe).append(", key AS ").append(&po);
            if let Some(pi) = &pi {
                w.append(", key + 1 AS ").append(pi);
            }
            w.append(" FROM json_each(").expr(self, xs, 0)?.append(")) AS ").append(&pt);
            Ok(())
        }))
    }

    /// `body`, or NULL for a NULL list (Java's `unlessNull`).
    fn unless_null<'a>(&'a self, xs: &'a SqlExpr, body: P<'a>) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, xs, 4)?.append(" IS NULL THEN NULL ELSE ");
            body(w)?;
            w.append(" END)");
            Ok(())
        })
    }

    /// `(SELECT json_group_array(select ORDER BY o) FROM elements [WHERE where])`, NULL for a NULL list (Java's
    /// `overElements`).
    fn over_elements<'a>(
        &'a self,
        xs: &'a SqlExpr,
        e: &str,
        o: &str,
        i: Option<&str>,
        select: P<'a>,
        r#where: Option<P<'a>>,
    ) -> Result<P<'a>, Refusal> {
        let po = self.param(o)?;
        let (e, o, i) = (e.to_owned(), o.to_owned(), i.map(str::to_owned));
        Ok(self.unless_null(
            xs,
            pc(move |w| {
                w.append("(SELECT json_group_array(");
                select(w)?;
                w.append(" ORDER BY ").append(&po).append(") FROM ");
                self.elements(xs, &e, &o, i.as_deref())?(w)?;
                if let Some(wh) = &r#where {
                    w.append(" WHERE ");
                    wh(w)?;
                }
                w.append(")");
                Ok(())
            }),
        ))
    }

    /// `(SELECT agg(e) FROM elements)`: a list reduced as rows; MODE the most frequent non-NULL element, the first
    /// among ties (Java's `overRows`).
    fn over_rows<'a>(&'a self, xs: &'a SqlExpr, f: AggFn, extras: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let e = self.param("__e")?;
        let o = self.param("__o")?;
        if f == AggFn::Mode {
            if !extras.is_empty() {
                return Err(Refusal::capability("a mode with arguments reached SQLite"));
            }
            return Ok(pc(move |w| {
                w.append(&format!("(SELECT {e} FROM "));
                self.elements(xs, "__e", "__o", None)?(w)?;
                w.append(&format!(
                    " WHERE {e} IS NOT NULL GROUP BY {e} ORDER BY count(*) DESC, min({o}) LIMIT 1)"
                ));
                Ok(())
            }));
        }
        let el = match list_element(xs) {
            None => build::derived(None, "__e"),
            Some(t) => build::typed_column(None, "__e", t.clone(), true, Origin::Derived),
        };
        let mut args = vec![el];
        args.extend(extras.iter().cloned());
        let order = if matches!(f, AggFn::StringAgg | AggFn::List) {
            vec![build::sort_key(build::derived(None, "__o"), true, None, None)]
        } else {
            Vec::new()
        };
        let r = build::reducer(f, args, false, order);
        Ok(pc(move |w| {
            w.append("(SELECT ");
            self.reducer(w, &r)?;
            w.append(" FROM ");
            self.elements(xs, "__e", "__o", None)?(w)?;
            w.append(")");
            Ok(())
        }))
    }

    /// The position alias of a lambda's element (Java's `offsetOf`).
    fn offset_of(l: &Lambda) -> Result<String, Refusal> {
        Ok(format!("{}__o", param_at(l, 0)?))
    }

    /// `body` over `operands`: each large one bound ONCE as a column of a one-row derived table under a scalar
    /// subquery (Java's `over`). Its alias, `__b<n>`, is taken when the piece is made.
    fn over<'a>(&'a self, operands: &'a [SqlExpr], body: impl FnOnce(Vec<SqlExpr>) -> P<'a>) -> P<'a> {
        let n = self.binds.get();
        self.binds.set(n + 1);
        let alias = format!("__b{n}");
        let mut refs = Vec::with_capacity(operands.len());
        let mut bound = Vec::new();
        for (i, x) in operands.iter().enumerate() {
            if bindable(x) {
                let name = format!("__a{i}");
                refs.push(match type_of(x) {
                    None => build::derived(Some(&alias), &name),
                    Some(t) => build::typed_column(Some(&alias), &name, t.clone(), true, Origin::Derived),
                });
                bound.push(i);
            } else {
                refs.push(x.clone());
            }
        }
        let b = body(refs);
        if bound.is_empty() {
            return b;
        }
        pc(move |w| {
            w.append("(SELECT ");
            b(w)?;
            w.append(" FROM (SELECT ");
            for (k, i) in bound.iter().enumerate() {
                w.append(if k > 0 { ", " } else { "" }).expr(self, &operands[*i], 0)?;
                w.append(&format!(" AS __a{i}"));
            }
            w.append(&format!(") AS {alias})"));
            Ok(())
        })
    }

    // ==================================================================
    // Variant: JSON text
    // ==================================================================

    /// A value as a Variant's JSON text (Java's `variantOf`).
    fn variant_of<'a>(&'a self, x: &'a SqlExpr) -> P<'a> {
        let t = type_of(x);
        if scalar_is(t, Scalar::Json) {
            return self.ex(x, 0);
        }
        if nested(t) || matches!(x, SqlExpr::ArrayLit { .. } | SqlExpr::StructLit { .. }) {
            return pc(move |w| {
                w.append("json(").expr(self, x, 0)?.append(")");
                Ok(())
            });
        }
        if scalar_is(t, Scalar::Boolean) || matches!(x, SqlExpr::BoolLit { .. }) {
            return pc(move |w| {
                w.append("(CASE WHEN ").expr(self, x, 0)?.append(" THEN 'true' WHEN NOT (").expr(self, x, 0)?;
                w.append(") THEN 'false' END)");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("json_quote(").expr(self, x, 0)?.append(")");
            Ok(())
        })
    }

    /// A Variant's value as `to` (Java's `fromVariant`, its name kept).
    #[allow(clippy::wrong_self_convention)]
    fn from_variant<'a>(&'a self, v: &'a SqlExpr, to: &SqlType) -> Result<P<'a>, Refusal> {
        let member = matches!(v, SqlExpr::Call { r#fn: SqlFn::VariantGet, .. });
        let varchar = SqlType::Scalar(Scalar::Varchar);
        if *to == varchar && !member {
            return Ok(self.ex(v, 0));
        }
        if *to == varchar {
            return Ok(pc(move |w| {
                w.append("(CASE WHEN json_type(").expr(self, v, 0)?.append(") = 'text' THEN (").expr(self, v, 9)?;
                w.append(" ->> '$') ELSE ").expr(self, v, 0)?.append(" END)");
                Ok(())
            }));
        }
        if *to == SqlType::Scalar(Scalar::Boolean) {
            return Ok(pc(move |w| {
                w.append("(CASE json_type(").expr(self, v, 0)?.append(") WHEN 'true' THEN 1 WHEN 'false' THEN 0");
                w.append(" WHEN 'text' THEN CASE lower(").expr(self, v, 9)?.append(" ->> '$') WHEN 'true' THEN 1 WHEN");
                w.append(" 'false' THEN 0 END WHEN 'integer' THEN (").expr(self, v, 9)?.append(" ->> '$') <> 0 END)");
                Ok(())
            }));
        }
        if let SqlType::Array { element } = to {
            let j = self.param("__j")?;
            let jo = self.param("__jo")?;
            let each = if **element == json_type() {
                text(j.clone())
            } else {
                self.cast_to(text(format!("({j} ->> '$')")), Some(&json_type()), element)
            };
            let held = json_of(each, Some(element));
            let read = element_read(Some(&json_type()), "");
            return Ok(pc(move |w| {
                w.append("(CASE WHEN json_type(").expr(self, v, 0)?.append(") = 'array' THEN (SELECT ");
                w.append("json_group_array(");
                held(w)?;
                w.append(" ORDER BY ").append(&jo).append(") FROM (SELECT ").append(&read).append(" AS ");
                w.append(&j).append(", key AS ").append(&jo).append(" FROM json_each(").expr(self, v, 0)?;
                w.append("))) END)");
                Ok(())
            }));
        }
        if matches!(to, SqlType::Map { .. } | SqlType::Struct { .. }) {
            return Ok(self.ex(v, 0));
        }
        let read = pc(move |w| {
            w.append("(").expr(self, v, 9)?.append(" ->> '$')");
            Ok(())
        });
        Ok(self.cast_to(read, Some(&json_type()), to))
    }

    /// `value` (of type `from`, when known) cast to `to`, each cast decided here (Java's `castTo`).
    fn cast_to<'a>(&'a self, value: P<'a>, from: Option<&SqlType>, to: &SqlType) -> P<'a> {
        if let SqlType::Decimal { scale, .. } = to {
            // DuckDB's decimal cast rounds to the scale, half away from zero (as round does)
            let tail = format!(" AS REAL), {scale})");
            return pc(move |w| {
                w.append("round(CAST(");
                value(w)?;
                w.append(&tail);
                Ok(())
            });
        }
        let SqlType::Scalar(s) = to else {
            return value;
        };
        let wrap = |head: &'static str, tail: &'static str, value: P<'a>| -> P<'a> {
            pc(move |w| {
                w.append(head);
                value(w)?;
                w.append(tail);
                Ok(())
            })
        };
        match s {
            Scalar::Varchar => self.text_of(value, from),
            Scalar::Boolean => {
                if scalar_is(from, Scalar::Varchar) || scalar_is(from, Scalar::Json) {
                    wrap("(CASE lower(", ") WHEN 'true' THEN 1 WHEN 'false' THEN 0 END)", value)
                } else {
                    wrap("(", " <> 0)", value)
                }
            }
            // DuckDB rounds a double to an integer, half away from zero (SQLite's CAST truncates)
            Scalar::Integer | Scalar::Bigint | Scalar::Hugeint => {
                if scalar_is(from, Scalar::Double) || matches!(from, Some(SqlType::Decimal { .. })) {
                    wrap("CAST(round(", ") AS INTEGER)", value)
                } else {
                    wrap("CAST(", " AS INTEGER)", value)
                }
            }
            Scalar::Double => wrap("CAST(", " AS REAL)", value),
            Scalar::Date => wrap("date(", ")", value),
            Scalar::Timestamp => {
                if scalar_is(from, Scalar::Date) {
                    wrap("(", " || ' 00:00:00')", value)
                } else if scalar_is(from, Scalar::Varchar) {
                    padded(date_time_text(value))
                } else {
                    value
                }
            }
            // a zoned date-time is held as its naive UTC text: its offset applied at the cast
            Scalar::Timestamptz => {
                if scalar_is(from, Scalar::Date) {
                    wrap("(", " || ' 00:00:00')", value)
                } else if scalar_is(from, Scalar::Varchar) {
                    let utc = utc_of(text("__u.t"));
                    let t = padded(date_time_text(value));
                    pc(move |w| {
                        w.append("(SELECT ");
                        utc(w)?;
                        w.append(" FROM (SELECT ");
                        t(w)?;
                        w.append(" AS t) AS __u)");
                        Ok(())
                    })
                } else {
                    value
                }
            }
            Scalar::Json => wrap("json_quote(", ")", value),
            // a time of day is its text: a date-time text's from its twelfth character, a date's midnight
            Scalar::Time => {
                if scalar_is(from, Scalar::Timestamp) || scalar_is(from, Scalar::Timestamptz) {
                    wrap("substr(", ", 12)", value)
                } else if scalar_is(from, Scalar::Date) {
                    text("'00:00:00'")
                } else {
                    value
                }
            }
            Scalar::TemporalText | Scalar::DecimalText => value,
        }
    }

    /// A value as text, as DuckDB's CAST to VARCHAR prints it (Java's `asText`).
    fn as_text<'a>(&'a self, e: Cow<'a, SqlExpr>) -> P<'a> {
        if matches!(e.as_ref(), SqlExpr::StringLit { .. }) {
            return self.ex_cow(e, 0);
        }
        // a decimal literal's text is its digits as written
        if let SqlExpr::DecimalLit { value, .. } = e.as_ref() {
            return text(self.lit(&big_decimal_plain(value.as_str())));
        }
        let t = match e.as_ref() {
            SqlExpr::FloatLit { .. } => Some(SqlType::Scalar(Scalar::Double)),
            SqlExpr::BoolLit { .. } => Some(SqlType::Scalar(Scalar::Boolean)),
            other => type_of(other).cloned(),
        };
        self.text_of(self.ex_cow(e, 0), t.as_ref())
    }

    /// DuckDB's text of a value of `type` (Java's `textOf`).
    fn text_of<'a>(&'a self, value: P<'a>, t: Option<&SqlType>) -> P<'a> {
        if scalar_is(t, Scalar::Double) {
            // the value bound once (its text is tried at three precisions)
            return pc(move |w| {
                w.append(&format!("(SELECT {SHORTEST} FROM (SELECT "));
                value(w)?;
                w.append(" AS v) AS __t)");
                Ok(())
            });
        }
        if scalar_is(t, Scalar::Boolean) {
            return pc(move |w| {
                w.append("(CASE WHEN ");
                value(w)?;
                w.append(" THEN 'true' WHEN NOT (");
                value(w)?;
                w.append(") THEN 'false' END)");
                Ok(())
            });
        }
        if let Some(SqlType::Decimal { scale, .. }) = t {
            if *scale > 0 {
                // DuckDB prints a DECIMAL with its scale's digits
                let head = format!("printf('%.{scale}f', ");
                return pc(move |w| {
                    w.append(&head);
                    value(w)?;
                    w.append(")");
                    Ok(())
                });
            }
            // a scale-0 decimal is an INTEGER or a REAL here: its text the value's own digits
            return pc(move |w| {
                w.append(&format!(
                    "(SELECT CASE WHEN typeof(__t.v) = 'integer' THEN CAST(__t.v AS TEXT) WHEN typeof(__t.v) = 'real' \
                     AND __t.v = round(__t.v) AND abs(__t.v) < 9.0e18 THEN CAST(CAST(__t.v AS INTEGER) AS TEXT) ELSE \
                     {SHORTEST} END FROM (SELECT "
                ));
                value(w)?;
                w.append(" AS v) AS __t)");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("CAST(");
            value(w)?;
            w.append(" AS TEXT)");
            Ok(())
        })
    }
}

/// `p.outputName()`: its alias, else a column's (or a stored read's) name.
fn output_name(p: &Projection) -> Option<String> {
    if let Some(alias) = &p.alias {
        return Some(alias.clone());
    }
    match &p.expr {
        SqlExpr::Column(c) => Some(c.name.clone()),
        SqlExpr::StoredRead { column, .. } => Some(column.name.clone()),
        _ => None,
    }
}

fn type_of_column(c: &Column) -> Option<&SqlType> {
    fact_type(&c.r#type)
}

/// `l.params().get(i)`, refused past the end as `ArrayList.get` words it.
fn param_at(l: &Lambda, i: usize) -> Result<&str, Refusal> {
    l.params.get(i).map(String::as_str).ok_or_else(|| Refusal {
        class: Refusal::INDEX_OUT_OF_BOUNDS,
        message: format!("Index {i} out of bounds for length {}", l.params.len()),
    })
}

/// A root select's value as its label delivers it, or none when it is delivered as it is (Java's `deliveredValue`).
fn delivered_value(e: &SqlExpr, label: &SqlType) -> Option<SqlExpr> {
    if matches!(e, SqlExpr::NullLit { .. } | SqlExpr::Star { .. } | SqlExpr::StarExcept { .. }) {
        return None;
    }
    if let SqlType::Decimal { scale, .. } = label {
        // printf reads a NULL as 0 and writes '0.00': a NULL stays NULL
        return Some(build::case(
            vec![build::when(
                build::call(SqlFn::IsNull, vec![e.clone()]),
                build::cast(build::null_lit(), SqlType::Scalar(Scalar::Double)),
            )],
            Some(build::call(SqlFn::Format, vec![build::string_lit(&format!("%.{scale}f")), e.clone()])),
        ));
    }
    if *label == SqlType::Scalar(Scalar::Double) {
        return Some(build::cast(e.clone(), SqlType::Scalar(Scalar::Double)));
    }
    // an integer past 64 bits has no SQLite value: its arithmetic overflows to a REAL silently, so a big integer's
    // column raises instead
    if *label == SqlType::Scalar(Scalar::Hugeint) {
        return Some(hugeint_raised(e));
    }
    None
}

/// A big integer's value, raised past 64 bits.
fn hugeint_raised(e: &SqlExpr) -> SqlExpr {
    build::case(
        vec![build::when(
            build::call(
                SqlFn::Or,
                vec![
                    build::call(SqlFn::Greater, vec![e.clone(), build::int_lit(i64::MAX)]),
                    build::call(SqlFn::Less, vec![e.clone(), build::int_lit(i64::MIN)]),
                ],
            ),
            build::call(SqlFn::Error, vec![build::string_lit("an integer past 64 bits has no SQLite value")]),
        )],
        Some(e.clone()),
    )
}

/// `s` with each star over a FROM holding a marker spelled out (Java's `starsSpelled`).
fn stars_spelled(s: &SqlSelect) -> Result<SqlSelect, Refusal> {
    let mut ps = Vec::new();
    if s.projections.is_empty() {
        spell(&s.from, None, &mut ps)?;
    }
    for p in &s.projections {
        match &p.expr {
            SqlExpr::Star { table, .. } => spell(&s.from, table.as_deref(), &mut ps)?,
            _ => ps.push(p.clone()),
        }
    }
    build::select(SqlSelect { projections: ps, ..s.clone() })
}

fn spell(src: &SqlSource, table: Option<&str>, into: &mut Vec<Projection>) -> Result<(), Refusal> {
    if let SqlSource::Join { left, right, .. } = src {
        if let Some((inner, alias)) = row_lateral(src) {
            spell(left, table, into)?;
            if table.is_none() || table == Some(alias) {
                for oc in query_outputs(inner) {
                    into.push(build::projection(
                        SqlExpr::Column(build::column_of(Some(alias), oc)),
                        Some(oc.name.clone()),
                        Some(oc.clone()),
                    )?);
                }
            }
        } else {
            spell(left, table, into)?;
            spell(right, table, into)?;
        }
        return Ok(());
    }
    if let SqlSource::Subselect { inner, alias, .. } = src
        && is_marker(inner)
        && (table.is_none() || table == Some(alias.as_str()))
    {
        let SqlQuery::Select(m) = inner.as_ref() else { unreachable!("a marker is a select") };
        for p in &m.projections {
            let slot = p.out.as_ref().map(|o| &o.r#type);
            into.push(build::projection(exploded_read(alias, p, slot)?, Some(marker_column(p)?), p.out.clone())?);
        }
        return Ok(());
    }
    if !matches!(src, SqlSource::Dual) {
        let alias = source_alias(src)?;
        if table.is_none() || table == Some(alias) {
            into.push(build::projection(build::star(Some(alias)), None, None)?);
        }
    }
    Ok(())
}

/// The lateral subquery a join reads as JSON rows (its inner query and alias), or none for any other join (Java's
/// `rowLateral`).
fn row_lateral(src: &SqlSource) -> Option<(&SqlQuery, &str)> {
    match src {
        SqlSource::Join { right, kind: JoinKind::CrossLateral | JoinKind::LeftLateral, .. } => match right.as_ref() {
            SqlSource::Subselect { inner, alias, .. } if !is_marker(inner) => Some((inner, alias)),
            _ => None,
        },
        _ => None,
    }
}

/// The lateral subqueries read as JSON rows, by alias, anywhere in a FROM (Java's `rowLateralsOf`; a repeated alias
/// replaced in place, as a `LinkedHashMap`).
fn row_laterals_of(src: &SqlSource, into: &mut RowLaterals) {
    if let SqlSource::Join { left, right, .. } = src {
        row_laterals_of(left, into);
        row_laterals_of(right, into);
        if let Some((inner, alias)) = row_lateral(src) {
            let outs = query_outputs(inner).to_vec();
            match into.iter_mut().find(|(a, _)| a == alias) {
                Some(slot) => slot.1 = outs,
                None => into.push((alias.to_owned(), outs)),
            }
        }
    }
}

/// The markers' exploded columns, by alias, anywhere in a FROM (Java's `lateralsOf`).
fn laterals_of(src: &SqlSource, into: &mut Markers) -> Result<(), Refusal> {
    match src {
        SqlSource::Join { left, right, .. } => {
            laterals_of(left, into)?;
            laterals_of(right, into)?;
        }
        SqlSource::Subselect { inner, alias, .. } => {
            if let SqlQuery::Select(m) = inner.as_ref()
                && is_marker_select(m)
            {
                let mut cols: Vec<(String, SqlExpr)> = Vec::new();
                for p in &m.projections {
                    let name = marker_column(p)?;
                    let list = unnest_list(&p.expr)?.clone();
                    match cols.iter_mut().find(|(n, _)| *n == name) {
                        Some(slot) => slot.1 = list,
                        None => cols.push((name, list)),
                    }
                }
                match into.iter_mut().find(|(a, _)| a == alias) {
                    Some(slot) => slot.1 = cols,
                    None => into.push((alias.clone(), cols)),
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// A date-time text as held: its 'T' a space (Java's `dateTimeText`).
fn date_time_text(t: P<'_>) -> P<'_> {
    pc(move |w| {
        w.append("replace(trim(");
        t(w)?;
        w.append("), 'T', ' ')");
        Ok(())
    })
}

/// A date-time text with an offset as its naive UTC text; one with none as it is (Java's `utcOf`).
fn utc_of(t: P<'_>) -> P<'_> {
    let t1 = t.clone();
    let shifted6 = shift(
        pc(move |w| {
            w.append("substr(");
            t1(w)?;
            w.append(", 1, length(");
            t1(w)?;
            w.append(") - 6)");
            Ok(())
        }),
        {
            let t = t.clone();
            pc(move |w| {
                w.append("substr(");
                t(w)?;
                w.append(", -6)");
                Ok(())
            })
        },
    );
    let t2 = t.clone();
    let t3 = t.clone();
    let shifted5 = shift(
        pc(move |w| {
            w.append("substr(");
            t2(w)?;
            w.append(", 1, length(");
            t2(w)?;
            w.append(") - 5)");
            Ok(())
        }),
        pc(move |w| {
            w.append("(substr(");
            t3(w)?;
            w.append(", -5, 3) || ':' || substr(");
            t3(w)?;
            w.append(", -2))");
            Ok(())
        }),
    );
    pc(move |w| {
        w.append("(CASE WHEN ");
        t(w)?;
        w.append(" GLOB '*Z' THEN substr(");
        t(w)?;
        w.append(", 1, length(");
        t(w)?;
        w.append(") - 1) WHEN ");
        t(w)?;
        w.append(" GLOB '*[+-][0-9][0-9]:[0-9][0-9]' THEN ");
        shifted6(w)?;
        w.append(" WHEN length(");
        t(w)?;
        w.append(") > 10 AND ");
        t(w)?;
        w.append(" GLOB '*[0-9][+-][0-9][0-9][0-9][0-9]' THEN ");
        shifted5(w)?;
        w.append(" ELSE ");
        t(w)?;
        w.append(" END)");
        Ok(())
    })
}

/// `body` (a naive date-time text) shifted back by `offset` ('+HH:MM') (Java's `shift`).
fn shift<'a>(body: P<'a>, offset: P<'a>) -> P<'a> {
    pc(move |w| {
        w.append("(datetime(substr(");
        body(w)?;
        w.append(", 1, 19), CASE substr(");
        offset(w)?;
        w.append(", 1, 1) WHEN '+' THEN '-' ELSE '+' END || substr(");
        offset(w)?;
        w.append(", 2)) || substr(");
        body(w)?;
        w.append(", 20))");
        Ok(())
    })
}

/// A date-time text with its date's fields padded, its time and fraction as written (Java's `padded`).
fn padded(text: P<'_>) -> P<'_> {
    let t = "__pd.t";
    let d = format!("CASE WHEN instr({t}, ' ') > 0 THEN substr({t}, 1, instr({t}, ' ') - 1) ELSE {t} END");
    let rest = format!("CASE WHEN instr({t}, ' ') > 0 THEN substr({t}, instr({t}, ' ')) ELSE '' END");
    let y = "substr(__pd2.d, 1, instr(__pd2.d, '-') - 1)";
    let md = "substr(__pd2.d, instr(__pd2.d, '-') + 1)";
    let head = format!(
        "(SELECT CASE WHEN __pd2.d GLOB '[0-9]*-[0-9]*-[0-9]*' THEN printf('%04d-%02d-%02d', {y}, substr({md}, 1, \
         instr({md}, '-') - 1), substr({md}, instr({md}, '-') + 1)) || __pd2.rest ELSE __pd2.t END FROM (SELECT {t} AS \
         t, {d} AS d, {rest} AS rest FROM (SELECT "
    );
    pc(move |w| {
        w.append(&head);
        text(w)?;
        w.append(" AS t) AS __pd) AS __pd2)");
        Ok(())
    })
}

// ==================================================================
// The renderer
// ==================================================================

impl AnsiSqlRenderer for Sqlite {
    fn lexicon(&self) -> &Lexicon {
        &LEXICON
    }

    fn ansi_state(&self) -> &AnsiState {
        &self.ansi
    }

    fn simple_name(&self) -> &'static str {
        "SQLite"
    }

    /// A statement's own fresh names start over at an outermost render; its labels and root selects are the
    /// statement's while it is written.
    fn render(&self, query: SqlQuery) -> Result<String, Refusal> {
        self.start_names();
        let _root = self.enter(query_outputs(&query).to_vec());
        ansi::render(self, query)
    }

    fn render_statement(&self, query: SqlQuery) -> Result<RenderedStatement, Refusal> {
        self.start_names();
        let _root = self.enter(query_outputs(&query).to_vec());
        ansi::render_statement(self, query)
    }

    /// A schema names no object of its own: its creation and its drop are no statement.
    fn render_ddl(&self, ddl: &SqlDdl) -> Result<String, Refusal> {
        Ok(match ddl {
            SqlDdl::CreateSchema { .. } | SqlDdl::DropSchema { .. } => "SELECT 1;".to_owned(),
            SqlDdl::CreateTable { schema, table, quoting, columns, temporary } => {
                let mut sb = String::from(if *temporary { "Create Temporary Table " } else { "Create Table " });
                sb.push_str(&self.qualified(schema.as_deref(), table, *quoting));
                sb.push('(');
                let mut keys = Vec::new();
                for (i, col) in columns.iter().enumerate() {
                    if i > 0 {
                        sb.push_str(", ");
                    }
                    sb.push_str(&self.ddl_identifier(&col.name, col.declared_quoted));
                    sb.push(' ');
                    sb.push_str(&self.ddl_type(&col.r#type)?);
                    sb.push_str(if col.primary_key || col.not_null { " NOT NULL" } else { " NULL" });
                    if col.primary_key {
                        keys.push(self.ddl_key_identifier(&col.name, col.declared_quoted));
                    }
                }
                if !keys.is_empty() {
                    sb.push_str(", PRIMARY KEY(");
                    sb.push_str(&keys.join(","));
                    sb.push(')');
                }
                sb.push_str(");");
                sb
            }
            SqlDdl::DropTable { schema, table, quoting } => {
                format!("Drop table if exists {};", self.qualified(schema.as_deref(), table, *quoting))
            }
        })
    }

    /// Rows in and out of a table, named as `tableName` names it; the store writes refused by the base until SQLite's
    /// spelling is measured.
    fn render_dml(&self, dml: &SqlDml) -> Result<String, Refusal> {
        match dml {
            SqlDml::InsertValues { schema, table, quoting, columns, quoted_columns, rows } => {
                let mut w = SqlWriter::new();
                let head = format!(
                    "INSERT INTO {}{} VALUES ",
                    self.qualified(schema.as_deref(), table, *quoting),
                    self.columns(columns, quoted_columns)
                );
                w.append(&head);
                for (r, row) in rows.iter().enumerate() {
                    w.append(if r == 0 { "(" } else { ", (" }).list(self, row)?.append(")");
                }
                w.append(";");
                let insert = w.into_statement()?;
                if !insert.binds.is_empty() {
                    return Err(Refusal::capability(format!(
                        "plan parameters {} in a DML row: a row holds values, and a DML statement is text",
                        crate::statement::list_text(&insert.binds)
                    )));
                }
                Ok(insert.sql)
            }
            SqlDml::InsertFromTable { schema, table, quoting, columns, quoted_columns, source } => Ok(format!(
                "INSERT INTO {}{} SELECT * FROM {};",
                self.qualified(schema.as_deref(), table, *quoting),
                self.columns(columns, quoted_columns),
                self.ident(source)
            )),
            SqlDml::DeleteAll { schema, table, quoting } => {
                Ok(format!("DELETE FROM {};", self.qualified(schema.as_deref(), table, *quoting)))
            }
            SqlDml::CreateTempTableAs { .. } => self.temp_table_as(dml)?.into_text(),
            SqlDml::InsertSelect { .. } | SqlDml::Update { .. } | SqlDml::DeleteWhere { .. } | SqlDml::Merge { .. } => {
                ansi::render_dml(self, dml)
            }
        }
    }

    /// The engine's H2 text is refused here: SQLite has no translation of it.
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        let _ = statement;
        Err(Refusal::capability("the engine's H2 text reached SQLite, which has no translation of it"))
    }

    /// The portable carrier strategies, star exclusions spelled out, the substring start clamp, whole-partition
    /// ordered sets as correlated subqueries, explosions placed as markers in FROM, QUALIFY as a wrapping subselect,
    /// a constant key as an expression, and a large repeated subexpression written once.
    fn passes(&self) -> Vec<Box<dyn SqlRewriter>> {
        vec![
            Box::new(crate::carrier::CarrierStrategies::new(CAPS)),
            Box::new(crate::shared_passes::StarExceptToColumns),
            Box::new(crate::passes::SubstringClamp),
            Box::new(crate::shared_passes::WholePartitionOrderedSets),
            Box::new(explosion_markers::LateralUnnestAsMarker),
            Box::new(explosion_markers::SelectListUnnestAsLateral::default()),
            Box::new(explosion_markers::StarOverExplosionUnwrapped),
            Box::new(crate::shared_passes::QualifyToSubselect::by_outputs()),
            Box::new(crate::passes::ConstantKeysAsExpressions::default()),
            Box::new(crate::common_subexpressions::CommonSubexpressions::new()),
        ]
    }

    /// A Float's literal spelling over the double's text bound ONCE.
    fn binds_float_text(&self) -> bool {
        true
    }

    /// A UNION branch takes no ORDER BY, LIMIT or parentheses here: such a branch, or a union, is `SELECT * FROM
    /// (branch)`. The first query written of a statement is its root: its selects are the root selects.
    fn query(&self, w: &mut SqlWriter, q: &SqlQuery, depth: usize) -> Result<(), Refusal> {
        {
            let mut top = self.root_selects.borrow_mut();
            if let Some(slot) = top.last_mut()
                && slot.is_none()
            {
                *slot = Some(selects_of(q));
            }
        }
        let SqlQuery::Union(u) = q else {
            return ansi::query(self, w, q, depth);
        };
        let op = if u.all { "UNION ALL" } else { "UNION" };
        for (i, b) in u.branches.iter().enumerate() {
            if i > 0 {
                self.nl(w, depth);
                w.append(op);
                self.nl(w, depth);
            }
            let wrap = match b {
                SqlQuery::Select(s) => !s.order_by.is_empty() || s.limit.is_some() || s.offset.is_some(),
                _ => true,
            };
            if wrap {
                w.append("SELECT * FROM (");
            }
            self.query(w, b, depth)?;
            if wrap {
                w.append(")");
            }
        }
        Ok(())
    }

    /// A select that only explodes lists is its reads over the marker's positions, in order; the markers of a FROM
    /// are in scope of its column reads, and a star over one is spelled out. A root select's values are delivered as
    /// their labels say.
    fn select(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        if is_marker_select(s) {
            let alias = format!("__u{depth}");
            let aligned = s.outputs.len() == s.projections.len();
            let mut ps = Vec::with_capacity(s.projections.len());
            for (i, p) in s.projections.iter().enumerate() {
                let slot = if aligned { Some(&s.outputs[i].r#type) } else { p.out.as_ref().map(|o| &o.r#type) };
                ps.push(build::projection(exploded_read(&alias, p, slot)?, Some(marker_column(p)?), p.out.clone())?);
            }
            let position = build::sort_key(build::derived(Some(&alias), "key"), true, None, None);
            let over = build::select(SqlSelect {
                projections: ps,
                distinct: false,
                from: build::subselect(build::query(s.clone()), &alias, None),
                r#where: None,
                group_by: Vec::new(),
                having: None,
                qualify: None,
                order_by: vec![position],
                limit: None,
                offset: None,
                outputs: Vec::new(),
            })?;
            // Java records `over` as a root select when `s` is one; its FROM holds a marker, so it is never delivered
            return self.select(w, &over, depth);
        }
        self.select_written(w, s, depth)
    }

    /// A marker's positions: `json_each` over its longest list, a lateral one joining its left side correlated; a
    /// lateral subquery read as JSON rows; any other source the base's.
    fn source(&self, w: &mut SqlWriter, src: &SqlSource, depth: usize) -> Result<(), Refusal> {
        if let SqlSource::Join { left, right, kind: kind @ (JoinKind::CrossLateral | JoinKind::LeftLateral), on, .. } =
            src
            && let SqlSource::Subselect { inner, alias, .. } = right.as_ref()
            && let SqlQuery::Select(m) = inner.as_ref()
            && is_marker_select(m)
        {
            if let Some(on) = on
                && !matches!(on.as_ref(), SqlExpr::BoolLit { value: true, .. })
            {
                return Err(Refusal::capability("a lateral explosion with a join condition reached SQLite"));
            }
            self.source(w, left, depth)?;
            self.nl(w, depth);
            let left_lateral = *kind == JoinKind::LeftLateral;
            w.append(if left_lateral { "LEFT JOIN " } else { "CROSS JOIN " });
            self.positions(m)?(w)?;
            let a = self.alias_ident(alias);
            w.append(" AS ").append(&a);
            if left_lateral {
                w.append(" ON TRUE");
            }
            return Ok(());
        }
        if let SqlSource::Subselect { inner, alias, .. } = src
            && let SqlQuery::Select(m) = inner.as_ref()
            && is_marker_select(m)
        {
            self.positions(m)?(w)?;
            let a = self.alias_ident(alias);
            w.append(" AS ").append(&a);
            return Ok(());
        }
        if let SqlSource::Join { left, kind, on, .. } = src
            && let Some((inner, alias)) = row_lateral(src)
        {
            let left_lateral = *kind == JoinKind::LeftLateral;
            let always = match on {
                None => true,
                Some(on) => matches!(on.as_ref(), SqlExpr::BoolLit { value: true, .. }),
            };
            self.source(w, left, depth)?;
            self.nl(w, depth);
            w.append(if left_lateral {
                "LEFT JOIN "
            } else if always {
                "CROSS JOIN "
            } else {
                "JOIN "
            });
            self.lateral_rows(inner, depth)?(w)?;
            let a = self.alias_ident(alias);
            w.append(" AS ").append(&a);
            if left_lateral || !always {
                w.append(" ON ");
                if always {
                    w.append("1");
                } else {
                    let on = on.as_ref().expect("a condition that is not always true");
                    w.expr(self, on, 0)?;
                }
            }
            return Ok(());
        }
        ansi::source(self, w, src, depth)
    }

    /// No VALUES alias list: the rows are named by a select over them (`column1`, ...); no rows is one row of NULLs,
    /// filtered.
    fn values_source(
        &self,
        w: &mut SqlWriter,
        rows: &[Vec<SqlExpr>],
        columns: &[String],
        alias: &str,
        _outputs: &[lite_sql::OutputCol],
    ) -> Result<(), Refusal> {
        w.append("(SELECT ");
        for (i, c) in columns.iter().enumerate() {
            let name = self.alias_ident(c);
            w.append(if i > 0 { ", " } else { "" });
            if rows.is_empty() {
                w.append("NULL");
            } else {
                w.append(&format!("column{}", i + 1));
            }
            w.append(" AS ").append(&name);
        }
        if rows.is_empty() {
            let a = self.alias_ident(alias);
            w.append(" WHERE 0) AS ").append(&a);
            return Ok(());
        }
        w.append(" FROM (VALUES ");
        for (r, row) in rows.iter().enumerate() {
            w.append(if r > 0 { ", " } else { "" }).append("(");
            w.join(row, ", ", |w, x| {
                w.expr(self, x, 0)?;
                Ok(())
            })?;
            w.append(")");
        }
        let a = self.alias_ident(alias);
        w.append(")) AS ").append(&a);
        Ok(())
    }

    /// A CTE is evaluated once when the plan says so (MATERIALIZED, SQLite 3.35).
    fn cte_as(&self, c: &Cte) -> String {
        if c.materialized { " AS MATERIALIZED (" } else { " AS (" }.to_owned()
    }

    fn pivot_source(&self, w: &mut SqlWriter, pivot: &SqlSource, depth: usize) -> Result<(), Refusal> {
        let _ = (w, pivot, depth);
        Err(Refusal::capability("a pivot reached SQLite unemulated: it has no PIVOT"))
    }

    fn as_of_join_clause(&self) -> Result<String, Refusal> {
        Err(Refusal::capability(
            "an as-of join reached SQLite, which has none (the carrier strategies emulate the witnessed shapes)",
        ))
    }

    fn star_except_keyword(&self) -> Result<String, Refusal> {
        Err(Refusal::capability("a star exclusion reached SQLite unexpanded (StarExceptToColumns)"))
    }

    /// Alias-less projections label EXPLICITLY from the declared output: SQLite labels an expression by its text.
    fn implicit_label(&self, p: &Projection) -> Result<Option<String>, Refusal> {
        match &p.out {
            Some(out) => Ok(Some(self.alias_ident(&out.name))),
            None => ansi::implicit_label(self, p),
        }
    }

    /// A NULL is NULL whatever its slot (a typed NULL has no SQLite type to carry).
    fn projection(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        if matches!(p.expr, SqlExpr::NullLit { .. }) {
            w.append("NULL");
            let label = match &p.alias {
                Some(alias) => Some(self.alias_ident(alias)),
                None => self.implicit_label(p)?,
            };
            if let Some(label) = label {
                w.append(" AS ").append(&label);
            }
            return Ok(());
        }
        ansi::projection(self, w, p)
    }

    fn expr(&self, w: &mut SqlWriter, e: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        match e {
            SqlExpr::DecimalLit { value, .. } => {
                w.append(&big_decimal_plain(value.as_str()));
                Ok(())
            }
            SqlExpr::ArrayLit { elements, .. } => self.array_lit(w, elements),
            SqlExpr::BoolLit { value, .. } => {
                w.append(if *value { "1" } else { "0" });
                Ok(())
            }
            // DuckDB's list(v ORDER BY k): ascending, NULLs last
            SqlExpr::OrderedListAgg { value, order_by, .. } => {
                w.append("json_group_array(");
                self.json(value)(w)?;
                w.append(" ORDER BY ").expr(self, order_by, 0)?.append(" NULLS LAST)");
                Ok(())
            }
            SqlExpr::Membership { collection, .. } if matches!(collection.as_ref(), SqlExpr::PlanParam { .. }) => {
                let SqlExpr::PlanParam { name, .. } = collection.as_ref() else { unreachable!("matched above") };
                Err(list_parameter(name))
            }
            SqlExpr::Quantified { value, comparison, quantifier, subquery, .. } => {
                self.quantified(value, *comparison, *quantifier, subquery)(w)
            }
            SqlExpr::Column(c) => {
                if let Some((list, position)) = self.exploded_column(c) {
                    return self.element(list, &position)(w);
                }
                if let Some((table, json)) = self.row_lateral_field(c) {
                    let a = self.alias_ident(&table);
                    let name = self.lit(&c.name);
                    w.append("(").append(&a).append(".value ").append(if json { "->" } else { "->>" });
                    w.append(" ").append(&name).append(")");
                    return Ok(());
                }
                ansi::expr(self, w, e, parent_prec)
            }
            _ => ansi::expr(self, w, e, parent_prec),
        }
    }

    // ---- lexical

    fn string_lit(&self, value: &str) -> Result<String, Refusal> {
        Ok(self.lit(value))
    }

    /// A date is its ISO text.
    fn date_lit(&self, iso: &str) -> String {
        format!("'{iso}'")
    }

    /// A date-time is its ISO text with a space, its fraction as written.
    fn timestamp_lit(&self, iso: &str) -> String {
        format!("'{}'", iso.replace('T', " "))
    }

    /// A time of day is its text.
    fn time_lit(&self, iso: &str) -> Result<String, Refusal> {
        Ok(format!("'{iso}'"))
    }

    /// A Float literal is a REAL; the infinities are REALs past the range; NaN has no SQLite value.
    fn float_literal(&self, v: f64) -> Result<String, Refusal> {
        if v.is_nan() {
            return Err(Refusal::capability("a NaN literal reached SQLite, which has no NaN (0.0 / 0 is NULL)"));
        }
        if v.is_infinite() {
            return Ok(if v > 0.0 { "9e999" } else { "-9e999" }.to_owned());
        }
        let s = crate::java::plain_float(v)?;
        Ok(if s.contains('.') || s.contains('E') || s.contains('e') { s } else { format!("{s}.0") })
    }

    fn array_type_name(&self, element: &str) -> String {
        let _ = element;
        "TEXT".to_owned()
    }

    fn map_type_name(&self, key: &str, value: &str) -> String {
        let _ = (key, value);
        "TEXT".to_owned()
    }

    fn scalar_type_name(&self, s: Scalar) -> Result<String, Refusal> {
        Ok(match s {
            Scalar::Boolean | Scalar::Integer | Scalar::Bigint | Scalar::Hugeint => "INTEGER",
            Scalar::Double => "REAL",
            Scalar::Varchar
            | Scalar::Date
            | Scalar::Timestamp
            | Scalar::Timestamptz
            | Scalar::Time
            | Scalar::Json
            | Scalar::TemporalText => "TEXT",
            Scalar::DecimalText => {
                return Err(Refusal::illegal_state(format!("{s} cast reached a dialect without {s} support")));
            }
        }
        .to_owned())
    }

    /// No STRUCT type here: a struct cast is refused.
    fn struct_type_name(&self, fields: &[StructField]) -> Result<String, Refusal> {
        let _ = fields;
        Err(Refusal::capability("a STRUCT type reached a dialect without struct support"))
    }

    /// A date format's parts: the base's strftime codes.
    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal> {
        Ok(ansi::strftime_text(parts))
    }

    // ---- JSON values

    fn array_lit(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("json_array(");
        w.join(elements, ", ", |w, x| self.json(x)(w))?;
        w.append(")");
        Ok(())
    }

    fn struct_lit(&self, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
        w.append("json_object(");
        w.join(fields, ", ", |w, f| {
            let name = self.lit(&f.name);
            w.append(&name).append(", ");
            self.json(&f.value)(w)
        })?;
        w.append(")");
        Ok(())
    }

    /// A field of a JSON object: a Variant field as its JSON (`->`), anything else as its value (`->>`).
    fn struct_get(&self, w: &mut SqlWriter, source: &SqlExpr, field: &str, r#type: &TypeFact) -> Result<(), Refusal> {
        let mut t = fact_type(r#type);
        if t.is_none()
            && let Some(SqlType::Struct { fields }) = type_of(source)
        {
            for f in fields {
                if f.name == field {
                    t = Some(&f.r#type);
                }
            }
        }
        let arrow = if scalar_is(t, Scalar::Json) { " -> " } else { " ->> " };
        w.append("(").expr(self, source, 9)?;
        let name = self.lit(field);
        w.append(arrow).append(&name).append(")");
        Ok(())
    }

    /// DuckDB's struct_insert: the struct with the field set (a new one last).
    fn struct_insert(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let SqlExpr::StringLit { value: name, .. } = arg(args, 1)? else {
            return Err(Refusal::capability("struct_insert of a field that is not a literal reached SQLite"));
        };
        w.append("json_set(").expr(self, arg(args, 0)?, 0)?.append(", ");
        let path = self.lit(&format!("$.\"{}\"", name.replace('"', "\\\"")));
        w.append(&path).append(", ");
        self.json(arg(args, 2)?)(w)?;
        w.append(")");
        Ok(())
    }

    /// exists([]) is false: Pure's empty-collection semantics.
    fn list_exists(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list_predicate(w, args, "max", "0", SqlFn::ListExists)
    }

    /// forAll([]) is true.
    fn list_for_all(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list_predicate(w, args, "min", "1", SqlFn::ListForAll)
    }

    /// No duplicates iff the distinct count is the count; an empty or NULL list is distinct.
    fn all_distinct(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("coalesce((SELECT count(*) = count(DISTINCT value) FROM json_each(").expr(self, arg(args, 0)?, 0)?;
        w.append(")), 1)");
        Ok(())
    }

    /// DuckDB's list_contains: NULL for a NULL list or needle; else found or not. A nested value compares as its
    /// JSON text.
    fn membership(&self, w: &mut SqlWriter, needle: &SqlExpr, collection: &SqlExpr) -> Result<(), Refusal> {
        if bindable(collection) || bindable(needle) {
            let operands = [needle.clone(), collection.clone()];
            let p = self.over(&operands, |r| {
                pc(move |w| {
                    let [n, c] = [&r[0], &r[1]];
                    self.membership(w, n, c)
                })
            });
            return p(w);
        }
        let el = list_element(collection);
        let e = self.param("__e")?;
        let nested_el = nested(el);
        w.append("(CASE WHEN ").expr(self, collection, 4)?.append(" IS NULL OR ").expr(self, needle, 4)?;
        w.append(" IS NULL THEN NULL ELSE EXISTS(SELECT 1 FROM ");
        self.elements(collection, "__e", "__eo", None)?(w)?;
        let read = if nested_el { format!("json({e})") } else { e };
        w.append(" WHERE ").append(&read).append(" = ");
        if nested_el {
            w.append("json(").expr(self, needle, 0)?.append(")");
        } else {
            w.expr(self, needle, 5)?;
        }
        w.append(") END)");
        Ok(())
    }

    fn reduce_collection(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        collection: &SqlExpr,
        extras: &[SqlExpr],
    ) -> Result<(), Refusal> {
        self.over_rows(collection, reducer, extras)?(w)
    }

    fn unnest_projection(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability(
            "an unnest in a select list reached SQLite, which has no set-returning projection (an explode is placed in \
             FROM as a json_each)",
        ))
    }

    /// Pure's fold, `(element, acc)`, as a correlated `WITH RECURSIVE` over the elements in order.
    fn fold_call(&self, w: &mut SqlWriter, fold: &SqlExpr) -> Result<(), Refusal> {
        let SqlExpr::FoldCall { source, lambda: l, init, .. } = fold else {
            return Err(Refusal::class_cast(crate::java::simple_name(fold), "FoldCall"));
        };
        let x = param_at(l, 0)?;
        let acc = param_at(l, 1)?;
        let body = lambda_body(l, list_element(source))?;
        let pacc = self.param(acc)?;
        let px = self.param(x)?;
        w.append("(WITH RECURSIVE __f(__i, ").append(&pacc).append(") AS (SELECT 0, ").expr(self, init, 0)?;
        w.append(" UNION ALL SELECT __f.__i + 1, ").expr(self, &body, 0)?;
        w.append(" FROM __f JOIN (SELECT key AS __k, ");
        w.append(&element_read(list_element(source), "")).append(" AS ");
        w.append(&px).append(" FROM json_each(").expr(self, source, 0)?;
        w.append(")) AS __x ON __x.__k = __f.__i) SELECT ").append(&pacc);
        w.append(" FROM __f ORDER BY __i DESC LIMIT 1)");
        Ok(())
    }

    /// Pure's splitPart: the non-empty tokens, 1-based, NULL past the end.
    fn split_part_call(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("(CASE WHEN ").expr(self, arg(a, 2)?, 4)?.append(" > 0 THEN (SELECT value FROM json_each(");
        self.split(arg(a, 0)?, arg(a, 1)?)(w)?;
        w.append(") WHERE value <> '' ORDER BY key LIMIT 1 OFFSET ").expr(self, arg(a, 2)?, 6)?;
        w.append(" - 1) END)");
        Ok(())
    }

    fn variant_construct(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() != 1 {
            return Err(Refusal::capability(format!("toVariant of {} arguments reached SQLite", a.len())));
        }
        self.variant_of(&a[0])(w)
    }

    /// A member by key, or a 0-based index (negative from the end; past either end NULL): `->`, JSON text.
    fn variant_get(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let key = arg(args, 1)?;
        if let SqlExpr::StringLit { value, .. } = key
            && value == "$"
        {
            w.expr(self, arg(args, 0)?, 0)?;
            return Ok(());
        }
        if is_text(key) {
            w.append("(").expr(self, arg(args, 0)?, 9)?.append(" -> ('$.\"' || replace(").expr(self, key, 0)?;
            w.append(", '\"', '\\\"') || '\"'))");
            return Ok(());
        }
        w.append("(").expr(self, arg(args, 0)?, 9)?.append(" -> (CASE WHEN ").expr(self, key, 4)?;
        w.append(" < 0 THEN '$[#' || ").expr(self, key, 6)?.append(" || ']' ELSE '$[' || ").expr(self, key, 6)?;
        w.append(" || ']' END))");
        Ok(())
    }

    /// A Variant array's elements, each a Variant: the array's JSON is the list (NULL for a non-array).
    fn variant_elements(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("(CASE WHEN json_type(").expr(self, arg(args, 0)?, 0)?.append(") = 'array' THEN ");
        w.expr(self, arg(args, 0)?, 0)?.append(" END)");
        Ok(())
    }

    fn variant_aware_cast(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        let _ = conform;
        if matches!(target, SqlType::Scalar(Scalar::TemporalText | Scalar::DecimalText)) {
            return w.expr(self, value, 0).map(|_| ());
        }
        if matches!(value, SqlExpr::NullLit { .. }) {
            w.append("NULL");
            return Ok(());
        }
        if *target == json_type() {
            if is_json(value) {
                w.expr(self, value, 0)?;
            } else if is_text(value) {
                w.append("json(").expr(self, value, 0)?.append(")");
            } else {
                self.variant_of(value)(w)?;
            }
            return Ok(());
        }
        if is_json(value) {
            return self.from_variant(value, target)?(w);
        }
        if let SqlType::Array { element } = target {
            let from = list_element(value);
            if from.is_none() || from == Some(element.as_ref()) {
                w.expr(self, value, 0)?;
                return Ok(());
            }
            // element-wise
            let c = self.param("__c")?;
            let each = json_of(self.cast_to(text(c), from, element), Some(element));
            return self.over_elements(value, "__c", "__co", None, each, None)?(w);
        }
        if matches!(target, SqlType::Struct { .. } | SqlType::Map { .. }) {
            w.expr(self, value, 0)?;
            return Ok(());
        }
        if *target == SqlType::Scalar(Scalar::Varchar) {
            // a big integer's text raises past 64 bits, as its root column does
            let v = if scalar_is(type_of(value), Scalar::Hugeint) {
                Cow::Owned(hugeint_raised(value))
            } else {
                Cow::Borrowed(value)
            };
            return self.as_text(v)(w);
        }
        self.cast_to(self.ex(value, 0), type_of(value), target)(w)
    }

    fn call(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact, parent_prec: i32) -> Result<(), Refusal> {
        if REPEATING.contains(&f) && a.iter().any(bindable) {
            let p = self.over(a, |refs| {
                pc(move |w| {
                    // Java's `(SqlExpr.Call) c.withChildren(refs)`: the call rebuilt, its fact recomputed
                    let SqlExpr::Call { args, r#type, .. } = build::call(f, refs.clone()) else {
                        unreachable!("a call")
                    };
                    self.calls(w, f, &args, &r#type, 0)
                })
            });
            return p(w);
        }
        self.calls(w, f, a, t, parent_prec)
    }

    fn json_kind_name(&self, kind: JsonKind) -> Result<String, Refusal> {
        Ok(match kind {
            JsonKind::Object => "OBJECT",
            JsonKind::Array => "ARRAY",
            JsonKind::Null => "NULL",
            JsonKind::String => "VARCHAR",
            JsonKind::Integer => "BIGINT",
            JsonKind::UnsignedInteger => "UBIGINT",
            JsonKind::Float => "DOUBLE",
            JsonKind::Boolean => "BOOLEAN",
        }
        .to_owned())
    }

    /// Refused, as TYPEOF is here (its type names are SQLite's storage classes).
    fn typeof_name(&self, scalar: Scalar) -> Result<String, Refusal> {
        let _ = scalar;
        Err(Refusal::capability("typeof reached SQLite: its type names are SQLite's storage classes"))
    }

    fn hash_signed(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.hash_signed_spelled(w, a)
    }

    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.round_half_even_spelled(w, a)
    }

    fn bit_op(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.bit_op_spelled(w, f, a)
    }

    fn reducer(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        self.reducer_spelled(w, r)
    }

    fn window_call(
        &self,
        w: &mut SqlWriter,
        f: &lite_sql::SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[lite_sql::SortKey],
        frame: Option<&lite_sql::Frame>,
    ) -> Result<(), Refusal> {
        self.window_call_spelled(w, f, partition_by, order_by, frame)
    }

    fn json_object(&self, w: &mut SqlWriter, kv: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("json_object(");
        for (i, x) in kv.iter().enumerate() {
            w.append(if i > 0 { ", " } else { "" });
            if i % 2 == 0 {
                w.expr(self, x, 0)?;
            } else {
                self.json(x)(w)?;
            }
        }
        w.append(")");
        Ok(())
    }

    fn json_array(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("json_array(");
        w.join(elements, ", ", |w, x| self.json(x)(w))?;
        w.append(")");
        Ok(())
    }

    /// A JSON array of the values, NULLs kept as JSON nulls; over no rows the empty array.
    fn json_array_agg(&self, w: &mut SqlWriter, value: &SqlExpr, order_keys: &[JsonArrayAggKey]) -> Result<(), Refusal> {
        w.append("json_group_array(");
        self.json(value)(w)?;
        if !order_keys.is_empty() {
            w.append(" ORDER BY ");
            w.join(order_keys, ", ", |w, k| {
                w.expr(self, &k.expr, 0)?.append(if k.desc { " DESC" } else { " ASC" }).append(" NULLS LAST");
                Ok(())
            })?;
        }
        w.append(")");
        Ok(())
    }

    /// A date or a date-time plan parameter is bound as its text.
    fn hole_type(&self, kind: ValueKind) -> Result<TypeSpelling, Refusal> {
        match kind {
            ValueKind::Date | ValueKind::DateTime | ValueKind::DateTimeNanos => {
                Ok(TypeSpelling::new("TEXT", Digits::None))
            }
            ValueKind::Integer | ValueKind::Decimal | ValueKind::Floating => ansi::hole_type(kind),
        }
    }

    /// A table of a schema is named `"schema.table"` in the session's one database.
    fn table_name(&self, name: &str, quoting: NameQuoting) -> String {
        match name.find('.') {
            Some(dot) if dot > 0 => self.delimited(name),
            _ => ansi::table_name(self, name, quoting),
        }
    }

    /// SQLite runs the standard spelling, its query a root as `render` writes one.
    fn temp_table_as(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        let SqlDml::CreateTempTableAs { query, .. } = dml else {
            return Err(Refusal::illegal_state("a temporary table from a query: not the statement this writer renders"));
        };
        let _root = self.enter(query_outputs(query).to_vec());
        ansi::create_temporary_table_as(self, dml, false, "CREATE TEMPORARY TABLE ", "")
    }

    fn ddl_type(&self, t: &ColumnType) -> Result<String, Refusal> {
        Ok(match t {
            ColumnType::Plain { kind } => match kind {
                ColumnKind::Bigint | ColumnKind::Smallint | ColumnKind::Tinyint | ColumnKind::Integer | ColumnKind::Bit => {
                    "INTEGER"
                }
                ColumnKind::Float | ColumnKind::Double | ColumnKind::Real => "REAL",
                ColumnKind::Timestamp
                | ColumnKind::Date
                | ColumnKind::Time
                | ColumnKind::TimestampNs
                | ColumnKind::Json
                | ColumnKind::Varchar
                | ColumnKind::Other
                | ColumnKind::Distinct
                | ColumnKind::Array
                | ColumnKind::Object => "TEXT",
            },
            ColumnType::Sized { kind, .. } => {
                if kind.to_uppercase().contains("BINARY") {
                    "BLOB"
                } else {
                    "TEXT"
                }
            }
            ColumnType::Scaled { .. } => "REAL",
        }
        .to_owned())
    }
}

impl Sqlite {
    /// `schema.table` as one delimited name, or the table's stored name in the default schema (Java's `qualified`).
    fn qualified(&self, schema: Option<&str>, table: &str, quoting: NameQuoting) -> String {
        match schema {
            None | Some("") | Some("default") => self.stored_name(table, quoting.table),
            Some(s) => self.delimited(&format!("{s}.{table}")),
        }
    }

    /// `" (a, b)"`, each column a `storedName`, or empty for every column (Java's `columns`).
    fn columns(&self, columns: &[String], quoted: &[String]) -> String {
        if columns.is_empty() {
            return String::new();
        }
        let names: Vec<String> = columns.iter().map(|c| self.stored_name(c, quoted.contains(c))).collect();
        format!(" ({})", names.join(", "))
    }

    /// `coalesce((SELECT agg(CASE WHEN body THEN 1 WHEN NOT (body) THEN 0 END) FROM elements), empty)` (Java's
    /// `listPredicate`).
    fn list_predicate(&self, w: &mut SqlWriter, args: &[SqlExpr], agg: &str, empty: &str, f: SqlFn) -> Result<(), Refusal> {
        let l = lambda_of(arg(args, 1)?, f)?;
        let xs = arg(args, 0)?;
        let body = lambda_body(l, list_element(xs))?;
        w.append(&format!("coalesce((SELECT {agg}(CASE WHEN ")).expr(self, &body, 0)?.append(" THEN 1 WHEN NOT (");
        w.expr(self, &body, 0)?.append(") THEN 0 END) FROM ");
        let index = l.params.get(1).map(String::as_str);
        self.elements(xs, param_at(l, 0)?, &Sqlite::offset_of(l)?, index)?(w)?;
        w.append(&format!("), {empty})"));
        Ok(())
    }

    /// DuckDB's string_split as a JSON array of the parts: a correlated recursive walk over the text (Java's
    /// `split`).
    fn split<'a>(&'a self, s: &'a SqlExpr, d: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, s, 4)?.append(" IS NULL OR ").expr(self, d, 4)?;
            w.append(" IS NULL THEN NULL WHEN ").expr(self, s, 4)?.append(" = '' THEN json_array() WHEN ").expr(self, d, 4)?;
            w.append(" = '' THEN (WITH RECURSIVE __s(__n, __p, __r) AS (SELECT 0, NULL, ").expr(self, s, 0)?;
            w.append(" UNION ALL SELECT __n + 1, substr(__r, 1, 1), substr(__r, 2) FROM __s WHERE __r <> '')");
            w.append(" SELECT json_group_array(__p ORDER BY __n) FROM __s WHERE __n > 0)");
            w.append(" ELSE (WITH RECURSIVE __s(__n, __p, __r) AS (SELECT 0, NULL, ").expr(self, s, 6)?.append(" || ");
            w.expr(self, d, 6)?.append(" UNION ALL SELECT __n + 1, substr(__r, 1, instr(__r, ").expr(self, d, 0)?;
            w.append(") - 1), substr(__r, instr(__r, ").expr(self, d, 0)?.append(") + length(").expr(self, d, 0)?;
            w.append(")) FROM __s WHERE __r <> '') SELECT json_group_array(__p ORDER BY __n) FROM __s");
            w.append(" WHERE __n > 0) END)");
            Ok(())
        })
    }
}

/// A lambda with parameters, refused by name otherwise (Java's `lambdaOf`).
fn lambda_of(e: &SqlExpr, f: SqlFn) -> Result<&Lambda, Refusal> {
    match e {
        SqlExpr::Lambda(l) if !l.params.is_empty() => Ok(l),
        _ => Err(Refusal::capability(format!("{f} without a lambda reached SQLite"))),
    }
}

/// A lambda's body over elements of `element` type: a field read off a struct parameter becomes a field read of the
/// JSON object, for every parameter (Java's `lambdaBody`).
fn lambda_body(l: &Lambda, element: Option<&SqlType>) -> Result<SqlExpr, Refusal> {
    let mut body = l.body.as_ref().clone();
    for (i, param) in l.params.iter().enumerate() {
        if let Some(b) = struct_fields(&body, param, if i == 0 { element } else { None })? {
            body = b;
        }
    }
    Ok(body)
}

/// `body` with each column qualified by `param` a field read of it (Java's `structFields`); none when none is.
fn struct_fields(body: &SqlExpr, param: &str, element: Option<&SqlType>) -> Result<Option<SqlExpr>, Refusal> {
    if let SqlExpr::Column(c) = body
        && c.table.as_deref() == Some(param)
    {
        let source = match element {
            Some(st @ SqlType::Struct { .. }) => build::typed_column(None, param, st.clone(), true, Origin::Derived),
            _ => build::derived(None, param),
        };
        return Ok(Some(build::struct_get_given(source, c.name.clone(), c.r#type.clone())));
    }
    crate::carrier::map_children(body, &mut |k| struct_fields(k, param, element))
}

#[cfg(test)]
mod tests;

// ==================================================================
// SQLite's `SqlDialect`: the renders as its renderer writes them, and what SQLite.java overrides of the interface
// ==================================================================

impl crate::dialect::SqlDialect for Sqlite {
    fn dialect_name(&self) -> &'static str {
        AnsiSqlRenderer::simple_name(self)
    }

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

    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        AnsiSqlRenderer::from_engine_text(self, statement)
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

    fn reads_stored(&self, t: &ColumnType) -> bool {
        <Sqlite as AnsiSqlRenderer>::reads_stored(t)
    }

    fn table_name_spelled(&self, name: &str, quoting: NameQuoting) -> String {
        AnsiSqlRenderer::table_name(self, name, quoting)
    }

    /// An embedded engine.
    fn in_process(&self) -> bool {
        true
    }

    /// LIKE is case-insensitive unless the session says otherwise.
    fn session_setup(&self) -> Vec<String> {
        vec!["PRAGMA case_sensitive_like = ON".to_owned()]
    }

    /// A read transaction is one snapshot.
    fn keys_snapshot(&self) -> Result<KeysSnapshot, Refusal> {
        Ok(KeysSnapshot::Transaction)
    }

    /// 2,000 columns (SQLITE_MAX_COLUMN).
    fn result_column_limit(&self) -> Result<Option<i32>, Refusal> {
        Ok(Some(2000))
    }

    /// No native pivot: a static pivot is emulated by the carrier strategies.
    fn needs_static_pivot(&self) -> bool {
        true
    }

    /// Its FLOAT is a REAL value, and NUMERIC its name for a NULL value.
    fn wire_type_name(&self, driver_type_name: Option<&str>) -> String {
        crate::wire_names::fold(driver_type_name.map(|n| match n {
            "FLOAT" => "DOUBLE",
            "NUMERIC" => "NULL",
            other => other,
        }))
    }

    /// Its own types: a NULL first value claims no type; a boolean and a big integer are an INTEGER; a date, a
    /// date-time, a list, a struct, a map, a Variant and a root decimal are their text.
    fn delivers_own(&self, label: &SqlType, wire: &str) -> bool {
        match wire {
            "NULL" => true,
            "INTEGER" => matches!(label, SqlType::Scalar(Scalar::Boolean | Scalar::Hugeint)),
            "VARCHAR" => {
                matches!(label, SqlType::Scalar(Scalar::Date | Scalar::Timestamp | Scalar::Timestamptz | Scalar::Json))
                    || matches!(
                        label,
                        SqlType::Array { .. } | SqlType::Struct { .. } | SqlType::Map { .. } | SqlType::Decimal { .. }
                    )
            }
            _ => false,
        }
    }
}
