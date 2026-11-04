// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `Snowflake` (`core/src/main/java/com/legend/sql/dialect/Snowflake.java`), the Snowflake execution dialect:
//! the base renderer with every identifier quoted (an unquoted one folds to upper case), a string literal's backslash
//! escaped, a decimal literal whose trailing zeros carry its scale spelled with its type, lists as semi-structured
//! `ARRAY`s of VARIANT elements cast where each is read, lambdas of one parameter that read no enclosing lambda's (a
//! captured name carried through a REDUCE's state, an index lambda emulated over the positions), an explosion a
//! lateral `FLATTEN` over its positions read at its `INDEX`, `LIMIT NULL OFFSET n`, constant-only VALUES, no raise
//! function (a sentinel-wrapped message cast to a type it cannot parse), a Float's text its shortest digits, and -- as
//! its [`crate::SqlDialect`] -- the session's one `ALTER SESSION`, no keys snapshot, no column limit, a static pivot,
//! its own wire names. Every `SqlFn` and aggregate is decided by an exhaustive match: `src/snowflake/functions.rs`
//! (`call` and its helpers: the lists, maps, variants, formats, regular expressions and temporal helpers),
//! `src/snowflake/texts.rs` (a Float's shortest digits and its three texts, Jaro-Winkler) and
//! `src/snowflake/aggregates.rs` (the hooks of the base, the aggregates and windows, the JSON constructors, the DDL).
//!
//! Java's per-render state -- `roots` (the labels of the statement being rendered, pushed by `render` and
//! `renderStatement`), `scopes` (the lambdas being written, each its parameters and the names it writes as pieces),
//! `laterals` (the explosion FLATTENs in scope of each select being written) and `fresh` (the `__f<n>`, `__s<n>`, ...
//! names, started over by the outermost render) -- is the dialect object's own: three [`RefCell`] stacks and a
//! [`Cell`], pushed, popped and reset where Java's are, so a fresh dialect per call is exact. A lambda scope's
//! substitutions are data ([`Sub`]) rather than Java's pieces: each is written, when a name it binds is read, in the
//! scopes of that read, as Java's piece is. Java's `SqlWriter.Piece` is [`P`], the shared closure of
//! [`crate::pieces`].

use crate::ansi::{self, cast_type_name};
use crate::dialect::KeysSnapshot;
use crate::explosion_markers::{self, exploded_read, is_marker, is_marker_select, marker_column, null_over_null_list};
use crate::pieces::{Ex, P, pc};
use crate::refusal::{Refusal, arg};
use crate::renderer::{AnsiSqlRenderer, AnsiState, Lexicon};
use crate::rewriter::{Rw, SqlRewriter};
use crate::statement::{Digits, RenderedStatement, TypeSpelling};
use crate::typing::{self, source_alias, unscaled};
use crate::writer::SqlWriter;
use crate::{build, carrier, passes, shared_passes};
use lite_sql::{
    AggFn, Bound, ColumnKind, ColumnType, DateFmt, Frame, JoinKind, JsonArrayAggKey, JsonKind, Lambda, Origin,
    OutputCol, Projection, Reducer, Scalar, SortKey, SqlAgg, SqlDdl, SqlDml, SqlExpr, SqlFn, SqlQuery, SqlSelect,
    SqlSource, SqlType, StructField, StructLitField, TypeFact, ValueKind, When,
};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::LazyLock;

mod aggregates;
mod functions;
mod texts;

/// The explosion FLATTENs of one select in scope: each FLATTEN's alias and its exploded columns' lists, in order
/// (Java's `Map<String, Map<String, SqlExpr>>`, a `LinkedHashMap` of them).
type Laterals = Vec<(String, Vec<(String, SqlExpr)>)>;

/// The Snowflake dialect: one per render, as Java's `new Snowflake()`.
#[derive(Debug, Default)]
pub struct Snowflake {
    ansi: AnsiState,
    /// Java's `roots`: the labels of each statement being rendered (its root's outputs before the passes).
    roots: RefCell<Vec<Vec<OutputCol>>>,
    /// Java's `scopes`: the lambdas being written, the innermost last.
    scopes: RefCell<Vec<Scope>>,
    /// Java's `laterals`: the FLATTENs in scope of each select being written, the innermost last.
    laterals: RefCell<Vec<Laterals>>,
    /// Java's `fresh`: the next fresh name's number.
    fresh: Cell<i64>,
}

impl Snowflake {
    /// Java's `new Snowflake()`.
    pub fn new() -> Snowflake {
        Snowflake::default()
    }
}

/// Snowflake's documented reserved keywords (Java's `LEXICON`'s). Snowflake quotes every identifier, so the list
/// decides no spelling.
pub const RESERVED: &[&str] = &[
    "account",
    "all",
    "alter",
    "and",
    "any",
    "as",
    "between",
    "by",
    "case",
    "cast",
    "check",
    "column",
    "connect",
    "connection",
    "constraint",
    "create",
    "cross",
    "current",
    "current_date",
    "current_time",
    "current_timestamp",
    "current_user",
    "database",
    "delete",
    "distinct",
    "drop",
    "else",
    "exists",
    "false",
    "following",
    "for",
    "from",
    "full",
    "grant",
    "group",
    "gscluster",
    "having",
    "ilike",
    "in",
    "increment",
    "inner",
    "insert",
    "intersect",
    "into",
    "is",
    "issue",
    "join",
    "lateral",
    "left",
    "like",
    "localtime",
    "localtimestamp",
    "minus",
    "natural",
    "not",
    "null",
    "of",
    "on",
    "or",
    "order",
    "organization",
    "qualify",
    "regexp",
    "revoke",
    "right",
    "rlike",
    "row",
    "rows",
    "sample",
    "schema",
    "select",
    "set",
    "some",
    "start",
    "table",
    "tablesample",
    "then",
    "to",
    "trigger",
    "true",
    "try_cast",
    "union",
    "unique",
    "update",
    "using",
    "values",
    "view",
    "when",
    "whenever",
    "where",
    "window",
    "with",
];

/// Java's `Snowflake.LEXICON`: `"` and [`RESERVED`].
static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| Lexicon::new('"', RESERVED));

/// Java's `Snowflake.CAPS`: semi-structured arrays with higher-order functions -- the strategies stand aside, as on
/// DuckDB and Databricks; FULL OUTER is native.
pub const CAPS: carrier::Caps =
    carrier::Caps { native_lists: true, correlated_explode: true, json_carrier: true, native_full_outer: true };

/// The session's one statement (Java's `sessionSetup`).
pub const SESSION_SETUP: &str = "ALTER SESSION SET TIMEZONE = 'UTC', WEEK_START = 1, WEEK_OF_YEAR_POLICY = 1, \
    TIMESTAMP_TYPE_MAPPING = 'TIMESTAMP_NTZ', QUOTED_IDENTIFIERS_IGNORE_CASE = FALSE, MULTI_STATEMENT_COUNT = 0, \
    JSON_INDENT = 0, STRICT_JSON_OUTPUT = TRUE";

// ==================================================================
// The type facts the arms read
// ==================================================================

/// Java's `typeOf(e)`.
fn type_of(e: &SqlExpr) -> Option<&SqlType> {
    fact_type(e.r#type())
}

fn fact_type(f: &TypeFact) -> Option<&SqlType> {
    match f {
        TypeFact::Typed { r#type, .. } => Some(r#type),
        _ => None,
    }
}

/// Java's `listElement(e)`: the element type of a list, none for a value that is no list.
fn list_element(e: &SqlExpr) -> Option<&SqlType> {
    match type_of(e) {
        Some(SqlType::Array { element }) => Some(element),
        _ => None,
    }
}

fn is_scalar(e: &SqlExpr, s: Scalar) -> bool {
    is_type(type_of(e), s)
}

fn is_type(t: Option<&SqlType>, s: Scalar) -> bool {
    matches!(t, Some(SqlType::Scalar(x)) if *x == s)
}

/// Java's `isJson(e)`.
fn is_json(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Json)
}

/// Java's `isText(e)`: a VARCHAR, or a string literal.
fn is_text(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Varchar) || matches!(e, SqlExpr::StringLit { .. })
}

/// Java's `isDouble(e)`: a DOUBLE.
fn is_double(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Double)
}

/// Java's `isInteger(t)`: INTEGER, BIGINT or HUGEINT.
fn is_integer(t: Option<&SqlType>) -> bool {
    matches!(t, Some(SqlType::Scalar(Scalar::Integer | Scalar::Bigint | Scalar::Hugeint)))
}

/// Java's `isTimestamp(e)`: a TIMESTAMP.
fn is_timestamp(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Timestamp)
}

/// A `SqlType`'s `toString`, or `null` for none (Java's string concatenation of a nullable type).
fn type_text(t: Option<&SqlType>) -> String {
    t.map_or_else(|| "null".to_owned(), crate::java::sql_type_text)
}

/// `x` as Java's `(SqlExpr.Lambda) x` reads it.
fn lambda_arg(x: &SqlExpr) -> Result<&Lambda, Refusal> {
    match x {
        SqlExpr::Lambda(l) => Ok(l),
        other => Err(Refusal::class_cast(crate::java::simple_name(other), "Lambda")),
    }
}

/// A lambda's `params().get(i)`, refused past the end as Java's list read words it.
fn param_at(l: &Lambda, i: usize) -> Result<&str, Refusal> {
    l.params.get(i).map(String::as_str).ok_or_else(|| Refusal {
        class: Refusal::INDEX_OUT_OF_BOUNDS,
        message: format!("Index {i} out of bounds for length {}", l.params.len()),
    })
}

/// A decimal's `scale() > 0 && stripTrailingZeros().scale() < scale()`, and its precision and scale (a decimal whose
/// trailing zeros carry its scale, which Snowflake drops from a literal's type).
fn trailing_scale(text: &str) -> Option<(i64, i64)> {
    let (_, digits, scale) = unscaled(text);
    let stripped = if digits == "0" { 0 } else { scale - (digits.len() - digits.trim_end_matches('0').len()) as i64 };
    (scale > 0 && stripped < scale).then_some((digits.len() as i64, scale))
}

/// `LinkedHashMap.put`: a key met again keeps its place, its value replaced.
fn put<V>(into: &mut Vec<(String, V)>, key: String, value: V) {
    match into.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => into.push((key, value)),
    }
}

// ==================================================================
// Lambda scopes
// ==================================================================

/// What a lambda writes for a name it binds in place of a parameter (Java's `SqlWriter.Piece` in a scope's `subs`):
/// fixed text, or an element of a list read at a position -- the list written where the name is read, in the scopes
/// of that read, as Java's piece is.
#[derive(Debug, Clone)]
enum Sub {
    Text(String),
    /// `GET(xs, at)`, cast to `cast` when the list's element type is known
    Element { xs: Rc<SqlExpr>, at: String, cast: Option<String> },
}

/// A lambda being written (Java's `Scope`): its declared parameters, and the names it writes as [`Sub`]s.
#[derive(Debug, Clone)]
struct Scope {
    params: Vec<String>,
    subs: Vec<(String, Sub)>,
}

/// What a name read inside lambdas resolves to (Java's `Resolved`).
enum Binding {
    None,
    Param,
    Sub(Sub),
}

/// A lambda parameter: its name and its declared type (none: a VARIANT).
#[derive(Debug, Clone)]
struct Param {
    name: String,
    r#type: Option<String>,
}

fn param(name: &str, t: Option<&str>) -> Param {
    Param { name: name.to_owned(), r#type: t.map(str::to_owned) }
}

/// An element lambda over a list (Java's `Ranged`): the array it ranges over, the lambda, and whether it ranges over
/// the list's positions (an emulated index lambda).
struct Ranged<'a> {
    over: P<'a>,
    lambda: P<'a>,
    positions: bool,
}

/// The positions of `xs`, 0-based (`NULL` for a NULL list): Java's `positions`.
fn positions<'a>(d: &'a Snowflake, xs: &'a SqlExpr) -> P<'a> {
    pc(move |w| {
        w.append("ARRAY_GENERATE_RANGE(0, ARRAY_SIZE(").expr(d, xs, 0)?.append("))");
        Ok(())
    })
}

/// `v` as a list element keeping a SQL NULL as JSON null (Java's `keepNull`).
fn keep_null<'a>(d: &'a Snowflake, v: &'a SqlExpr) -> P<'a> {
    pc(move |w| {
        w.append("COALESCE(TO_VARIANT(").expr(d, v, 0)?.append("), PARSE_JSON('null'))");
        Ok(())
    })
}

/// The names `e` reads that an enclosing lambda binds and `bound` does not (Java's `collectCaptured`).
fn collect_captured(e: &SqlExpr, bound: &HashSet<String>, scopes: &[Scope], out: &mut Vec<String>) {
    if let SqlExpr::Lambda(inner) = e {
        let mut b = bound.clone();
        b.extend(inner.params.iter().cloned());
        collect_captured(&inner.body, &b, scopes, out);
        return;
    }
    if let SqlExpr::Column(c) = e {
        let name = c.table.as_deref().unwrap_or(&c.name);
        if !bound.contains(name)
            && scopes.iter().any(|s| s.params.iter().any(|p| p == name) || s.subs.iter().any(|(k, _)| k == name))
            && !out.iter().any(|o| o == name)
        {
            out.push(name.to_owned());
        }
    }
    for k in typing::children(e) {
        collect_captured(&k, bound, scopes, out);
    }
}

/// The type a read of `name` in `e` declares (Java's `capturedType`; none: no read does, a VARIANT).
fn captured_type(e: &SqlExpr, name: &str) -> Option<SqlType> {
    if let SqlExpr::Column(c) = e
        && c.table.is_none()
        && c.name == name
        && let Some(t) = type_of(e)
    {
        return Some(t.clone());
    }
    for k in typing::children(e) {
        if let Some(t) = captured_type(&k, name) {
            return Some(t);
        }
    }
    None
}

impl Snowflake {
    /// Java's `freshName(stem)`: `__<stem><n>`, the counter advanced.
    fn fresh_name(&self, stem: &str) -> String {
        let n = self.fresh.get();
        self.fresh.set(n + 1);
        format!("__{stem}{n}")
    }

    /// The labels of the statement being rendered (none outside a render): Java's `rootLabels`.
    fn root_labels(&self) -> Vec<OutputCol> {
        self.roots.borrow().last().cloned().unwrap_or_default()
    }

    /// What `name` resolves to in the lambdas being written (Java's `resolve`): an enclosing lambda's name is refused.
    fn resolve(&self, name: &str) -> Result<Binding, Refusal> {
        let scopes = self.scopes.borrow();
        for (depth, s) in scopes.iter().rev().enumerate() {
            let sub = s.subs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
            if sub.is_some() || s.params.iter().any(|p| p == name) {
                if depth > 0 {
                    return Err(Refusal::capability(format!(
                        "a lambda reading the parameter '{name}' of an enclosing lambda reached Snowflake, whose \
                         lambdas cannot (measured: invalid identifier)"
                    )));
                }
                return Ok(match sub {
                    Some(v) => Binding::Sub(v),
                    None => Binding::Param,
                });
            }
        }
        Ok(Binding::None)
    }

    /// A substitution, written.
    fn write_sub(&self, w: &mut SqlWriter, sub: &Sub) -> Result<(), Refusal> {
        match sub {
            Sub::Text(t) => {
                w.append(t);
            }
            Sub::Element { xs, at, cast } => {
                if cast.is_some() {
                    w.append("CAST(");
                }
                w.append("GET(").expr(self, xs, 0)?.append(", ").append(at).append(")");
                if let Some(name) = cast {
                    w.append(" AS ").append(name).append(")");
                }
            }
        }
        Ok(())
    }

    /// The cast a VARIANT read as `t` takes (Java's `typed`'s name): none for no type or a VARIANT.
    fn cast_of(&self, t: Option<&SqlType>) -> Result<Option<String>, Refusal> {
        match t {
            None | Some(SqlType::Scalar(Scalar::Json)) => Ok(None),
            Some(t) => Ok(Some(cast_type_name(self, t)?)),
        }
    }

    /// A VARIANT read as type `t` (Java's `typed`): a scalar cast to it, a list to ARRAY, a struct or map to OBJECT; a
    /// variant (or an unknown type) as it is. The type's name is spelled when the piece is made.
    fn typed<'a>(&self, v: P<'a>, t: Option<&SqlType>) -> Result<P<'a>, Refusal> {
        Ok(match self.cast_of(t)? {
            None => v,
            Some(name) => pc(move |w| {
                w.append("CAST(").piece(&*v)?.append(" AS ").append(&name).append(")");
                Ok(())
            }),
        })
    }

    /// [`Snowflake::typed`] of fixed text.
    fn typed_text(&self, v: String, t: Option<&SqlType>) -> Result<String, Refusal> {
        Ok(match self.cast_of(t)? {
            None => v,
            Some(name) => format!("CAST({v} AS {name})"),
        })
    }

    /// The SQL type a lambda parameter over elements of `t` is declared as, or none to leave it a VARIANT (Java's
    /// `paramType`).
    fn param_type(&self, t: Option<&SqlType>) -> Result<Option<String>, Refusal> {
        t.map(|t| cast_type_name(self, t)).transpose()
    }

    /// `"p" T -> body` (or `("a" T, "b" U) -> body`), the body written inside the lambda's scope (Java's `lambdaOf`).
    fn lambda_of<'a>(&'a self, params: Vec<Param>, subs: Vec<(String, Sub)>, body: P<'a>) -> P<'a> {
        pc(move |w| {
            let decl: Vec<String> = params
                .iter()
                .map(|p| match &p.r#type {
                    None => self.delimited(&p.name),
                    Some(t) => format!("{} {t}", self.delimited(&p.name)),
                })
                .collect();
            let head = if decl.len() == 1 { decl[0].clone() } else { format!("({})", decl.join(", ")) };
            w.append(&head).append(" -> ");
            let mut names: Vec<String> = Vec::new();
            for p in &params {
                if !names.contains(&p.name) {
                    names.push(p.name.clone());
                }
            }
            self.scopes.borrow_mut().push(Scope { params: names, subs: subs.clone() });
            let written = body(w);
            self.scopes.borrow_mut().pop();
            written
        })
    }

    /// An element lambda over `xs` (Java's `ranged`): one IR parameter is a typed SQL parameter; with an index
    /// parameter (DuckDB's, 1-based) it ranges over the positions instead, reading the element and the index off the
    /// position.
    fn ranged<'a>(
        &'a self,
        xs: &'a SqlExpr,
        l: &'a Lambda,
        body: impl Fn(P<'a>) -> P<'a>,
    ) -> Result<Ranged<'a>, Refusal> {
        let element = list_element(xs);
        if l.params.len() == 1 {
            let t = self.param_type(element)?;
            let lambda = self.lambda_of(vec![param(&l.params[0], t.as_deref())], Vec::new(), body(self.ex(&l.body, 0)));
            return Ok(Ranged { over: self.ex(xs, 0), lambda, positions: false });
        }
        if l.params.len() != 2 {
            return Err(Refusal::capability(format!(
                "a lambda of {} parameters over a list reached Snowflake",
                l.params.len()
            )));
        }
        let i = self.fresh_name("i");
        let i_ref = self.delimited(&i);
        let mut subs = Vec::new();
        let cast = self.cast_of(element)?;
        put(&mut subs, l.params[0].clone(), Sub::Element { xs: Rc::new(xs.clone()), at: i_ref.clone(), cast });
        put(&mut subs, l.params[1].clone(), Sub::Text(format!("({i_ref} + 1)")));
        let lambda = self.lambda_of(vec![param(&i, Some("INTEGER"))], subs, body(self.ex(&l.body, 0)));
        Ok(Ranged { over: positions(self, xs), lambda, positions: true })
    }

    /// The names `l`'s body reads that an enclosing lambda binds (Java's `captured`): carried in by
    /// [`Snowflake::carried`].
    fn captured(&self, l: &Lambda) -> Vec<String> {
        let scopes = self.scopes.borrow();
        let mut out = Vec::new();
        if !scopes.is_empty() {
            let bound: HashSet<String> = l.params.iter().cloned().collect();
            collect_captured(&l.body, &bound, &scopes, &mut out);
        }
        out
    }

    /// `FILTER(xs, p -> body)` or `TRANSFORM(xs, p -> body)` whose body reads an enclosing lambda's names, as a REDUCE
    /// whose accumulator carries those values (Java's `carried`).
    fn carried<'a>(
        &'a self,
        xs: &'a SqlExpr,
        l: &'a Lambda,
        names: Vec<String>,
        filter: bool,
    ) -> Result<P<'a>, Refusal> {
        let state = self.delimited(&self.fresh_name("s"));
        let mut subs = Vec::new();
        for (k, c) in names.iter().enumerate() {
            let t = captured_type(&l.body, c);
            let read = format!("GET({state}, 'c{k}')");
            let v = match t {
                None => read,
                Some(t) => self.typed_text(read, Some(&t))?,
            };
            put(&mut subs, c.clone(), Sub::Text(v));
        }
        let element = param_at(l, 0)?.to_owned();
        let declared = self.param_type(list_element(xs))?;
        let kept = format!("CAST(GET({state}, 'r') AS ARRAY)");
        let next: P<'a> = if filter {
            let kept = kept.clone();
            let element = self.delimited(&element);
            pc(move |w| {
                w.append("IFF(").expr(self, &l.body, 0)?.append(", ARRAY_APPEND(").append(&kept);
                w.append(&format!(", {element}), ")).append(&kept).append(")");
                Ok(())
            })
        } else {
            let kept = kept.clone();
            pc(move |w| {
                w.append("ARRAY_APPEND(").append(&kept).append(", ").expr(self, &l.body, 0)?.append(")");
                Ok(())
            })
        };
        let count = names.len();
        let step_state = state.clone();
        let step = pc(move |w| {
            w.append("OBJECT_CONSTRUCT_KEEP_NULL('r', ").piece(&*next)?;
            for k in 0..count {
                w.append(&format!(", 'c{k}', GET({step_state}, 'c{k}')"));
            }
            w.append(")");
            Ok(())
        });
        let start = pc(move |w| {
            w.append("OBJECT_CONSTRUCT_KEEP_NULL('r', ARRAY_CONSTRUCT()");
            for (k, c) in names.iter().enumerate() {
                w.append(&format!(", 'c{k}', "));
                let read = build::derived(None, c);
                w.expr(self, &read, 0)?;
            }
            w.append(")");
            Ok(())
        });
        let lambda = self.lambda_of(
            vec![
                param(&state.replace('"', ""), Some("OBJECT")),
                param(&element, Some(declared.as_deref().unwrap_or("VARIANT"))),
            ],
            subs,
            step,
        );
        Ok(pc(move |w| {
            w.append("CAST(GET(REDUCE(").expr(self, xs, 0)?.append(", ").piece(&*start)?.append(", ");
            w.piece(&*lambda)?.append("), 'r') AS ARRAY)");
            Ok(())
        }))
    }

    /// A column (Java's `column`): a lambda parameter (or its emulated piece), a field of a struct parameter, a
    /// lateral FLATTEN's exploded element, or a plain reference.
    fn column(&self, w: &mut SqlWriter, c: &lite_sql::Column) -> Result<(), Refusal> {
        let Some(table) = &c.table else {
            let r = self.resolve(&c.name)?;
            if let Binding::Sub(sub) = &r {
                return self.write_sub(w, sub);
            }
            // an unqualified read of an exploded column: the FLATTEN that carries it, when exactly one of the
            // innermost select's does
            if matches!(r, Binding::None) {
                let owner = {
                    let laterals = self.laterals.borrow();
                    laterals.last().and_then(|innermost| {
                        let owners: Vec<&(String, Vec<(String, SqlExpr)>)> =
                            innermost.iter().filter(|(_, cols)| cols.iter().any(|(n, _)| *n == c.name)).collect();
                        (owners.len() == 1).then(|| {
                            let (alias, cols) = owners[0];
                            let list = cols.iter().find(|(n, _)| *n == c.name).map(|(_, l)| l.clone());
                            (alias.clone(), list)
                        })
                    })
                };
                if let Some((alias, list)) = owner {
                    let list = list.ok_or_else(|| Refusal::null_pointer("exploded column"))?;
                    return self.exploded_column(w, &alias, c, &list);
                }
            }
            let text = self.column_ref(c)?;
            w.append(&text);
            return Ok(());
        };
        let r = if self.scopes.borrow().is_empty() { Binding::None } else { self.resolve(table)? };
        if !matches!(r, Binding::None) {
            let param: P<'_> = match r {
                Binding::Sub(sub) => pc(move |w| self.write_sub(w, &sub)),
                _ => {
                    let table = table.clone();
                    pc(move |w| {
                        let derived = lite_sql::Column {
                            table: None,
                            name: table.clone(),
                            r#type: TypeFact::Unknown,
                            origin: Some(Origin::Derived),
                        };
                        let text = self.column_ref(&derived)?;
                        w.append(&text);
                        Ok(())
                    })
                }
            };
            let name = &c.name;
            let field = pc(move |w| {
                w.append("GET(").piece(&*param)?.append(", ");
                let lit = self.string_lit(name)?;
                w.append(&lit).append(")");
                Ok(())
            });
            let piece = match fact_type(&c.r#type) {
                None => pc(move |w| {
                    w.append("STRIP_NULL_VALUE(").piece(&*field)?.append(")");
                    Ok(())
                }),
                Some(t) => self.typed(field, Some(t))?,
            };
            w.piece(&*piece)?;
            return Ok(());
        }
        let found = {
            let laterals = self.laterals.borrow();
            laterals.iter().rev().find_map(|scope| {
                scope
                    .iter()
                    .find(|(alias, _)| alias == table)
                    .and_then(|(_, cols)| cols.iter().find(|(n, _)| *n == c.name))
                    .map(|(_, list)| list.clone())
            })
        };
        if let Some(list) = found {
            return self.exploded_column(w, table, c, &list);
        }
        let text = self.column_ref(c)?;
        w.append(&text);
        Ok(())
    }

    /// `c`, an exploded column of the FLATTEN `table`: its list's element at the FLATTEN's position (Java's
    /// `explodedColumn`).
    fn exploded_column(
        &self,
        w: &mut SqlWriter,
        table: &str,
        c: &lite_sql::Column,
        list: &SqlExpr,
    ) -> Result<(), Refusal> {
        let t = fact_type(&c.r#type).or_else(|| list_element(list));
        let at = format!("{}.\"INDEX\")", self.alias_ident(table));
        let get = pc(move |w| {
            w.append("GET(").expr(self, list, 0)?.append(", ").append(&at);
            Ok(())
        });
        let piece = self.typed(get, t)?;
        w.piece(&*piece)?;
        Ok(())
    }

    /// A marker's FLATTEN input: the positions of its longest list (Java's `markerPositions`).
    fn marker_positions<'a>(&'a self, m: &'a SqlSelect) -> Result<P<'a>, Refusal> {
        let lists =
            m.projections.iter().map(|p| explosion_markers::unnest_list(&p.expr)).collect::<Result<Vec<_>, _>>()?;
        Ok(pc(move |w| {
            w.append("ARRAY_GENERATE_RANGE(0, ");
            if lists.len() == 1 {
                w.append("ARRAY_SIZE(").expr(self, lists[0], 0)?.append(")");
            } else {
                w.append("GREATEST_IGNORE_NULLS(").join(&lists, ", ", |v, l| {
                    v.append("ARRAY_SIZE(").expr(self, l, 0)?.append(")");
                    Ok(())
                })?;
                w.append(")");
            }
            w.append(")");
            Ok(())
        }))
    }

    /// `*` over a FROM holding a marker's FLATTEN, spelled out (Java's `star`): each other source's columns, then each
    /// exploded column read off its position.
    fn star<'a>(&'a self, src: &'a SqlSource, into: &mut Vec<P<'a>>) -> Result<(), Refusal> {
        match src {
            SqlSource::Join { left, right, .. } => {
                self.star(left, into)?;
                self.star(right, into)
            }
            SqlSource::Subselect { inner, alias, .. } if is_marker(inner) => {
                let SqlQuery::Select(m) = inner.as_ref() else { unreachable!("a marker is a select") };
                for p in &m.projections {
                    let name = marker_column(p)?;
                    let read = exploded_read(alias, p, p.out.as_ref().map(|o| &o.r#type))?;
                    into.push(pc(move |w| {
                        let label = self.alias_ident(&name);
                        w.expr(self, &read, 0)?.append(" AS ").append(&label);
                        Ok(())
                    }));
                }
                Ok(())
            }
            SqlSource::Dual => Ok(()),
            other => {
                into.push(pc(move |w| {
                    let a = self.alias_ident(source_alias(other)?);
                    w.append(&a).append(".*");
                    Ok(())
                }));
                Ok(())
            }
        }
    }

    /// The select's body after its FLATTENs are in scope (Java's `select` past `laterals.push`).
    fn select_in_scope(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize, scope: &Laterals) -> Result<(), Refusal> {
        w.append("SELECT ");
        if s.distinct {
            w.append("DISTINCT ");
        }
        if s.projections.is_empty() && !scope.is_empty() {
            let mut star = Vec::new();
            self.star(&s.from, &mut star)?;
            w.join(&star, ", ", |w, p| p(w))?;
        } else if s.projections.is_empty() {
            w.append("*");
        } else {
            // a list's element, min or max over the NULL list is NULL: written as its slot's NULL
            let root = self.root_labels();
            let labels: &[OutputCol] =
                if depth == 0 && root.len() == s.projections.len() { &root } else { &s.outputs };
            let aligned = labels.len() == s.projections.len();
            ansi::projections_with(w, &s.projections, &|w, i| {
                let p = &s.projections[i];
                if let SqlExpr::Star { table, .. } = &p.expr
                    && !scope.is_empty()
                {
                    // a star beside other columns, over a FROM holding a marker's FLATTEN: spelled out as well
                    let mut star = Vec::new();
                    match table {
                        None => self.star(&s.from, &mut star)?,
                        Some(t) => self.star(source_named(&s.from, t)?, &mut star)?,
                    }
                    w.join(&star, ", ", |w, p| p(w))?;
                } else if aligned && null_over_null_list(&p.expr) {
                    let t = cast_type_name(self, &labels[i].r#type)?;
                    w.append(&format!("CAST(NULL AS {t})"));
                    let label = self.alias_ident(&marker_column(p)?);
                    w.append(" AS ").append(&label);
                } else {
                    self.projection(w, p)?;
                }
                Ok(())
            })?;
        }
        if !matches!(s.from, SqlSource::Dual) {
            self.nl(w, depth);
            w.append("FROM ");
            self.source(w, &s.from, depth)?;
        }
        if let Some(wh) = &s.r#where {
            self.nl(w, depth);
            w.append("WHERE ").expr(self, wh, 0)?;
        }
        if !s.group_by.is_empty() {
            self.nl(w, depth);
            w.append("GROUP BY ").list(self, &s.group_by)?;
        }
        if let Some(h) = &s.having {
            self.nl(w, depth);
            w.append("HAVING ").expr(self, h, 0)?;
        }
        if s.qualify.is_some() {
            self.append_qualify(w, s, depth)?;
        }
        if !s.order_by.is_empty() {
            self.nl(w, depth);
            w.append("ORDER BY ").join(&s.order_by, ", ", |w, k| self.sort_key(w, k))?;
        }
        if let Some(limit) = s.limit {
            self.nl(w, depth);
            w.append("LIMIT ").append_long(limit);
        } else if s.offset.is_some() {
            self.nl(w, depth);
            w.append("LIMIT NULL");
        }
        if let Some(offset) = s.offset {
            self.nl(w, depth);
            w.append("OFFSET ").append_long(offset);
        }
        Ok(())
    }
}

/// The source of a FROM bound to `alias` (Java's `sourceNamed`).
fn source_named<'a>(from: &'a SqlSource, alias: &str) -> Result<&'a SqlSource, Refusal> {
    source_named_or_none(from, alias)?.ok_or_else(|| {
        Refusal::capability(format!("a star over '{alias}', which the FROM does not bind, reached Snowflake"))
    })
}

fn source_named_or_none<'a>(from: &'a SqlSource, alias: &str) -> Result<Option<&'a SqlSource>, Refusal> {
    match from {
        SqlSource::Join { left, right, .. } => match source_named_or_none(left, alias)? {
            Some(l) => Ok(Some(l)),
            None => source_named_or_none(right, alias),
        },
        SqlSource::Dual => Ok(None),
        other => Ok((source_alias(other)? == alias).then_some(other)),
    }
}

/// The markers' exploded columns, by FLATTEN alias, anywhere in a FROM (Java's `lateralsOf`).
fn laterals_of(src: &SqlSource, into: &mut Laterals) -> Result<(), Refusal> {
    match src {
        SqlSource::Join { left, right, .. } => {
            laterals_of(left, into)?;
            laterals_of(right, into)
        }
        SqlSource::Subselect { inner, alias, .. } => {
            if let SqlQuery::Select(m) = inner.as_ref()
                && is_marker_select(m)
            {
                let mut cols: Vec<(String, SqlExpr)> = Vec::new();
                for p in &m.projections {
                    let name = marker_column(p)?;
                    let xs = explosion_markers::unnest_list(&p.expr)?.clone();
                    put(&mut cols, name, xs);
                }
                put(into, alias.clone(), cols);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// An as-of join's ON, split at its ANDs (Java's `conjuncts`): a TRUE literal is no conjunct.
fn conjuncts<'a>(e: Option<&'a SqlExpr>, into: &mut Vec<&'a SqlExpr>) {
    let Some(e) = e else {
        return;
    };
    if matches!(e, SqlExpr::BoolLit { value: true, .. }) {
        return;
    }
    if let SqlExpr::Call { r#fn: SqlFn::And, args, .. } = e {
        for x in args {
            conjuncts(Some(x), into);
        }
    } else {
        into.push(e);
    }
}

/// An inequality's comparison (Java's `inequality`).
fn inequality(e: &SqlExpr) -> bool {
    matches!(
        e,
        SqlExpr::Call { r#fn: SqlFn::Greater | SqlFn::GreaterEqual | SqlFn::Less | SqlFn::LessEqual, .. }
    )
}

/// A value VALUES takes as it is (Java's `constant`).
fn constant(e: &SqlExpr) -> bool {
    match e {
        SqlExpr::IntLit { .. } => true,
        SqlExpr::FloatLit { value, .. } => value.value().is_finite(),
        SqlExpr::DecimalLit { .. } => true,
        SqlExpr::StringLit { value, .. } => !value.contains('\0'),
        SqlExpr::BoolLit { .. }
        | SqlExpr::NullLit { .. }
        | SqlExpr::DateLit { .. }
        | SqlExpr::TimestampLit { .. }
        | SqlExpr::TimeLit { .. } => true,
        SqlExpr::Cast { value, target, .. } => {
            !matches!(
                target,
                SqlType::Array { .. } | SqlType::Map { .. } | SqlType::Struct { .. } | SqlType::Scalar(Scalar::Json)
            ) && constant(value)
        }
        _ => false,
    }
}

/// `iso` with its fraction cut to six digits (Java's `microseconds`): read by UTF-16 unit, a digit as
/// `Character.isDigit` takes it.
fn microseconds(iso: &str) -> String {
    let units: Vec<u16> = iso.encode_utf16().collect();
    let Some(dot) = units.iter().rposition(|&u| u == u16::from(b'.')) else {
        return iso.to_owned();
    };
    let digits = units[dot + 1..].iter().take_while(|&&u| lite_javacompat::character::is_digit(u)).count();
    if digits > 6 {
        let kept = String::from_utf16_lossy(&units[..dot + 7]);
        let rest = String::from_utf16_lossy(&units[dot + 1 + digits..]);
        format!("{kept}{rest}")
    } else {
        iso.to_owned()
    }
}

/// A quote doubled, a backslash escaped (Java's `quoted`).
fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}

// ==================================================================
// The pass Snowflake installs as its own class
// ==================================================================

/// Java's `Snowflake.PivotAsGroupBy`, the dialect's own copy of the shared pass (its nested class shadows the
/// package's `PivotAsGroupBy` in `passes()`): a static pivot as filtered aggregates, but a pivot whose aggregate takes
/// no argument left as it came (the writer refuses it by name) and an unnamed value refused in Snowflake's words.
#[derive(Debug, Default)]
pub struct PivotAsGroupBy;

/// Snowflake's own words for the shared pass's two refusals.
const PIVOT_WORDS: shared_passes::PivotWords =
    shared_passes::PivotWords { no_argument: None, unnamed_value: " reached Snowflake without a column name" };

impl SqlRewriter for PivotAsGroupBy {
    fn name(&self) -> &'static str {
        "Snowflake.PivotAsGroupBy"
    }

    fn source(&mut self, s: SqlSource) -> Result<Rw<SqlSource>, Refusal> {
        shared_passes::pivot_as_group_by(s, &PIVOT_WORDS)
    }
}

// ==================================================================
// The renderer (Java's overrides of AnsiSqlRenderer)
// ==================================================================

impl AnsiSqlRenderer for Snowflake {
    fn lexicon(&self) -> &Lexicon {
        &LEXICON
    }

    fn ansi_state(&self) -> &AnsiState {
        &self.ansi
    }

    fn simple_name(&self) -> &'static str {
        "Snowflake"
    }

    /// The statement's labels in scope while it is written; the outermost render starts the fresh names over.
    fn render(&self, query: SqlQuery) -> Result<String, Refusal> {
        if self.roots.borrow().is_empty() {
            self.fresh.set(0);
        }
        self.roots.borrow_mut().push(ansi::query_outputs(&query).to_vec());
        let rendered = ansi::render(self, query);
        self.roots.borrow_mut().pop();
        rendered
    }

    fn render_statement(&self, query: SqlQuery) -> Result<RenderedStatement, Refusal> {
        if self.roots.borrow().is_empty() {
            self.fresh.set(0);
        }
        self.roots.borrow_mut().push(ansi::query_outputs(&query).to_vec());
        let rendered = ansi::render_statement(self, query);
        self.roots.borrow_mut().pop();
        rendered
    }

    /// The engine's H2 text is refused here: Snowflake has no translation of it.
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        let _ = statement;
        Err(Refusal::capability("the engine's H2 text reached Snowflake, which has no translation of it"))
    }

    /// Spelled here ([`Snowflake::float_canon`]).
    fn spells_float_canon(&self) -> bool {
        true
    }

    /// Spelled here ([`Snowflake::float_print`]).
    fn spells_float_print(&self) -> bool {
        true
    }

    /// Native lists; the pivot as filtered aggregates (Snowflake's own copy); the checked-defect envelopes as list
    /// lambdas; the substring start clamp; a select-list UNNEST as a lateral FLATTEN; a constant GROUP BY or ORDER BY
    /// key as a typed expression; the root's DECIMAL(p,s) and HUGEINT columns cast to their declared types.
    fn passes(&self) -> Vec<Box<dyn SqlRewriter>> {
        vec![
            Box::new(carrier::CarrierStrategies::new(CAPS)),
            Box::new(PivotAsGroupBy),
            Box::new(passes::CheckedDefectsToLists),
            Box::new(passes::SubstringClamp),
            Box::new(explosion_markers::LateralUnnestAsMarker),
            Box::new(explosion_markers::SelectListUnnestAsLateral::default()),
            Box::new(explosion_markers::StarOverExplosionUnwrapped),
            Box::new(passes::ConstantKeysAsExpressions::default()),
            Box::new(shared_passes::RootNumericTypes),
        ]
    }

    fn supports_qualify(&self) -> bool {
        true
    }

    fn append_qualify(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        self.nl(w, depth);
        let q = s.qualify.as_ref().ok_or_else(|| Refusal::null_pointer("qualify"))?;
        w.append("QUALIFY ").expr(self, q, 0)?;
        Ok(())
    }

    fn row_order_column(&self) -> Result<String, Refusal> {
        Err(Refusal::capability("row order reached Snowflake, which has no row-order column"))
    }

    /// The as-of join is written whole by `source`.
    fn as_of_join_clause(&self) -> Result<String, Refusal> {
        Err(Refusal::illegal_state("an as-of join reached the base join writer: Snowflake's source() writes it"))
    }

    fn pivot_source(&self, w: &mut SqlWriter, pivot: &SqlSource, depth: usize) -> Result<(), Refusal> {
        let _ = (w, depth);
        let SqlSource::Pivot { on, r#in, .. } = pivot else { unreachable!("a pivot") };
        Err(Refusal::capability(format!(
            "a pivot reached the Snowflake writer: PivotAsGroupBy emulates a static one{}; this one has {} pivot \
             columns",
            if r#in.is_empty() { " (a dynamic pivot is discovered first: needsStaticPivot)" } else { "" },
            on.len()
        )))
    }

    /// OFFSET without LIMIT as `LIMIT NULL OFFSET n`; a select-list explosion left as a marker written over a FLATTEN
    /// of its positions; the lateral FLATTENs of its FROM in scope for its column reads.
    fn select(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        if is_marker_select(s) {
            let alias = self.fresh_name("u");
            // the root's slots are the plan's own labels, before this dialect's passes re-typed its expressions
            let root = self.root_labels();
            let labels: &[OutputCol] =
                if depth == 0 && root.len() == s.projections.len() { &root } else { &s.outputs };
            let aligned = labels.len() == s.projections.len();
            let mut ps = Vec::with_capacity(s.projections.len());
            for (i, p) in s.projections.iter().enumerate() {
                let slot = if aligned { Some(&labels[i].r#type) } else { p.out.as_ref().map(|o| &o.r#type) };
                let read = exploded_read(&alias, p, slot)?;
                ps.push(build::projection(read, Some(marker_column(p)?), p.out.clone())?);
            }
            let over = build::select(SqlSelect {
                projections: ps,
                distinct: false,
                from: build::subselect(build::query(s.clone()), &alias, None),
                r#where: None,
                group_by: Vec::new(),
                having: None,
                qualify: None,
                order_by: Vec::new(),
                limit: None,
                offset: None,
                outputs: Vec::new(),
            })?;
            return self.select(w, &over, depth);
        }
        let mut scope = Vec::new();
        laterals_of(&s.from, &mut scope)?;
        self.laterals.borrow_mut().push(scope.clone());
        let written = self.select_in_scope(w, s, depth, &scope);
        self.laterals.borrow_mut().pop();
        written
    }

    /// A marker's FLATTEN over its positions (a lateral one CROSS JOINed, a LEFT one with `OUTER => TRUE`); an as-of
    /// join's inequality as its MATCH_CONDITION.
    fn source(&self, w: &mut SqlWriter, src: &SqlSource, depth: usize) -> Result<(), Refusal> {
        if let SqlSource::Join { left, right, kind, on, .. } = src {
            if let SqlSource::Subselect { inner, alias, .. } = right.as_ref()
                && let SqlQuery::Select(m) = inner.as_ref()
                && is_marker_select(m)
                && matches!(kind, JoinKind::CrossLateral | JoinKind::LeftLateral)
            {
                if let Some(on) = on
                    && !matches!(on.as_ref(), SqlExpr::BoolLit { value: true, .. })
                {
                    return Err(Refusal::capability("a lateral explosion with a join condition reached Snowflake"));
                }
                self.source(w, left, depth)?;
                self.nl(w, depth);
                let positions = self.marker_positions(m)?;
                w.append("CROSS JOIN LATERAL FLATTEN(INPUT => ").piece(&*positions)?;
                let a = self.alias_ident(alias);
                w.append(if *kind == JoinKind::LeftLateral { ", OUTER => TRUE" } else { "" });
                w.append(") AS ").append(&a);
                return Ok(());
            }
            // DuckDB's ASOF LEFT JOIN ... ON eq AND t >= u: the one inequality is the MATCH_CONDITION, the
            // equalities ON
            if *kind == JoinKind::AsofLeft {
                let mut all = Vec::new();
                conjuncts(on.as_deref(), &mut all);
                let matched: Vec<&SqlExpr> = all.iter().copied().filter(|e| inequality(e)).collect();
                let rest: Vec<&SqlExpr> = all.iter().copied().filter(|e| !inequality(e)).collect();
                if matched.len() != 1 {
                    return Err(Refusal::capability(format!(
                        "an as-of join of {} inequalities reached Snowflake (its MATCH_CONDITION takes one)",
                        matched.len()
                    )));
                }
                self.source(w, left, depth)?;
                self.nl(w, depth);
                w.append("ASOF JOIN ");
                self.source(w, right, depth)?;
                w.append(" MATCH_CONDITION (").expr(self, matched[0], 0)?.append(")");
                if !rest.is_empty() {
                    w.append(" ON ").join(&rest, " AND ", |w, e| {
                        w.append("(").expr(self, e, 0)?.append(")");
                        Ok(())
                    })?;
                }
                return Ok(());
            }
        }
        if let SqlSource::Subselect { inner, alias, .. } = src
            && let SqlQuery::Select(m) = inner.as_ref()
            && is_marker_select(m)
        {
            let positions = self.marker_positions(m)?;
            w.append("TABLE(FLATTEN(INPUT => ").piece(&*positions)?;
            let a = self.alias_ident(alias);
            w.append(")) AS ").append(&a);
            return Ok(());
        }
        if let SqlSource::Join { kind: JoinKind::LeftLateral, .. } = src {
            return Err(Refusal::capability(
                "a LEFT JOIN LATERAL of a subquery reached Snowflake (only FLATTEN joins laterally, and without ON)",
            ));
        }
        ansi::source(self, w, src, depth)
    }

    /// VALUES takes constants only: rows holding any other expression are a UNION ALL of selects.
    fn values_source(
        &self,
        w: &mut SqlWriter,
        rows: &[Vec<SqlExpr>],
        columns: &[String],
        alias: &str,
        outputs: &[OutputCol],
    ) -> Result<(), Refusal> {
        let _ = outputs;
        let constants = rows.iter().all(|r| r.iter().all(constant));
        if constants && !rows.is_empty() {
            return ansi::values_source(self, w, rows, columns, alias);
        }
        if rows.is_empty() {
            return Err(Refusal::capability("an empty VALUES reached Snowflake"));
        }
        w.append("(");
        for (r, row) in rows.iter().enumerate() {
            if r > 0 {
                w.append(" UNION ALL ");
            }
            w.append("SELECT ");
            for (c, cell) in row.iter().enumerate() {
                if c > 0 {
                    w.append(", ");
                }
                w.expr(self, cell, 0)?;
                if r == 0 {
                    let name = columns.get(c).ok_or_else(|| Refusal {
                        class: Refusal::INDEX_OUT_OF_BOUNDS,
                        message: format!("Index {c} out of bounds for length {}", columns.len()),
                    })?;
                    let a = self.alias_ident(name);
                    w.append(" AS ").append(&a);
                }
            }
        }
        let a = self.alias_ident(alias);
        w.append(") AS ").append(&a);
        Ok(())
    }

    // ---- lexical: identifiers, literals, types

    /// Every identifier quoted; a quote-bearing declaration is already its own spelling.
    fn ident(&self, name: &str) -> String {
        if name.len() > 1
            && name.starts_with('"')
            && name.ends_with('"')
            && !name[1..name.len() - 1].replace("\"\"", "").contains('"')
        {
            return name.to_owned();
        }
        self.delimited(name)
    }

    /// A CTE's name quoted as every identifier is.
    fn cte_name(&self, name: &str) -> String {
        self.ident(name)
    }

    /// Alias-less projections label EXPLICITLY from the declared output.
    fn implicit_label(&self, p: &Projection) -> Result<Option<String>, Refusal> {
        match &p.out {
            Some(out) => Ok(Some(self.alias_ident(&out.name))),
            None => ansi::implicit_label(self, p),
        }
    }

    /// A quote doubled, a backslash escaped; NUL spliced as `chr(0)`.
    fn string_lit(&self, value: &str) -> Result<String, Refusal> {
        if value.contains('\0') {
            let parts: Vec<String> = value.split('\0').map(quoted).collect();
            return Ok(format!("({})", parts.join(" || chr(0) || ")));
        }
        Ok(quoted(value))
    }

    /// `TIMESTAMP '...'`, finer digits than a microsecond cut.
    fn timestamp_lit(&self, iso: &str) -> Result<String, Refusal> {
        Ok(format!("TIMESTAMP '{}'", microseconds(iso)))
    }

    /// A Float literal's plain spelling, its type spelled where its trailing zeros carry its scale; NaN and the
    /// infinities as FLOAT text.
    fn float_literal(&self, v: f64) -> Result<String, Refusal> {
        if v.is_nan() {
            return Ok("CAST('NaN' AS DOUBLE)".to_owned());
        }
        if v.is_infinite() {
            return Ok(if v > 0.0 { "CAST('inf' AS DOUBLE)" } else { "CAST('-inf' AS DOUBLE)" }.to_owned());
        }
        let plain = crate::java::plain_float(v)?;
        if let Some(digits) = crate::java::float_decimal(v)?
            && let Some((precision, scale)) = trailing_scale(&digits)
        {
            return Ok(format!("CAST({plain} AS NUMBER({},{scale}))", precision.max(scale)));
        }
        Ok(plain)
    }

    /// Lists are semi-structured ARRAYs whatever their element.
    fn array_type_name(&self, element: &str) -> Result<String, Refusal> {
        let _ = element;
        Ok("ARRAY".to_owned())
    }

    fn map_type_name(&self, key: &str, value: &str) -> String {
        let _ = (key, value);
        "OBJECT".to_owned()
    }

    /// An OBJECT, its fields spelled first (as Java's `structFields` is called).
    fn struct_type_name(&self, fields: &[StructField]) -> Result<String, Refusal> {
        ansi::struct_fields(self, fields)?;
        Ok("OBJECT".to_owned())
    }

    /// A date and a date-time placeholder take the NTZ timestamp; finer than a microsecond is cut to six digits.
    fn hole_type(&self, kind: ValueKind) -> Result<TypeSpelling, Refusal> {
        match kind {
            ValueKind::Date => Ok(TypeSpelling::new("DATE", Digits::None)),
            ValueKind::DateTime => Ok(TypeSpelling::new("TIMESTAMP_NTZ", Digits::None)),
            ValueKind::DateTimeNanos => {
                Ok(TypeSpelling { name: "TIMESTAMP_NTZ".to_owned(), digits: Digits::None, fraction: Some(6) })
            }
            ValueKind::Integer | ValueKind::Decimal | ValueKind::Floating => ansi::hole_type(kind),
        }
    }

    /// This database's name for each scalar type, a refusal where it has none.
    fn scalar_type_name(&self, s: Scalar) -> Result<String, Refusal> {
        Ok(match s {
            Scalar::Boolean => "BOOLEAN",
            Scalar::Integer => "INTEGER",
            Scalar::Bigint => "BIGINT",
            Scalar::Hugeint => "NUMBER(38,0)",
            Scalar::Double => "DOUBLE",
            Scalar::Varchar | Scalar::TemporalText => "VARCHAR",
            Scalar::Date => "DATE",
            Scalar::Timestamp => "TIMESTAMP_NTZ",
            Scalar::Timestamptz => "TIMESTAMP_LTZ",
            Scalar::Json => "VARIANT",
            Scalar::Time => "TIME",
            Scalar::DecimalText => {
                return Err(Refusal::illegal_state(format!("{s} cast reached a dialect without {s} support")));
            }
        }
        .to_owned())
    }

    fn ddl_type(&self, t: &ColumnType) -> Result<String, Refusal> {
        Ok(match t {
            ColumnType::Plain { kind } => match kind {
                ColumnKind::Bigint => "BIGINT",
                ColumnKind::Smallint => "SMALLINT",
                ColumnKind::Tinyint => "TINYINT",
                ColumnKind::Integer => "INTEGER",
                ColumnKind::Float => "FLOAT",
                ColumnKind::Double => "DOUBLE",
                ColumnKind::Real => "REAL",
                ColumnKind::Bit => "BOOLEAN",
                ColumnKind::Timestamp => "TIMESTAMP_NTZ",
                ColumnKind::Date => "DATE",
                ColumnKind::Time => "TIME",
                ColumnKind::TimestampNs => "TIMESTAMP_NTZ(9)",
                ColumnKind::Json => "VARIANT",
                ColumnKind::Varchar => "VARCHAR",
                ColumnKind::Other | ColumnKind::Distinct => {
                    return Err(Refusal::illegal_state(format!("no DDL spelling for declared column type {kind}")));
                }
                ColumnKind::Array => "ARRAY",
                ColumnKind::Object => "OBJECT",
            }
            .to_owned(),
            ColumnType::Sized { kind, size } => format!("{kind}({size})"),
            ColumnType::Scaled { kind, precision, scale } => format!("{kind}({precision}, {scale})"),
        })
    }

    // ---- expressions: lambda scopes, element reads, lateral reads

    fn expr(&self, w: &mut SqlWriter, e: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        match e {
            // a dynamic pivot's per-key column is the base's per-key name, its key written in by the runner
            SqlExpr::Column(c) if c.origin == Some(Origin::PerKey) => ansi::expr(self, w, e, parent_prec),
            SqlExpr::Column(c) => self.column(w, c),
            SqlExpr::OrderedListAgg { value, order_by, .. } => {
                w.append("ARRAY_AGG(").piece(&*keep_null(self, value))?.append(") WITHIN GROUP (ORDER BY ");
                w.expr(self, order_by, 0)?.append(")");
                Ok(())
            }
            SqlExpr::Membership { collection, .. } if matches!(collection.as_ref(), SqlExpr::PlanParam { .. }) => {
                let SqlExpr::PlanParam { name, .. } = collection.as_ref() else { unreachable!("a plan parameter") };
                Err(Refusal::capability(format!(
                    "list parameter '{name}' reached Snowflake, which binds no array (= ANY(?))"
                )))
            }
            // a decimal literal whose trailing zeros carry its scale spells its type
            SqlExpr::DecimalLit { value, r#type } if trailing_scale(value.as_str()).is_some() => {
                let (precision, scale) = trailing_scale(value.as_str()).expect("a trailing scale");
                let t = match r#type {
                    TypeFact::Typed { r#type: dd @ SqlType::Decimal { .. }, .. } => cast_type_name(self, dd)?,
                    _ => format!("NUMBER({},{scale})", precision.max(scale)),
                };
                w.append(&format!("CAST({} AS {t})", crate::java::big_decimal_plain(value.as_str())));
                Ok(())
            }
            // Java's `reduceCollection(writer, rc)` reads the node's own type: its arm takes the node
            SqlExpr::ReduceCollection { reducer, collection, extras, r#type } => {
                self.reduce_collection_arm(w, *reducer, collection, extras, r#type)
            }
            other => ansi::expr(self, w, other, parent_prec),
        }
    }

    /// An IR lambda written as it is: its parameters as VARIANTs.
    fn lambda(&self, w: &mut SqlWriter, l: &Lambda) -> Result<(), Refusal> {
        let params = l.params.iter().map(|p| param(p, None)).collect();
        w.piece(&*self.lambda_of(params, Vec::new(), self.ex(&l.body, 0)))?;
        Ok(())
    }

    // ---- lists: semi-structured ARRAYs, elements cast where read

    fn array_lit(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("ARRAY_CONSTRUCT(").list(self, elements)?.append(")");
        Ok(())
    }

    /// A pure collection holds no empties: ARRAY_COMPACT.
    fn compact_list(&self, w: &mut SqlWriter, list: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        let _ = parent_prec;
        w.append("ARRAY_COMPACT(").expr(self, list, 0)?.append(")");
        Ok(())
    }

    fn list_call(&self, w: &mut SqlWriter, f: SqlFn, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list(w, f, args, None)
    }

    /// exists([]) is false: the kept elements are some.
    fn list_exists(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let r = self.ranged(arg(args, 0)?, lambda_arg(arg(args, 1)?)?, |b| b)?;
        w.append("COALESCE(ARRAY_SIZE(FILTER(").piece(&*r.over)?.append(", ").piece(&*r.lambda)?;
        w.append(")) > 0, FALSE)");
        Ok(())
    }

    /// forAll([]) is true: no element is false.
    fn list_for_all(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let r = self.ranged(arg(args, 0)?, lambda_arg(arg(args, 1)?)?, |b| {
            pc(move |w| {
                w.append("NOT (").piece(&*b)?.append(")");
                Ok(())
            })
        })?;
        w.append("COALESCE(ARRAY_SIZE(FILTER(").piece(&*r.over)?.append(", ").piece(&*r.lambda)?;
        w.append(")) = 0, TRUE)");
        Ok(())
    }

    /// No duplicates iff the distinct non-NULL count is the count.
    fn all_distinct(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("COALESCE(ARRAY_SIZE(ARRAY_DISTINCT(ARRAY_COMPACT(").expr(self, arg(args, 0)?, 0)?;
        w.append("))) = ARRAY_SIZE(").expr(self, arg(args, 0)?, 0)?.append("), TRUE)");
        Ok(())
    }

    /// DuckDB's list_contains: NULL for a NULL needle or list.
    fn membership(&self, w: &mut SqlWriter, needle: &SqlExpr, collection: &SqlExpr) -> Result<(), Refusal> {
        w.append("(CASE WHEN ").expr(self, needle, 4)?.append(" IS NULL THEN NULL ELSE ARRAY_CONTAINS(TO_VARIANT(");
        w.expr(self, needle, 0)?.append("), ").expr(self, collection, 0)?.append(") END)");
        Ok(())
    }

    /// Reached only through the expression walk's own arm ([`Snowflake::expr`]), which passes the node's type; a
    /// direct call recomputes it as the node's constructor did.
    fn reduce_collection(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        collection: &SqlExpr,
        extras: &[SqlExpr],
    ) -> Result<(), Refusal> {
        let t = typing::reduce_collection_type(reducer, collection);
        self.reduce_collection_arm(w, reducer, collection, extras, &t)
    }

    /// Pure's fold as `REDUCE(xs, init, (acc T, x E) -> body)`.
    fn fold_call(&self, w: &mut SqlWriter, fold: &SqlExpr) -> Result<(), Refusal> {
        self.fold_call_arm(w, fold)
    }

    fn unnest_projection(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability(
            "an UNNEST Snowflake cannot place reached the writer: it explodes only as a lateral FLATTEN of a select's \
             FROM",
        ))
    }

    /// Pure's splitPart: the non-empty tokens, 1-based, NULL past the end.
    fn split_part_call(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.split_part_arm(w, a)
    }

    // ---- structs and maps: OBJECTs

    /// OBJECT_CONSTRUCT_KEEP_NULL: a NULL field is kept as JSON null.
    fn struct_lit(&self, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
        w.append("OBJECT_CONSTRUCT_KEEP_NULL(").join(fields, ", ", |w, f| {
            let name = self.string_lit(&f.name)?;
            w.append(&name).append(", ").expr(self, &f.value, 0)?;
            Ok(())
        })?;
        w.append(")");
        Ok(())
    }

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
        let Some(t) = t else {
            return Err(Refusal::capability(format!("a field '{field}' of unknown type reached Snowflake")));
        };
        let get = pc(move |w| {
            w.append("GET(").expr(self, source, 0)?.append(", ");
            let lit = self.string_lit(field)?;
            w.append(&lit).append(")");
            Ok(())
        });
        let piece = self.typed(get, Some(t))?;
        w.piece(&*piece)?;
        Ok(())
    }

    /// The struct with one field set (a NULL kept as JSON null).
    fn struct_insert(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("OBJECT_INSERT(").expr(self, arg(args, 0)?, 0)?.append(", ").expr(self, arg(args, 1)?, 0)?;
        w.append(", ").piece(&*keep_null(self, arg(args, 2)?))?.append(", TRUE)");
        Ok(())
    }

    // ---- variant: VARIANT

    fn variant_construct(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() != 1 {
            return Err(Refusal::capability(format!("toVariant of {} arguments reached Snowflake", a.len())));
        }
        w.append("TO_VARIANT(").expr(self, &a[0], 0)?.append(")");
        Ok(())
    }

    fn variant_get(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.variant_get_arm(w, args)
    }

    /// A variant array's elements (NULL when it is no array); a list already is one.
    fn variant_elements(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let a0 = arg(args, 0)?;
        if list_element(a0).is_some() {
            w.expr(self, a0, 0)?;
            return Ok(());
        }
        w.append("AS_ARRAY(TO_VARIANT(").expr(self, a0, 0)?.append("))");
        Ok(())
    }

    fn variant_aware_cast(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        self.variant_aware_cast_arm(w, value, target, conform)
    }

    // ---- scalar functions: every SqlFn decided in `functions.rs`

    fn call(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact, parent_prec: i32) -> Result<(), Refusal> {
        self.call_arm(w, f, a, t, parent_prec)
    }

    /// A projection that IS error() raised in its column's type, as a CASE branch is.
    fn projection_value(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        if let Some(out) = &p.out
            && let SqlExpr::Call { r#fn: SqlFn::Error, args, .. } = &p.expr
        {
            let raised = self.raise(args, Some(&out.r#type))?;
            w.piece(&*raised)?;
            return Ok(());
        }
        ansi::projection_value(self, w, p)
    }

    /// The base CASE, with a branch that IS error() raised in the CASE's own type.
    fn case_expr(
        &self,
        w: &mut SqlWriter,
        whens: &[When],
        otherwise: Option<&SqlExpr>,
        r#type: &TypeFact,
    ) -> Result<(), Refusal> {
        let slot = fact_type(r#type);
        w.append("CASE");
        for wh in whens {
            w.append(" WHEN ").expr(self, &wh.condition, 0)?.append(" THEN ");
            let b = self.branch(&wh.then, slot)?;
            w.piece(&*b)?;
        }
        if let Some(o) = otherwise {
            w.append(" ELSE ");
            let b = self.branch(o, slot)?;
            w.piece(&*b)?;
        }
        w.append(" END");
        Ok(())
    }

    /// The format of a date format written as a literal (TO_TIMESTAMP's parse format).
    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal> {
        functions::format_text(parts)
    }

    // ---- the hooks of the base, the aggregates and windows: `aggregates.rs`

    /// Pure's hashCode: HASH, a signed 64-bit integer.
    fn hash_signed(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("HASH(").list(self, a)?.append(")");
        Ok(())
    }

    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.round_half_even_arm(w, a)
    }

    fn bit_op(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        aggregates::bit_op(self, w, f, a)
    }

    fn reducer(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        self.reducer_arm(w, r)
    }

    fn window_call(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        self.window_call_arm(w, f, partition_by, order_by, frame)
    }

    /// Interval frame offsets: `INTERVAL '3 DAYS'`.
    fn bound(&self, b: &Bound) -> Result<String, Refusal> {
        Ok(match b {
            Bound::IntervalPreceding { n, unit } => format!("INTERVAL '{n} {unit}' PRECEDING"),
            Bound::IntervalFollowing { n, unit } => format!("INTERVAL '{n} {unit}' FOLLOWING"),
            other => ansi::bound(self, other),
        })
    }

    fn json_object(&self, w: &mut SqlWriter, kv: &[SqlExpr]) -> Result<(), Refusal> {
        w.function(self, "OBJECT_CONSTRUCT_KEEP_NULL", kv)?;
        Ok(())
    }

    /// A NULL element is JSON null.
    fn json_array(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("ARRAY_CONSTRUCT(").join(elements, ", ", |w, e| {
            w.piece(&*keep_null(self, e))?;
            Ok(())
        })?;
        w.append(")");
        Ok(())
    }

    /// ARRAY_AGG answers [] over no row; a NULL value is kept as JSON null.
    fn json_array_agg(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        order_keys: &[JsonArrayAggKey],
    ) -> Result<(), Refusal> {
        w.append("ARRAY_AGG(").piece(&*keep_null(self, value))?.append(")");
        if !order_keys.is_empty() {
            w.append(" WITHIN GROUP (ORDER BY ").join(order_keys, ", ", |w, k| {
                w.expr(self, &k.expr, 0)?.append(if k.desc { " DESC" } else { " ASC" }).append(" NULLS LAST");
                Ok(())
            })?;
            w.append(")");
        }
        Ok(())
    }

    // ---- values: what the driver hands back

    /// Its JSON_TYPE answers DuckDB's names (a DECODE over TYPEOF).
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

    /// A DATE is `DATE` (TYPEOF of its VARIANT); no other type's name is compared here.
    fn typeof_name(&self, scalar: Scalar) -> Result<String, Refusal> {
        if scalar == Scalar::Date {
            return Ok("DATE".to_owned());
        }
        Err(Refusal::capability(format!("a TYPEOF compared with {scalar}'s name reached Snowflake")))
    }
}

impl Snowflake {
    /// A CASE branch: error() raised in the CASE's own type, any other value as it is (Java's `branch`).
    fn branch<'a>(&'a self, value: &'a SqlExpr, slot: Option<&SqlType>) -> Result<P<'a>, Refusal> {
        match value {
            SqlExpr::Call { r#fn: SqlFn::Error, args, .. } => self.raise(args, slot),
            other => Ok(self.ex(other, 0)),
        }
    }
}

#[cfg(test)]
mod tests;

// ==================================================================
// SqlDialect
// ==================================================================

/// Java's `isNumber(driverTypeName)`: the driver's NUMBER, trimmed and in any case.
fn is_number(driver_type_name: Option<&str>) -> bool {
    driver_type_name.is_some_and(|n| crate::java::trim(n).eq_ignore_ascii_case("NUMBER"))
}

impl crate::dialect::SqlDialect for Snowflake {
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
        <Snowflake as AnsiSqlRenderer>::reads_stored(t)
    }

    fn table_name_spelled(&self, name: &str, quoting: lite_sql::NameQuoting) -> String {
        AnsiSqlRenderer::table_name(self, name, quoting)
    }

    /// One statement: the platform's naive-UTC contract, ISO weeks from Monday, a bare TIMESTAMP as NTZ, quoted
    /// identifiers keeping their case, a script in one execute, and list, object and variant cells as compact,
    /// standard JSON.
    fn session_setup(&self) -> Vec<String> {
        vec![SESSION_SETUP.to_owned()]
    }

    /// READ COMMITTED only, each statement its own snapshot.
    fn keys_snapshot(&self) -> Result<KeysSnapshot, Refusal> {
        Ok(KeysSnapshot::None)
    }

    /// No result column limit documented.
    fn result_column_limit(&self) -> Result<Option<i32>, Refusal> {
        Ok(None)
    }

    /// A dynamic pivot is discovered first: the static form is emulated (PivotAsGroupBy).
    fn needs_static_pivot(&self) -> bool {
        true
    }

    /// Its driver names every fixed-point number NUMBER.
    fn wire_type_name(&self, driver_type_name: Option<&str>) -> String {
        if is_number(driver_type_name) {
            return "DECIMAL".to_owned();
        }
        crate::wire_names::fold(driver_type_name)
    }

    /// NUMBER does not say whether it is an integer or a decimal: it types nothing.
    fn typing_wire_name(&self, driver_type_name: Option<&str>) -> Option<String> {
        if is_number(driver_type_name) {
            return None;
        }
        Some(self.wire_type_name(driver_type_name))
    }

    /// Its own types: an integer as NUMBER(p,0), a struct or a map as OBJECT.
    fn delivers_own(&self, label: &SqlType, wire: &str) -> bool {
        let integral = matches!(label, SqlType::Scalar(Scalar::Bigint | Scalar::Integer));
        integral && wire.starts_with("DECIMAL(") && wire.ends_with(",0)")
            || matches!(label, SqlType::Struct { .. } | SqlType::Map { .. }) && wire == "OBJECT"
    }
}
