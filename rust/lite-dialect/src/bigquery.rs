// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `BigQuery` (`core/src/main/java/com/legend/sql/dialect/BigQuery.java`), the Google BigQuery (GoogleSQL)
//! execution dialect: the base renderer with every identifier backticked (a backtick and a backslash escaped by a
//! backslash), a string literal's quote, backslash, control characters and `?` escaped, a Float or Decimal literal an
//! exact `BIGNUMERIC`, `UNION DISTINCT`, no OFFSET without LIMIT, no VALUES (an array of structs unnested), lambda-less
//! lists (each list operation a correlated subquery over `UNNEST(xs) WITH OFFSET`), a lateral explosion over its
//! positions and any other lateral derived table as an unnested array of structs, a map as its entries, a MODE counted
//! in a select over its source, and -- as its [`crate::SqlDialect`] -- no session statements, no keys snapshot, 10,000
//! result columns, a static pivot and its own wire names. Every `SqlFn` and aggregate is decided by an exhaustive
//! match: `src/bigquery/functions.rs` (`call` and its helpers, the maps, the regular expressions and the temporal
//! helpers), `src/bigquery/aggregates.rs` (the hooks of the base, the aggregates and windows, the JSON constructors,
//! the date formats, the DDL and the wire) and `src/bigquery/folds.rs` (`FoldsAsRecursiveCtes`, the pass only
//! BigQuery installs).
//!
//! Java's per-render state -- `laterals` (the explosion markers in scope of each select being written), `roots` (the
//! labels of the statement being rendered, pushed by `render` and `renderStatement`) and `modeCounts` (each select's
//! counted MODEs) -- is three [`RefCell`] stacks of the dialect object, pushed and popped where Java pushes and pops
//! them; nothing outlives a render. Java's `SqlWriter.Piece` is [`P`], a shared closure.

use crate::ansi::{self, cast_type_name};
use crate::dialect::KeysSnapshot;
use crate::explosion_markers::{
    self, exploded_read, is_marker, is_marker_select, marker_column, null_list, null_over_null_list, unnest_list,
};
use crate::refusal::{Refusal, arg};
use crate::renderer::{AnsiSqlRenderer, AnsiState, Lexicon};
use crate::rewriter::SqlRewriter;
use crate::statement::{Digits, RenderedStatement, TypeSpelling};
use crate::typing::{self, source_alias, unscaled};
use crate::writer::SqlWriter;
use crate::{build, carrier, common_subexpressions, passes, shared_passes};
use lite_sql::{
    AggFn, ColumnType, DateFmt, Frame, JoinKind, JsonArrayAggKey, JsonKind, Lambda, NameQuoting, Origin, OutputCol,
    Projection, Quantifier, Reducer, Scalar, SortKey, SqlAgg, SqlDdl, SqlDml, SqlExpr, SqlFn, SqlQuery, SqlSelect,
    SqlSource, SqlType, StructField, StructLitField, TypeFact, ValueKind, When,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::LazyLock;

mod aggregates;
pub(crate) mod folds;
mod functions;

/// The explosion markers of one select in scope: each marker's alias and its exploded columns' lists, in order (Java's
/// `Map<String, Map<String, SqlExpr>>`, a `LinkedHashMap` of them).
type Scope = Vec<(String, Vec<(String, SqlExpr)>)>;

/// The BigQuery dialect: one per render, as Java's `new BigQuery()`.
#[derive(Debug, Default)]
pub struct BigQuery {
    ansi: AnsiState,
    /// Java's `laterals`: the markers in scope of each select being written, the innermost last.
    laterals: RefCell<Vec<Scope>>,
    /// Java's `roots`: the labels of each statement being rendered (its root's outputs before the passes).
    roots: RefCell<Vec<Vec<OutputCol>>>,
    /// Java's `modeCounts`: per select being written, each MODE in it and the column counting its value's rows.
    mode_counts: RefCell<Vec<Vec<(SqlExpr, SqlExpr)>>>,
}

impl BigQuery {
    /// Java's `new BigQuery()`.
    pub fn new() -> BigQuery {
        BigQuery::default()
    }
}

/// GoogleSQL's reserved keywords (Java's `LEXICON`'s). BigQuery quotes every identifier, so the list decides no
/// spelling.
pub const RESERVED: &[&str] = &[
    "all",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "assert_rows_modified",
    "at",
    "between",
    "by",
    "case",
    "cast",
    "collate",
    "contains",
    "create",
    "cross",
    "cube",
    "current",
    "default",
    "define",
    "desc",
    "distinct",
    "else",
    "end",
    "enum",
    "escape",
    "except",
    "exclude",
    "exists",
    "extract",
    "false",
    "fetch",
    "following",
    "for",
    "from",
    "full",
    "group",
    "grouping",
    "groups",
    "hash",
    "having",
    "if",
    "ignore",
    "in",
    "inner",
    "intersect",
    "interval",
    "into",
    "is",
    "join",
    "lateral",
    "left",
    "like",
    "limit",
    "lookup",
    "merge",
    "natural",
    "new",
    "no",
    "not",
    "null",
    "nulls",
    "of",
    "on",
    "or",
    "order",
    "outer",
    "over",
    "partition",
    "preceding",
    "proto",
    "qualify",
    "range",
    "recursive",
    "respect",
    "right",
    "rollup",
    "rows",
    "select",
    "set",
    "some",
    "struct",
    "tablesample",
    "then",
    "to",
    "treat",
    "true",
    "unbounded",
    "union",
    "unnest",
    "using",
    "when",
    "where",
    "window",
    "with",
    "within",
];

/// Java's `BigQuery.LEXICON`: backticks and [`RESERVED`].
static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| Lexicon::new('`', RESERVED));

/// Java's `BigQuery.CAPS`: the portable strategies, as on Postgres: arrays native but lambda-less; FULL OUTER native.
pub const CAPS: carrier::Caps =
    carrier::Caps { native_lists: false, correlated_explode: true, json_carrier: true, native_full_outer: true };

// ==================================================================
// Pieces and the type facts the arms read
// ==================================================================

/// Java's `SqlWriter.Piece`: SQL that writes itself, shareable among the helpers that write it.
pub type P<'a> = Rc<dyn Fn(&mut SqlWriter) -> Result<(), Refusal> + 'a>;

fn pc<'a>(f: impl Fn(&mut SqlWriter) -> Result<(), Refusal> + 'a) -> P<'a> {
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
    fact_element(e.r#type())
}

fn fact_element(f: &TypeFact) -> Option<&SqlType> {
    match fact_type(f) {
        Some(SqlType::Array { element }) => Some(element),
        _ => None,
    }
}

fn is_scalar(e: &SqlExpr, s: Scalar) -> bool {
    matches!(type_of(e), Some(SqlType::Scalar(t)) if *t == s)
}

fn is_type(t: Option<&SqlType>, s: Scalar) -> bool {
    matches!(t, Some(SqlType::Scalar(x)) if *x == s)
}

/// Java's `isJson(e)`.
fn is_json(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Json)
}

/// Java's `isText(e)`: a string literal, or a VARCHAR.
fn is_text(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::StringLit { .. }) || is_scalar(e, Scalar::Varchar)
}

/// Java's `isDouble(e)`: a Float literal, or a DOUBLE.
fn is_double(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::FloatLit { .. }) || is_scalar(e, Scalar::Double)
}

/// Java's `bottom(xs)`: the bottom-typed empty list `[]` (compacted or not).
fn bottom(xs: &SqlExpr) -> bool {
    match xs {
        SqlExpr::CompactList { list, .. } => bottom(list),
        other => matches!(other.r#type(), TypeFact::Bottom),
    }
}

/// A `SqlType`'s `toString`, or `null` for none (Java's string concatenation of a nullable type).
fn type_text(t: Option<&SqlType>) -> String {
    t.map_or_else(|| "null".to_owned(), crate::java::sql_type_text)
}

/// A source's class's simple name (`src.getClass().getSimpleName()`).
fn source_class(src: &SqlSource) -> &'static str {
    match src {
        SqlSource::Table { .. } => "Table",
        SqlSource::Subselect { .. } => "Subselect",
        SqlSource::Cte { .. } => "Cte",
        SqlSource::Join { .. } => "Join",
        SqlSource::Pivot { .. } => "Pivot",
        SqlSource::Values { .. } => "Values",
        SqlSource::Dual => "Dual",
        SqlSource::SourceUrl { .. } => "SourceUrl",
        SqlSource::RawSql { .. } => "RawSql",
        SqlSource::Catalog { .. } => "Catalog",
        SqlSource::VarSetPlaceholder { .. } => "VarSetPlaceholder",
    }
}

/// The capability a list of lists meets (Java's `nestedList`).
fn nested_list() -> Refusal {
    Refusal::capability(
        "a list of lists reached BigQuery, whose arrays hold no array (measured: \"nested arrays are not supported\")",
    )
}

/// `new BigDecimal(text).stripTrailingZeros()`'s precision and scale (zero's are 1 and 0).
fn stripped(text: &str) -> (i64, i64) {
    let (_, digits, scale) = unscaled(text);
    if digits == "0" {
        return (1, 0);
    }
    let kept = digits.trim_end_matches('0');
    (kept.len() as i64, scale - (digits.len() - kept.len()) as i64)
}

/// Java's `fitsBigNumeric`: 38 digits each side of the point.
fn fits_big_numeric(text: &str) -> bool {
    let (precision, scale) = stripped(text);
    scale.max(0) <= 38 && precision - scale <= 38
}

/// A number BIGNUMERIC holds exactly (Java's `exactNumber`).
fn exact_number(e: &SqlExpr) -> Result<bool, Refusal> {
    Ok(match e {
        SqlExpr::IntLit { .. } | SqlExpr::NullLit { .. } => true,
        SqlExpr::DecimalLit { value, .. } => fits_big_numeric(value.as_str()),
        SqlExpr::FloatLit { value, .. } => {
            let v = value.value();
            v.is_finite() && crate::java::float_decimal(v)?.is_some_and(|p| fits_big_numeric(&p))
        }
        _ => false,
    })
}

/// A value NULL by its shape: a list's element, min or max over the NULL list, or a COALESCE of NULLs (Java's
/// `nullValued`).
fn null_valued(e: &SqlExpr) -> bool {
    null_over_null_list(e)
        || matches!(e, SqlExpr::Call { r#fn: SqlFn::Coalesce, args, .. }
            if args.iter().all(|x| matches!(x, SqlExpr::NullLit { .. })))
}

/// Java's `textual(e)`: a string literal, or a text type.
fn textual(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::StringLit { .. })
        || matches!(type_of(e), Some(SqlType::Scalar(Scalar::Varchar | Scalar::TemporalText | Scalar::DecimalText)))
}

/// The capability a list plan parameter meets (Java's `listParameter`).
fn list_parameter(name: &str) -> Refusal {
    Refusal::capability(format!(
        "list plan parameter '{name}' reached BigQuery: binding an array parameter through its driver is unmeasured"
    ))
}

/// Java's `SqlExpr.Column.param(name, collection)`.
fn column_param(name: &str, collection: &SqlExpr) -> SqlExpr {
    match list_element(collection) {
        Some(element) => build::typed_column(None, name, element.clone(), true, Origin::Derived),
        None => build::derived(None, name),
    }
}

/// `e` with each unqualified reference to `name` replaced by `with` (Java's `substitute`): `None` where nothing
/// changed.
fn substitute(e: &SqlExpr, name: &str, with: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
    if let SqlExpr::Column(c) = e
        && c.table.is_none()
        && c.name == name
    {
        return Ok(Some(with.clone()));
    }
    carrier::map_children(e, &mut |k| substitute(k, name, with))
}

fn substituted(e: SqlExpr, name: &str, with: &SqlExpr) -> Result<SqlExpr, Refusal> {
    Ok(substitute(&e, name, with)?.unwrap_or(e))
}

/// A lambda as Java's `lambdaOf(e, fn)` takes it: one with a parameter, else refused by name.
fn lambda_of(e: &SqlExpr, f: SqlFn) -> Result<&Lambda, Refusal> {
    match e {
        SqlExpr::Lambda(l) if !l.params.is_empty() => Ok(l),
        _ => Err(Refusal::capability(format!("{f} without a lambda reached BigQuery"))),
    }
}

/// The offset alias of a lambda's element (Java's `offsetOf`).
fn offset_of(l: &Lambda) -> String {
    format!("{}__o", l.params[0])
}

/// A lambda's body over the elements of `xs` (Java's `lambdaBody`).
fn lambda_body(xs: &SqlExpr, l: &Lambda) -> Result<SqlExpr, Refusal> {
    let body = l.body.as_ref().clone();
    // the empty list [] has no element to apply the body to: the body is a NULL of its own type
    if null_list(xs) && list_element(xs).is_none() && bottom(xs) {
        let Some(t) = type_of(&body) else {
            return Err(Refusal::capability("a lambda of no known type over the empty list reached BigQuery"));
        };
        return Ok(build::cast(build::null_lit(), t.clone()));
    }
    let mut body = body;
    if is_type(list_element(xs), Scalar::Json) {
        let json = build::typed_column(None, &l.params[0], SqlType::Scalar(Scalar::Json), true, Origin::Derived);
        body = substituted(body, &l.params[0], &json)?;
    }
    if l.params.len() < 2 {
        return Ok(body);
    }
    let index = build::call(SqlFn::Plus, vec![build::derived(None, &offset_of(l)), build::int_lit(1)]);
    substituted(body, &l.params[1], &index)
}

// ==================================================================
// The dialect's own helpers (Java's private methods)
// ==================================================================

impl BigQuery {
    /// `e`, written at `prec`, as a piece.
    fn ex<'a>(&'a self, e: &'a SqlExpr, prec: i32) -> P<'a> {
        pc(move |w| {
            w.expr(self, e, prec)?;
            Ok(())
        })
    }

    /// A lambda parameter (or an unnest's element or offset) as its references spell it (Java's `param`: a derived
    /// column's reference, its name by `ident`).
    fn param(&self, name: &str) -> String {
        self.ident(name)
    }

    /// A type's BigQuery spelling (Java's `bqType`).
    fn bq_type(&self, t: &SqlType) -> Result<String, Refusal> {
        Ok(match t {
            SqlType::Scalar(_) => cast_type_name(self, t)?,
            SqlType::Decimal { .. } => "BIGNUMERIC".to_owned(),
            SqlType::Array { element } => {
                if matches!(**element, SqlType::Array { .. }) {
                    return Err(nested_list());
                }
                format!("ARRAY<{}>", self.bq_type(element)?)
            }
            SqlType::Map { key, value } => self.map_type_name(&self.bq_type(key)?, &self.bq_type(value)?),
            SqlType::Struct { fields } => {
                let mut sb = String::from("STRUCT<");
                for (i, f) in fields.iter().enumerate() {
                    sb.push_str(if i > 0 { ", " } else { "" });
                    sb.push_str(&self.ident(&f.name));
                    sb.push(' ');
                    sb.push_str(&self.bq_type(&f.r#type)?);
                }
                sb.push('>');
                sb
            }
            SqlType::Tagged { .. } => {
                return Err(Refusal::illegal_state(
                    "a tagged value is never cast or stored: its tag and slots are its query's columns",
                ));
            }
        })
    }

    /// The type a list or a VALUES column of `t` holds `cells` as (Java's `heldType`).
    fn held_type(&self, t: &SqlType, cells: &[&SqlExpr]) -> Result<String, Refusal> {
        if matches!(t, SqlType::Scalar(Scalar::Double)) {
            let mut all = true;
            for c in cells {
                if !exact_number(c)? {
                    all = false;
                    break;
                }
            }
            if all {
                return Ok("BIGNUMERIC".to_owned());
            }
        }
        self.bq_type(t)
    }

    /// A NULL of `t`, or a bare one where the type is not known (Java's `nullOf`).
    fn null_of(&self, t: Option<&SqlType>) -> Result<String, Refusal> {
        Ok(match t {
            None => "NULL".to_owned(),
            Some(t) => format!("CAST(NULL AS {})", self.bq_type(t)?),
        })
    }

    /// An empty list of `e`'s element type, typed when the type is known (Java's `emptyList`).
    fn empty_list(&self, element: Option<&SqlType>) -> Result<String, Refusal> {
        Ok(match element {
            None => "[]".to_owned(),
            Some(el) => format!("ARRAY<{}>[]", self.bq_type(el)?),
        })
    }

    /// `decimalLiteral`: a decimal BIGNUMERIC holds, else refused.
    fn decimal_literal(&self, text: &str) -> Result<String, Refusal> {
        let plain = crate::java::big_decimal_plain(text);
        if !fits_big_numeric(text) {
            return Err(Refusal::capability(format!(
                "a decimal literal {plain} reached BigQuery, whose BIGNUMERIC holds 38 digits each side of the point"
            )));
        }
        Ok(format!("BIGNUMERIC '{plain}'"))
    }

    /// `UNNEST(xs) AS e WITH OFFSET AS o`, a NULL list typed (Java's `elements`): refused at once for a NULL list of
    /// unknown element type.
    fn elements<'a>(&'a self, xs: &'a SqlExpr, e: &str, o: &str) -> Result<P<'a>, Refusal> {
        let known = list_element(xs);
        if null_list(xs) && known.is_none() && !bottom(xs) {
            return Err(Refusal::capability("a NULL list of unknown element type reached BigQuery"));
        }
        let element = known.cloned().unwrap_or(SqlType::Scalar(Scalar::Varchar));
        let tail = format!(") AS {} WITH OFFSET AS {}", self.param(e), self.param(o));
        let null = null_list(xs);
        Ok(pc(move |w| {
            w.append("UNNEST(");
            if null {
                let t = cast_type_name(self, &SqlType::Array { element: Box::new(element.clone()) })?;
                w.append(&format!("CAST(NULL AS {t})"));
            } else {
                w.expr(self, xs, 0)?;
            }
            w.append(&tail);
            Ok(())
        }))
    }

    /// `body`, or NULL for a NULL list (Java's `unlessNull`).
    fn unless_null<'a>(&'a self, xs: &'a SqlExpr, body: P<'a>) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN ").expr(self, xs, 4)?.append(" IS NULL THEN NULL ELSE ").piece(&*body)?;
            w.append(" END)");
            Ok(())
        })
    }

    /// `ARRAY(SELECT select FROM UNNEST(xs) AS e WITH OFFSET AS o [WHERE where] ORDER BY o)`, NULL for a NULL list
    /// (Java's `overElements`; its unnest read when it is written).
    fn over_elements<'a>(
        &'a self,
        xs: &'a SqlExpr,
        e: String,
        o: String,
        select: P<'a>,
        r#where: Option<P<'a>>,
    ) -> P<'a> {
        let order = format!(" ORDER BY {})", self.param(&o));
        self.unless_null(
            xs,
            pc(move |w| {
                w.append("ARRAY(SELECT ").piece(&*select)?.append(" FROM ");
                let elements = self.elements(xs, &e, &o)?;
                w.piece(&*elements)?;
                if let Some(wh) = &r#where {
                    w.append(" WHERE ").piece(&**wh)?;
                }
                w.append(&order);
                Ok(())
            }),
        )
    }

    /// `(SELECT agg(e) FROM UNNEST(xs) AS e WITH OFFSET AS o)`: a list reduced as rows; MODE its most frequent
    /// non-NULL element, the first to occur among ties (Java's `overRows`).
    fn over_rows<'a>(&'a self, xs: &'a SqlExpr, f: AggFn, extras: &[SqlExpr]) -> Result<P<'a>, Refusal> {
        let e = self.param("__e");
        let o = self.param("__o");
        if f == AggFn::Mode {
            if !extras.is_empty() {
                return Err(Refusal::capability("a mode with arguments reached BigQuery"));
            }
            return Ok(pc(move |w| {
                w.append(&format!("(SELECT {e} FROM "));
                let elements = self.elements(xs, "__e", "__o")?;
                w.piece(&*elements)?;
                w.append(&format!(" WHERE {e} IS NOT NULL GROUP BY {e} ORDER BY COUNT(*) DESC, MIN({o}) LIMIT 1)"));
                Ok(())
            }));
        }
        let el = if list_element(xs).is_none() { build::derived(None, "__e") } else { column_param("__e", xs) };
        let mut args = vec![el];
        args.extend(extras.iter().cloned());
        let order = if matches!(f, AggFn::StringAgg | AggFn::List) {
            vec![build::sort_key(build::derived(None, "__o"), true, None, None)]
        } else {
            Vec::new()
        };
        let r = build::reducer(f, args, false, order);
        let dbl = f == AggFn::Avg;
        Ok(pc(move |w| {
            w.append(if dbl { "(SELECT CAST(" } else { "(SELECT " });
            self.reducer(w, &r)?;
            w.append(if dbl { " AS FLOAT64)" } else { "" }).append(" FROM ");
            let elements = self.elements(xs, "__e", "__o")?;
            w.piece(&*elements)?.append(")");
            Ok(())
        }))
    }

    /// A list literal, typed when its element type is known (Java's `arrayLiteral`).
    fn array_literal(&self, w: &mut SqlWriter, a: &SqlExpr) -> Result<(), Refusal> {
        let SqlExpr::ArrayLit { elements, .. } = a else { unreachable!("a list literal") };
        let el = list_element(a);
        for x in elements {
            if matches!(x, SqlExpr::ArrayLit { .. }) || matches!(type_of(x), Some(SqlType::Array { .. })) {
                return Err(nested_list());
            }
        }
        let Some(el) = el else {
            w.append("[").list(self, elements)?.append("]");
            return Ok(());
        };
        if matches!(el, SqlType::Array { .. }) {
            return Err(nested_list());
        }
        let cells: Vec<&SqlExpr> = elements.iter().collect();
        let held = self.held_type(el, &cells)?;
        w.append("ARRAY<").append(&held).append(">[").list(self, elements)?.append("]");
        Ok(())
    }

    /// `v cmp ANY|ALL (subquery)` counted in SQL's three-valued logic (Java's `quantified`).
    fn quantified(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        comparison: SqlFn,
        quantifier: Quantifier,
        subquery: &SqlQuery,
    ) -> Result<(), Refusal> {
        let cmp = build::call(comparison, vec![value.clone(), build::derived(None, "__qc")]);
        let any = quantifier == Quantifier::Any;
        w.append("(SELECT CASE WHEN ");
        if any {
            w.append("COUNTIF(").expr(self, &cmp, 0)?.append(") > 0 THEN TRUE WHEN COUNT(*) = 0 THEN FALSE");
        } else {
            w.append("COUNT(*) = 0 THEN TRUE WHEN COUNTIF(NOT (").expr(self, &cmp, 0)?.append(")) > 0 THEN FALSE");
        }
        w.append(" WHEN COUNTIF((").expr(self, &cmp, 0)?.append(") IS NULL) > 0 THEN NULL ELSE ");
        w.append(if any { "FALSE" } else { "TRUE" }).append(" END FROM UNNEST(ARRAY(");
        ansi::inline(self, w, subquery)?;
        w.append(")) AS ").append(&self.param("__qc")).append(")");
        Ok(())
    }

    /// An exploded column's read: its list and the marker whose position reads it (Java's `explodedColumn`), cloned
    /// out of the scopes so nothing is borrowed while it is written.
    fn exploded_column(&self, c: &lite_sql::Column) -> Option<(SqlExpr, String)> {
        let laterals = self.laterals.borrow();
        for scope in laterals.iter().rev() {
            match &c.table {
                Some(t) => {
                    if let Some((_, cols)) = scope.iter().find(|(alias, _)| alias == t)
                        && let Some((_, xs)) = cols.iter().find(|(name, _)| *name == c.name)
                    {
                        return Some((xs.clone(), t.clone()));
                    }
                }
                None => {
                    let owners: Vec<&(String, Vec<(String, SqlExpr)>)> =
                        scope.iter().filter(|(_, cols)| cols.iter().any(|(name, _)| *name == c.name)).collect();
                    if owners.len() == 1 {
                        let (alias, cols) = owners[0];
                        let xs = &cols.iter().find(|(name, _)| *name == c.name).expect("an owner").1;
                        return Some((xs.clone(), alias.clone()));
                    }
                }
            }
        }
        None
    }

    /// `xs[SAFE_OFFSET(position)]`; a NULL list's element is its type's NULL (Java's `element`).
    fn element(&self, w: &mut SqlWriter, c: &lite_sql::Column, xs: &SqlExpr, position: &str) -> Result<(), Refusal> {
        if null_list(xs) {
            match fact_type(&c.r#type) {
                None => {
                    w.append("NULL");
                }
                Some(t) => {
                    let name = cast_type_name(self, t)?;
                    w.append(&format!("CAST(NULL AS {name})"));
                }
            }
            return Ok(());
        }
        let a = self.alias_ident(position);
        w.expr(self, xs, 0)?.append("[SAFE_OFFSET(").append(&a).append(")]");
        Ok(())
    }

    /// A marker's positions: `UNNEST(GENERATE_ARRAY(0, n - 1))` over its longest list (Java's `positions`).
    fn positions(&self, w: &mut SqlWriter, m: &SqlSelect) -> Result<(), Refusal> {
        let mut lists = Vec::new();
        for p in &m.projections {
            let xs = unnest_list(&p.expr)?;
            if !null_list(xs) {
                lists.push(xs);
            }
        }
        w.append("UNNEST(GENERATE_ARRAY(0, ");
        match lists.len() {
            0 => {
                w.append("0");
            }
            1 => {
                w.append("IFNULL(ARRAY_LENGTH(").expr(self, lists[0], 0)?.append("), 0)");
            }
            _ => {
                w.append("GREATEST(").join(&lists, ", ", |v, l| {
                    v.append("IFNULL(ARRAY_LENGTH(").expr(self, l, 0)?.append("), 0)");
                    Ok(())
                })?;
                w.append(")");
            }
        }
        w.append(" - 1))");
        Ok(())
    }

    /// A lateral derived table as an unnested array of structs, one per row (Java's `lateral`).
    fn lateral(
        &self,
        w: &mut SqlWriter,
        left: &SqlSource,
        right: &SqlSource,
        kind: JoinKind,
        on: Option<&SqlExpr>,
        depth: usize,
    ) -> Result<(), Refusal> {
        let SqlSource::Subselect { inner, alias, .. } = right else {
            return Err(Refusal::capability(format!("a lateral join over a {} reached BigQuery", source_class(right))));
        };
        let trivial = on.is_none_or(|on| matches!(on, SqlExpr::BoolLit { value: true, .. }));
        let is_left = kind == JoinKind::LeftLateral;
        self.source(w, left, depth)?;
        self.nl(w, depth);
        w.append(if is_left {
            "LEFT JOIN "
        } else if trivial {
            "CROSS JOIN "
        } else {
            "JOIN "
        })
        .append("UNNEST(ARRAY(SELECT AS STRUCT * FROM (");
        self.nl(w, depth + 1);
        self.query(w, inner, depth + 1)?;
        self.nl(w, depth);
        let a = self.alias_ident(alias);
        w.append("))) AS ").append(&a);
        if is_left || !trivial {
            w.append(" ON ");
            if trivial {
                w.append("TRUE");
            } else {
                w.expr(self, on.ok_or_else(|| Refusal::null_pointer("on"))?, 0)?;
            }
        }
        Ok(())
    }

    /// `s`, its FROM given a count per MODE (Java's `modesCounted`).
    fn modes_counted(&self, s: &SqlSelect, counts: &mut Vec<(SqlExpr, SqlExpr)>) -> Result<SqlSelect, Refusal> {
        let mut modes: Vec<(SqlExpr, Vec<SqlExpr>)> = Vec::new();
        let mut roots: Vec<&SqlExpr> = s.projections.iter().map(|p| &p.expr).collect();
        roots.extend(s.having.as_ref());
        roots.extend(s.qualify.as_ref());
        for e in roots {
            collect_modes(e, &s.group_by, &mut modes)?;
        }
        if modes.is_empty() {
            return Ok(s.clone());
        }
        let from = &s.from;
        let alias = source_alias(from)?.to_owned();
        let outputs = typing::source_outputs(from);
        if matches!(from, SqlSource::Join { .. }) || outputs.is_empty() {
            return Err(Refusal::capability(format!(
                "a MODE over a {} reached BigQuery: its counts are a select over one aliased source",
                source_class(from)
            )));
        }
        let mut ps = Vec::new();
        for oc in outputs {
            let read = SqlExpr::Column(build::column_of(Some(&alias), oc));
            ps.push(build::projection(read, Some(oc.name.clone()), Some(oc.clone()))?);
        }
        for (k, (mode, keys)) in modes.into_iter().enumerate() {
            let name = format!("__mc{k}");
            let v = mode_value(&mode)?.clone();
            let mut partition: Vec<SqlExpr> = keys.iter().map(partition_key).collect();
            partition.push(partition_key(&v));
            let count = build::window_call(
                SqlAgg::Reducer(build::reducer(AggFn::Count, vec![v], false, Vec::new())),
                partition,
                Vec::new(),
                None,
            );
            ps.push(build::projection(count, Some(name.clone()), None)?);
            put(counts, mode, build::derived(Some(&alias), &name));
        }
        let counted = build::select_of(ps, from.clone(), Vec::new())?;
        let spelled = ansi::stars_as_source_columns(s, outputs, &alias, "BigQuery")?;
        let s = build::with_projections(s.clone(), spelled)?;
        build::with_from(s, build::subselect(build::query(counted), &alias, None))
    }

    /// A mode where it is written: the value of its most counted row (Java's `modeArm`).
    fn mode_arm(&self, w: &mut SqlWriter, mode: Mode<'_>) -> Result<(), Refusal> {
        let count = {
            let counts = self.mode_counts.borrow();
            counts.last().and_then(|top| top.iter().find(|(k, _)| mode.is(k)).map(|(_, c)| c.clone()))
        };
        let Some(count) = count else {
            return Err(Refusal::illegal_state("a MODE reached BigQuery's writer uncounted: modesCounted did not run"));
        };
        let r = match mode {
            Mode::Grouped(r) => r,
            Mode::Window { r#fn, .. } => match r#fn {
                SqlAgg::Reducer(r) => r,
                _ => unreachable!("a windowed MODE is a reducer"),
            },
        };
        if r.args.len() != 1 || r.distinct {
            return Err(Refusal::capability(format!("a mode of {} arguments reached BigQuery", r.args.len())));
        }
        let v = &r.args[0];
        if let Mode::Window { partition_by, .. } = mode {
            w.append("FIRST_VALUE(").expr(self, v, 0)?.append(" IGNORE NULLS) OVER (");
            if !partition_by.is_empty() {
                w.append("PARTITION BY ").list(self, partition_by)?.append(" ");
            }
            w.append("ORDER BY ").expr(self, &count, 0)?.append(" DESC, ").expr(self, v, 0)?;
            w.append(" ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)");
            return Ok(());
        }
        w.append("ARRAY_AGG(").expr(self, v, 0)?.append(" IGNORE NULLS ORDER BY ").expr(self, &count, 0)?;
        w.append(" DESC, ").expr(self, v, 0)?.append(" LIMIT 1)[SAFE_OFFSET(0)]");
        Ok(())
    }

    /// At the root, a value NULL by its shape is its label's NULL (Java's `rootNulls`).
    fn root_nulls(&self, s: SqlSelect, depth: usize) -> Result<SqlSelect, Refusal> {
        let labels = self.roots.borrow().last().cloned();
        let Some(labels) = labels else {
            return Ok(s);
        };
        if depth != 0 || labels.len() != s.projections.len() || !s.projections.iter().any(|p| null_valued(&p.expr)) {
            return Ok(s);
        }
        let mut ps = Vec::with_capacity(s.projections.len());
        for (i, p) in s.projections.iter().enumerate() {
            ps.push(if null_valued(&p.expr) {
                build::projection(
                    build::cast(build::null_lit(), labels[i].r#type.clone()),
                    p.alias.clone(),
                    p.out.clone(),
                )?
            } else {
                p.clone()
            });
        }
        build::select(SqlSelect { projections: ps, ..s })
    }

    /// `s` with each star over a FROM holding a marker spelled out (Java's `starsSpelled`).
    fn stars_spelled(&self, s: SqlSelect) -> Result<SqlSelect, Refusal> {
        let mut ps = Vec::new();
        if s.projections.is_empty() {
            spell(&s.from, None, &mut ps)?;
        }
        for p in &s.projections {
            if let SqlExpr::Star { table, .. } = &p.expr {
                spell(&s.from, table.as_deref(), &mut ps)?;
            } else {
                ps.push(p.clone());
            }
        }
        build::select(SqlSelect { projections: ps, ..s })
    }
}

/// A MODE where it is written: a grouped reducer, or a window call over one (Java's `SqlExpr mode`).
#[derive(Clone, Copy)]
enum Mode<'a> {
    Grouped(&'a Reducer),
    Window { r#fn: &'a SqlAgg, partition_by: &'a [SqlExpr], order_by: &'a [SortKey], frame: Option<&'a Frame> },
}

impl Mode<'_> {
    /// Whether `key` (a MODE `modesCounted` recorded) is this one, by Java's record equality (a window's type fact is
    /// its function's, so its parts decide).
    fn is(&self, key: &SqlExpr) -> bool {
        match (self, key) {
            (Mode::Grouped(r), SqlExpr::Reducer(k)) => *r == k,
            (
                Mode::Window { r#fn, partition_by, order_by, frame },
                SqlExpr::WindowCall { r#fn: kf, partition_by: kp, order_by: ko, frame: kr, .. },
            ) => {
                **r#fn == **kf && *partition_by == kp.as_slice() && *order_by == ko.as_slice() && *frame == kr.as_ref()
            }
            _ => false,
        }
    }
}

/// `LinkedHashMap.put`: a key met again keeps its place, its value replaced.
fn put(into: &mut Vec<(SqlExpr, SqlExpr)>, key: SqlExpr, value: SqlExpr) {
    match into.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => into.push((key, value)),
    }
}

/// Each MODE under `e`: a grouped reducer with the select's keys, a window call with its partition (Java's
/// `collectModes`).
fn collect_modes(e: &SqlExpr, group_by: &[SqlExpr], out: &mut Vec<(SqlExpr, Vec<SqlExpr>)>) -> Result<(), Refusal> {
    if let SqlExpr::Reducer(r) = e
        && r.r#fn == AggFn::Mode
    {
        put_modes(out, e.clone(), group_by.to_vec());
        return Ok(());
    }
    if let SqlExpr::WindowCall { r#fn, partition_by, order_by, frame, .. } = e
        && let SqlAgg::Reducer(r) = r#fn.as_ref()
        && r.r#fn == AggFn::Mode
    {
        if !order_by.is_empty() || frame.is_some() {
            return Err(Refusal::capability("a MODE over an ordered or framed window reached BigQuery"));
        }
        put_modes(out, e.clone(), partition_by.clone());
        return Ok(());
    }
    for c in typing::children(e) {
        collect_modes(&c, group_by, out)?;
    }
    Ok(())
}

fn put_modes(into: &mut Vec<(SqlExpr, Vec<SqlExpr>)>, key: SqlExpr, value: Vec<SqlExpr>) {
    match into.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => into.push((key, value)),
    }
}

/// The value a recorded MODE counts (Java's `modeValue`).
fn mode_value(mode: &SqlExpr) -> Result<&SqlExpr, Refusal> {
    let r = match mode {
        SqlExpr::WindowCall { r#fn, .. } => match r#fn.as_ref() {
            SqlAgg::Reducer(r) => r,
            _ => unreachable!("a windowed MODE is a reducer"),
        },
        SqlExpr::Reducer(r) => r,
        _ => unreachable!("a MODE"),
    };
    if r.args.len() != 1 || r.distinct {
        return Err(Refusal::capability(format!("a mode of {} arguments reached BigQuery", r.args.len())));
    }
    Ok(&r.args[0])
}

/// `key` as a window partitions by it: a FLOAT64 by its JSON text (Java's `partitionKey`).
fn partition_key(key: &SqlExpr) -> SqlExpr {
    if is_scalar(key, Scalar::Double) {
        build::cast(build::cast(key.clone(), SqlType::Scalar(Scalar::Json)), SqlType::Scalar(Scalar::Varchar))
    } else {
        key.clone()
    }
}

/// The markers' exploded columns, by alias, anywhere in a FROM (Java's `lateralsOf`).
fn laterals_of(src: &SqlSource, into: &mut Scope) -> Result<(), Refusal> {
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
                    let xs = unnest_list(&p.expr)?.clone();
                    match cols.iter_mut().find(|(n, _)| *n == name) {
                        Some(slot) => slot.1 = xs,
                        None => cols.push((name, xs)),
                    }
                }
                match into.iter_mut().find(|(a, _)| a == alias) {
                    Some(slot) => slot.1 = cols,
                    None => into.push((alias.clone(), cols)),
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Each other source's columns of a star, then each exploded column (Java's `spell`).
fn spell(src: &SqlSource, table: Option<&str>, into: &mut Vec<Projection>) -> Result<(), Refusal> {
    match src {
        SqlSource::Join { left, right, .. } => {
            spell(left, table, into)?;
            spell(right, table, into)
        }
        SqlSource::Subselect { inner, alias, .. } if is_marker(inner) && table.is_none_or(|t| t == alias) => {
            let SqlQuery::Select(m) = inner.as_ref() else { unreachable!("a marker is a select") };
            for p in &m.projections {
                let read = exploded_read(alias, p, p.out.as_ref().map(|o| &o.r#type))?;
                into.push(build::projection(read, Some(marker_column(p)?), p.out.clone())?);
            }
            Ok(())
        }
        SqlSource::Dual => Ok(()),
        other => {
            let alias = source_alias(other)?;
            if table.is_none_or(|t| t == alias) {
                into.push(build::projection(build::star(Some(alias)), None, None)?);
            }
            Ok(())
        }
    }
}

// ==================================================================
// The renderer (Java's overrides of AnsiSqlRenderer)
// ==================================================================

impl AnsiSqlRenderer for BigQuery {
    fn lexicon(&self) -> &Lexicon {
        &LEXICON
    }

    fn ansi_state(&self) -> &AnsiState {
        &self.ansi
    }

    fn simple_name(&self) -> &'static str {
        "BigQuery"
    }

    /// The statement's labels (its root's outputs before the passes) in scope while it is written.
    fn render(&self, query: SqlQuery) -> Result<String, Refusal> {
        self.roots.borrow_mut().push(ansi::query_outputs(&query).to_vec());
        let rendered = ansi::render(self, query);
        self.roots.borrow_mut().pop();
        rendered
    }

    fn render_statement(&self, query: SqlQuery) -> Result<RenderedStatement, Refusal> {
        self.roots.borrow_mut().push(ansi::query_outputs(&query).to_vec());
        let rendered = ansi::render_statement(self, query);
        self.roots.borrow_mut().pop();
        rendered
    }

    /// GoogleSQL DDL: a temporary table, unqualified, lives in a session; a key is declared NOT ENFORCED.
    fn render_ddl(&self, ddl: &SqlDdl) -> Result<String, Refusal> {
        self.render_ddl_arm(ddl)
    }

    /// A DELETE needs a WHERE clause in GoogleSQL.
    fn render_dml(&self, dml: &SqlDml) -> Result<String, Refusal> {
        if let SqlDml::DeleteAll { schema, table, quoting } = dml {
            return Ok(format!("DELETE FROM {} WHERE TRUE;", self.qualified(schema.as_deref(), table, *quoting)));
        }
        ansi::render_dml(self, dml)
    }

    /// The engine's H2 text is refused here: BigQuery has no translation of it.
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        let _ = statement;
        Err(Refusal::capability("the engine's H2 text reached BigQuery, which has no translation of it"))
    }

    /// A Float's literal spelling over the double's text bound once.
    fn binds_float_text(&self) -> bool {
        true
    }

    /// A Float's print forms over the value and its text bound once.
    fn binds_float_print(&self) -> bool {
        true
    }

    /// The portable carrier strategies, a closed fold as a top-level recursive expression, the substring start clamp,
    /// the explosions as markers, averages delivered as DOUBLE, a constant GROUP BY or ORDER BY key as a typed
    /// expression, and a large repeated subexpression written once.
    fn passes(&self) -> Vec<Box<dyn SqlRewriter>> {
        vec![
            Box::new(carrier::CarrierStrategies::new(CAPS)),
            Box::new(folds::FoldsAsRecursiveCtes::default()),
            Box::new(passes::SubstringClamp),
            Box::new(explosion_markers::LateralUnnestAsMarker),
            Box::new(explosion_markers::SelectListUnnestAsLateral::default()),
            Box::new(explosion_markers::StarOverExplosionUnwrapped),
            Box::new(shared_passes::AggregatesDeliverDouble::new(&[AggFn::Avg])),
            Box::new(passes::ConstantKeysAsExpressions::default()),
            Box::new(common_subexpressions::CommonSubexpressions::new()),
        ]
    }

    fn supports_qualify(&self) -> bool {
        true
    }

    /// QUALIFY needs no WHERE, GROUP BY or HAVING beside it.
    fn append_qualify(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        self.nl(w, depth);
        let q = s.qualify.as_ref().ok_or_else(|| Refusal::null_pointer("qualify"))?;
        w.append("QUALIFY ").expr(self, q, 0)?;
        Ok(())
    }

    fn star_except_keyword(&self) -> Result<String, Refusal> {
        Ok("EXCEPT".to_owned())
    }

    fn row_order_column(&self) -> Result<String, Refusal> {
        Err(Refusal::capability("row order reached BigQuery, which has no row-order column"))
    }

    fn as_of_join_clause(&self) -> Result<String, Refusal> {
        Err(Refusal::capability(
            "an as-of join reached BigQuery, which has none (the carrier strategies emulate the witnessed shapes)",
        ))
    }

    /// A static pivot never reaches here (the carrier strategies emulate it).
    fn pivot_source(&self, w: &mut SqlWriter, pivot: &SqlSource, depth: usize) -> Result<(), Refusal> {
        let _ = (w, pivot, depth);
        Err(Refusal::capability(
            "a pivot reached BigQuery unemulated (dynamic, several keys, or a COUNT(*) using): its native PIVOT names \
             columns alias_value",
        ))
    }

    /// UNION is UNION ALL or UNION DISTINCT; a branch with its own ORDER BY or LIMIT, or a union, is parenthesized.
    fn query(&self, w: &mut SqlWriter, q: &SqlQuery, depth: usize) -> Result<(), Refusal> {
        let SqlQuery::Union(u) = q else {
            return ansi::query(self, w, q, depth);
        };
        let op = if u.all { "UNION ALL" } else { "UNION DISTINCT" };
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
                w.append("(");
            }
            self.query(w, b, depth)?;
            if wrap {
                w.append(")");
            }
        }
        Ok(())
    }

    /// OFFSET only after a LIMIT; a marker select its reads over the marker's positions; a FROM-less select with a
    /// filter over one row; the select's MODEs counted, its markers in scope, its stars over them spelled.
    fn select(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        if is_marker_select(s) {
            let alias = format!("__u{depth}");
            let aligned = s.outputs.len() == s.projections.len();
            let mut ps = Vec::with_capacity(s.projections.len());
            for (i, p) in s.projections.iter().enumerate() {
                let slot = if aligned { Some(&s.outputs[i].r#type) } else { p.out.as_ref().map(|o| &o.r#type) };
                let read = exploded_read(&alias, p, slot)?;
                ps.push(build::projection(read, Some(marker_column(p)?), p.out.clone())?);
            }
            let position = build::sort_key(build::derived(None, &alias), true, None, None);
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
            return self.select(w, &over, depth);
        }
        if matches!(s.from, SqlSource::Dual) && (s.r#where.is_some() || !s.group_by.is_empty() || s.having.is_some()) {
            // "Query without FROM clause cannot have a WHERE clause": over one row instead
            let one = build::select_of(
                vec![build::projection(build::int_lit(1), Some("__one".to_owned()), None)?],
                SqlSource::Dual,
                Vec::new(),
            )?;
            let over =
                build::select(SqlSelect { from: build::subselect(build::query(one), "__dual", None), ..s.clone() })?;
            return self.select(w, &over, depth);
        }
        let mut counts = Vec::new();
        let s = self.modes_counted(s, &mut counts)?;
        let mut scope = Vec::new();
        laterals_of(&s.from, &mut scope)?;
        let spelled = if scope.is_empty() { s } else { self.stars_spelled(s)? };
        let spelled = self.root_nulls(spelled, depth)?;
        self.laterals.borrow_mut().push(scope);
        self.mode_counts.borrow_mut().push(counts);
        let written = (|| {
            let s = if spelled.offset.is_some() && spelled.limit.is_none() {
                build::with_limit(spelled, Some(i64::MAX))?
            } else {
                spelled
            };
            ansi::select(self, w, &s, depth)
        })();
        self.mode_counts.borrow_mut().pop();
        self.laterals.borrow_mut().pop();
        written
    }

    /// A marker's positions (a lateral one joined correlated); any other lateral derived table unnested as structs.
    fn source(&self, w: &mut SqlWriter, src: &SqlSource, depth: usize) -> Result<(), Refusal> {
        match src {
            SqlSource::Join {
                left, right, kind: kind @ (JoinKind::CrossLateral | JoinKind::LeftLateral), on, ..
            } => {
                if let SqlSource::Subselect { inner, alias, .. } = right.as_ref()
                    && let SqlQuery::Select(m) = inner.as_ref()
                    && is_marker_select(m)
                {
                    if let Some(on) = on
                        && !matches!(on.as_ref(), SqlExpr::BoolLit { value: true, .. })
                    {
                        return Err(Refusal::capability("a lateral explosion with a join condition reached BigQuery"));
                    }
                    self.source(w, left, depth)?;
                    self.nl(w, depth);
                    let is_left = *kind == JoinKind::LeftLateral;
                    w.append(if is_left { "LEFT JOIN " } else { "CROSS JOIN " });
                    self.positions(w, m)?;
                    let a = self.alias_ident(alias);
                    w.append(" AS ").append(&a);
                    if is_left {
                        w.append(" ON TRUE");
                    }
                    return Ok(());
                }
                self.lateral(w, left, right, *kind, on.as_deref(), depth)
            }
            SqlSource::Subselect { inner, alias, .. } if is_marker(inner) => {
                let SqlQuery::Select(m) = inner.as_ref() else { unreachable!("a marker is a select") };
                self.positions(w, m)?;
                let a = self.alias_ident(alias);
                w.append(" AS ").append(&a);
                Ok(())
            }
            other => ansi::source(self, w, other, depth),
        }
    }

    /// No VALUES clause: the rows as a typed array of structs, unnested.
    fn values_source(
        &self,
        w: &mut SqlWriter,
        rows: &[Vec<SqlExpr>],
        columns: &[String],
        alias: &str,
        outputs: &[OutputCol],
    ) -> Result<(), Refusal> {
        if outputs.len() != columns.len() {
            return Err(Refusal::capability("a VALUES source without a declared type per column reached BigQuery"));
        }
        w.append("(SELECT * FROM UNNEST(ARRAY<STRUCT<");
        for (i, column) in columns.iter().enumerate() {
            let cells = rows.iter().map(|r| arg(r, i)).collect::<Result<Vec<_>, _>>()?;
            let name = self.ident(column);
            let held = self.held_type(&outputs[i].r#type, &cells)?;
            w.append(if i > 0 { ", " } else { "" }).append(&name).append(" ").append(&held);
        }
        w.append(">>[");
        for (r, row) in rows.iter().enumerate() {
            w.append(if r > 0 { ", " } else { "" }).append("STRUCT(").list(self, row)?.append(")");
        }
        let a = self.alias_ident(alias);
        w.append("])) AS ").append(&a);
        Ok(())
    }

    // ---- lexical: identifiers, literals, types

    /// Every identifier in backticks; a double-quoted declaration is its inner name.
    fn ident(&self, name: &str) -> String {
        if name.len() > 1
            && name.starts_with('"')
            && name.ends_with('"')
            && !name[1..name.len() - 1].replace("\"\"", "").contains('"')
        {
            return self.delimited(&name[1..name.len() - 1].replace("\"\"", "\""));
        }
        self.delimited(name)
    }

    /// A backtick and a backslash inside a quoted identifier escaped by a backslash.
    fn delimited(&self, name: &str) -> String {
        format!("`{}`", name.replace('\\', "\\\\").replace('`', "\\`"))
    }

    /// Backslash escapes: a quote `\'`, the control characters `\xHH`, a `?` `\x3f`.
    fn string_lit(&self, value: &str) -> Result<String, Refusal> {
        let mut sb = String::with_capacity(value.len() + 2);
        sb.push('\'');
        for ch in value.chars() {
            match ch {
                '\\' => sb.push_str("\\\\"),
                '\'' => sb.push_str("\\'"),
                '\n' => sb.push_str("\\n"),
                '\r' => sb.push_str("\\r"),
                '\t' => sb.push_str("\\t"),
                '?' => sb.push_str("\\x3f"),
                c if (c as u32) < 0x20 || c as u32 == 0x7f => sb.push_str(&format!("\\x{:02x}", c as u32)),
                c => sb.push(c),
            }
        }
        sb.push('\'');
        Ok(sb)
    }

    /// A naive date-time is a DATETIME of six fraction digits at most (finer ones cut).
    fn timestamp_lit(&self, iso: &str) -> Result<String, Refusal> {
        let t = iso.replace('T', " ");
        let units: Vec<u16> = t.encode_utf16().collect();
        let t = match units.iter().rposition(|&u| u == u16::from(b'.')) {
            Some(dot) => {
                let digits = units[dot + 1..].iter().take_while(|&&u| lite_javacompat::character::is_digit(u)).count();
                if digits > 6 {
                    let kept = String::from_utf16_lossy(&units[..dot + 7]);
                    let rest = String::from_utf16_lossy(&units[dot + 1 + digits..]);
                    format!("{kept}{rest}")
                } else {
                    t
                }
            }
            None => t,
        };
        Ok(format!("DATETIME '{t}'"))
    }

    /// A Float literal is an exact BIGNUMERIC; a magnitude BIGNUMERIC cannot hold, NaN and the infinities FLOAT64.
    fn float_literal(&self, v: f64) -> Result<String, Refusal> {
        if v.is_nan() {
            return Ok("CAST('nan' AS FLOAT64)".to_owned());
        }
        if v.is_infinite() {
            return Ok(if v > 0.0 { "CAST('inf' AS FLOAT64)" } else { "CAST('-inf' AS FLOAT64)" }.to_owned());
        }
        if let Some(plain) = crate::java::float_decimal(v)?
            && fits_big_numeric(&plain)
        {
            return Ok(format!("BIGNUMERIC '{}'", crate::java::big_decimal_plain(&plain)));
        }
        Ok(format!("CAST('{}' AS FLOAT64)", lite_javacompat::double_to_string(v)))
    }

    /// A list type: an array holds no array.
    fn array_type_name(&self, element: &str) -> Result<String, Refusal> {
        if element.starts_with("ARRAY<") {
            return Err(nested_list());
        }
        Ok(format!("ARRAY<{element}>"))
    }

    /// A map is its entries in order, `ARRAY<STRUCT<key K, value V>>`.
    fn map_type_name(&self, key: &str, value: &str) -> String {
        format!("ARRAY<STRUCT<{} {key}, {} {value}>>", self.ident("key"), self.ident("value"))
    }

    fn struct_type_name(&self, fields: &[StructField]) -> Result<String, Refusal> {
        let fields = ansi::struct_fields(self, fields)?;
        let mut sb = String::from("STRUCT<");
        for (i, (name, t)) in fields.iter().enumerate() {
            sb.push_str(if i > 0 { ", " } else { "" });
            sb.push_str(name);
            sb.push(' ');
            sb.push_str(t);
        }
        sb.push('>');
        Ok(sb)
    }

    /// Alias-less projections label EXPLICITLY from the declared output.
    fn implicit_label(&self, p: &Projection) -> Result<Option<String>, Refusal> {
        match &p.out {
            Some(out) => Ok(Some(self.alias_ident(&out.name))),
            None => ansi::implicit_label(self, p),
        }
    }

    /// A projection that IS error() is raised in its column's type.
    fn projection_value(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        if let Some(out) = &p.out
            && let SqlExpr::Call { r#fn: SqlFn::Error, args, .. } = &p.expr
        {
            let t = self.bq_type(&out.r#type)?;
            let datetime = t == "DATETIME";
            w.append(if datetime { "CAST(IF(TRUE, " } else { "IF(TRUE, " });
            ansi::raise_envelope(self, w, args, "ERROR")?;
            if datetime {
                w.append(", CAST(NULL AS STRING)) AS DATETIME)");
            } else {
                w.append(&format!(", CAST(NULL AS {t}))"));
            }
            return Ok(());
        }
        ansi::projection_value(self, w, p)
    }

    /// A NULL under a list, struct or decimal slot is typed as the slot.
    fn projection(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        if let SqlExpr::NullLit { .. } = p.expr
            && let Some(out) = &p.out
            && matches!(out.r#type, SqlType::Array { .. } | SqlType::Struct { .. } | SqlType::Decimal { .. })
        {
            let t = self.bq_type(&out.r#type)?;
            w.append("CAST(NULL AS ").append(&t).append(")");
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
                let text = self.decimal_literal(value.as_str())?;
                w.append(&text);
                Ok(())
            }
            SqlExpr::ArrayLit { .. } => self.array_literal(w, e),
            // DuckDB's list(v ORDER BY k): ascending, NULLs last
            SqlExpr::OrderedListAgg { value, order_by, .. } => {
                w.append("ARRAY_AGG(").expr(self, value, 0)?.append(" ORDER BY ");
                w.piece(&*aggregates::order_key(self.ex(order_by, 0), true, false))?.append(")");
                Ok(())
            }
            SqlExpr::Membership { collection, .. } if matches!(collection.as_ref(), SqlExpr::PlanParam { .. }) => {
                let SqlExpr::PlanParam { name, .. } = collection.as_ref() else { unreachable!("a plan parameter") };
                Err(list_parameter(name))
            }
            SqlExpr::Quantified { value, comparison, quantifier, subquery, .. } => {
                self.quantified(w, value, *comparison, *quantifier, subquery)
            }
            SqlExpr::Column(c) => match self.exploded_column(c) {
                Some((xs, position)) => self.element(w, c, &xs, &position),
                None => ansi::expr(self, w, e, parent_prec),
            },
            other => ansi::expr(self, w, other, parent_prec),
        }
    }

    /// A CASE mixing a text branch with others is text: each other branch cast to STRING.
    fn case_expr(
        &self,
        w: &mut SqlWriter,
        whens: &[When],
        otherwise: Option<&SqlExpr>,
        r#type: &TypeFact,
    ) -> Result<(), Refusal> {
        let branches: Vec<&SqlExpr> = whens.iter().map(|w| &w.then).chain(otherwise).collect();
        let text = branches.iter().any(|b| textual(b));
        let mixed = text && branches.iter().any(|b| !textual(b) && !matches!(b, SqlExpr::NullLit { .. }));
        if !mixed {
            let _ = r#type;
            return ansi::case_expr(self, w, whens, otherwise);
        }
        let as_text = |b: &SqlExpr| -> SqlExpr {
            if textual(b) || matches!(b, SqlExpr::NullLit { .. }) {
                b.clone()
            } else {
                build::cast(b.clone(), SqlType::Scalar(Scalar::Varchar))
            }
        };
        w.append("CASE");
        for wh in whens {
            w.append(" WHEN ").expr(self, &wh.condition, 0)?.append(" THEN ").expr(self, &as_text(&wh.then), 0)?;
        }
        if let Some(o) = otherwise {
            w.append(" ELSE ").expr(self, &as_text(o), 0)?;
        }
        w.append(" END");
        Ok(())
    }

    // ---- lists: ARRAY<T>, lambda-less

    fn array_lit(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("[").list(self, elements)?.append("]");
        Ok(())
    }

    /// Typed when declared, so a NULL field has its type.
    fn struct_lit(&self, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
        if let TypeFact::Typed { r#type: st @ SqlType::Struct { .. }, .. } = typing::struct_lit_type(fields)
            && let SqlType::Struct { fields: declared } = &st
            && declared.len() == fields.len()
        {
            let t = self.bq_type(&st)?;
            w.append(&t).append("(").join(fields, ", ", |w, f| {
                w.expr(self, &f.value, 0)?;
                Ok(())
            })?;
            w.append(")");
            return Ok(());
        }
        w.append("STRUCT(").join(fields, ", ", |w, f| {
            let name = self.ident(&f.name);
            w.expr(self, &f.value, 0)?.append(" AS ").append(&name);
            Ok(())
        })?;
        w.append(")");
        Ok(())
    }

    fn struct_get(&self, w: &mut SqlWriter, source: &SqlExpr, field: &str, r#type: &TypeFact) -> Result<(), Refusal> {
        let _ = r#type;
        let f = self.ident(field);
        w.append("(").expr(self, source, 0)?.append(").").append(&f);
        Ok(())
    }

    fn struct_insert(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let untyped = || Refusal::capability("struct_insert over an untyped struct reached BigQuery");
        let Some(SqlType::Struct { fields }) = type_of(arg(args, 0)?) else {
            return Err(untyped());
        };
        let SqlExpr::StringLit { value: name, .. } = arg(args, 1)? else {
            return Err(untyped());
        };
        w.append("STRUCT(");
        let mut first = true;
        for f in fields {
            if f.name == *name {
                continue;
            }
            let n = self.ident(&f.name);
            w.append(if first { "" } else { ", " }).append("(").expr(self, &args[0], 0)?.append(").").append(&n);
            w.append(" AS ").append(&n);
            first = false;
        }
        let n = self.ident(name);
        w.append(if first { "" } else { ", " }).expr(self, arg(args, 2)?, 0)?.append(" AS ").append(&n).append(")");
        Ok(())
    }

    /// exists([]) is false.
    fn list_exists(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list_predicate(w, args, "LOGICAL_OR", "FALSE", SqlFn::ListExists)
    }

    /// forAll([]) is true.
    fn list_for_all(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list_predicate(w, args, "LOGICAL_AND", "TRUE", SqlFn::ListForAll)
    }

    /// No duplicates iff the distinct count is the count; an empty or NULL list is distinct.
    fn all_distinct(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let e = self.param("__e");
        w.append(&format!("COALESCE((SELECT COUNT(*) = COUNT(DISTINCT {e}) FROM UNNEST("));
        w.expr(self, arg(args, 0)?, 0)?.append(&format!(") AS {e}), TRUE)"));
        Ok(())
    }

    /// DuckDB's list_contains: NULL for a NULL list or needle; else found or not.
    fn membership(&self, w: &mut SqlWriter, needle: &SqlExpr, collection: &SqlExpr) -> Result<(), Refusal> {
        let e = self.param("__e");
        w.append("(CASE WHEN ").expr(self, collection, 4)?.append(" IS NULL OR ").expr(self, needle, 4)?;
        w.append(" IS NULL THEN NULL ELSE EXISTS(SELECT 1 FROM UNNEST(").expr(self, collection, 0)?;
        w.append(&format!(") AS {e} WHERE {e} = ")).expr(self, needle, 5)?.append(") END)");
        Ok(())
    }

    fn reduce_collection(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        collection: &SqlExpr,
        extras: &[SqlExpr],
    ) -> Result<(), Refusal> {
        let rows = self.over_rows(collection, reducer, extras)?;
        w.piece(&*rows)?;
        Ok(())
    }

    fn unnest_projection(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability(
            "an unnest in a select list reached BigQuery, which has no set-returning projection (an explode is placed \
             in FROM by the carrier strategies)",
        ))
    }

    fn fold_call(&self, w: &mut SqlWriter, fold: &SqlExpr) -> Result<(), Refusal> {
        let _ = (w, fold);
        Err(Refusal::capability(
            "a fold that reads its row reached BigQuery, which has no lambdas and no correlated recursion (WITH \
             RECURSIVE is top-level only, measured; a closed fold is hoisted there)",
        ))
    }

    /// Pure's splitPart: the non-empty tokens, 1-based, NULL past the end.
    fn split_part_call(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let t = self.param("__t");
        let to = self.param("__to");
        w.append(&format!("ARRAY(SELECT {t} FROM UNNEST(SPLIT(")).expr(self, arg(a, 0)?, 0)?.append(", ");
        w.expr(self, arg(a, 1)?, 0)?;
        w.append(&format!(")) AS {t} WITH OFFSET AS {to} WHERE {t} <> '' ORDER BY {to})[SAFE_ORDINAL(CAST("));
        w.expr(self, arg(a, 2)?, 0)?.append(" AS INT64))]");
        Ok(())
    }

    // ---- variant: BigQuery's JSON type

    fn variant_construct(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() != 1 {
            return Err(Refusal::capability(format!("toVariant of {} arguments reached BigQuery", a.len())));
        }
        if is_json(&a[0]) {
            w.expr(self, &a[0], 0)?;
        } else {
            w.append("TO_JSON(").expr(self, &a[0], 0)?.append(")");
        }
        Ok(())
    }

    /// A member by key, or a 0-based index (-1 the last; past either end NULL): the JSON subscript.
    fn variant_get(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let key = arg(args, 1)?;
        if let SqlExpr::StringLit { value, .. } = key
            && value == "$"
        {
            w.expr(self, arg(args, 0)?, 0)?;
            return Ok(());
        }
        if is_text(key) {
            w.append("(").expr(self, arg(args, 0)?, 0)?.append(")[").expr(self, key, 0)?.append("]");
            return Ok(());
        }
        w.append("(").expr(self, arg(args, 0)?, 0)?.append(")[CASE WHEN ").expr(self, key, 4)?;
        w.append(" < 0 THEN ARRAY_LENGTH(JSON_QUERY_ARRAY(").expr(self, &args[0], 0)?.append(")) + CAST(");
        w.expr(self, key, 0)?.append(" AS INT64) ELSE CAST(").expr(self, key, 0)?.append(" AS INT64) END]");
        Ok(())
    }

    /// A Variant array's elements, each a Variant.
    fn variant_elements(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let a0 = arg(args, 0)?;
        if list_element(a0).is_some() {
            w.expr(self, a0, 0)?;
            return Ok(());
        }
        w.append("JSON_QUERY_ARRAY(").expr(self, a0, 0)?.append(")");
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
        self.variant_aware_cast_arm(w, value, target)
    }

    // ---- scalar functions: every SqlFn decided in `functions.rs`

    fn call(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact, parent_prec: i32) -> Result<(), Refusal> {
        self.call_arm(w, f, a, t, parent_prec)
    }

    // ---- the hooks of the base, the aggregates and windows: `aggregates.rs`

    /// Pure's hashCode: FARM_FINGERPRINT of the value's text as JSON.
    fn hash_signed(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 1 {
            w.append("FARM_FINGERPRINT(TO_JSON_STRING(").piece(&*self.as_text(&a[0]))?.append("))");
            return Ok(());
        }
        w.append("FARM_FINGERPRINT(TO_JSON_STRING([").join(a, ", ", |w, x| {
            w.piece(&*self.as_text(x))?;
            Ok(())
        })?;
        w.append("]))");
        Ok(())
    }

    /// Pure round is half-EVEN: ROUND(x, n, 'ROUND_HALF_EVEN') over a BIGNUMERIC.
    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("ROUND(CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS BIGNUMERIC), ");
        if a.len() > 1 {
            w.append("CAST(").expr(self, &a[1], 0)?.append(" AS INT64)");
        } else {
            w.append("0");
        }
        w.append(", 'ROUND_HALF_EVEN')");
        Ok(())
    }

    fn bit_op(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.bit_op_arm(w, f, a)
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

    fn json_object(&self, w: &mut SqlWriter, kv: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("JSON_OBJECT(").list(self, kv)?.append(")");
        Ok(())
    }

    fn json_array(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("JSON_ARRAY(").list(self, elements)?.append(")");
        Ok(())
    }

    /// A JSON array of the values, NULLs kept as JSON nulls; over no rows the empty array.
    fn json_array_agg(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        order_keys: &[JsonArrayAggKey],
    ) -> Result<(), Refusal> {
        w.append("TO_JSON(IFNULL(ARRAY_AGG(TO_JSON(").expr(self, value, 0)?.append(")");
        if !order_keys.is_empty() {
            w.append(" ORDER BY ").join(order_keys, ", ", |w, k| {
                w.piece(&*aggregates::order_key(self.ex(&k.expr, 0), !k.desc, false))?;
                Ok(())
            })?;
        }
        w.append("), ARRAY<JSON>[]))");
        Ok(())
    }

    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal> {
        aggregates::format_text(parts)
    }

    /// A date-time parameter is cast to DATETIME; finer than a microsecond, its first six digits.
    fn hole_type(&self, kind: ValueKind) -> Result<TypeSpelling, Refusal> {
        match kind {
            ValueKind::Date => Ok(TypeSpelling::new("DATE", Digits::None)),
            ValueKind::DateTime => Ok(TypeSpelling::new("DATETIME", Digits::None)),
            ValueKind::DateTimeNanos => {
                Ok(TypeSpelling { name: "DATETIME".to_owned(), digits: Digits::None, fraction: Some(6) })
            }
            ValueKind::Integer | ValueKind::Decimal | ValueKind::Floating => ansi::hole_type(kind),
        }
    }

    /// This database's name for each scalar type, a refusal where it has none.
    fn scalar_type_name(&self, s: Scalar) -> Result<String, Refusal> {
        Ok(match s {
            Scalar::Boolean => "BOOL",
            Scalar::Integer | Scalar::Bigint => "INT64",
            Scalar::Hugeint => "BIGNUMERIC",
            Scalar::Double => "FLOAT64",
            Scalar::Varchar | Scalar::TemporalText => "STRING",
            Scalar::Date => "DATE",
            Scalar::Timestamp => "DATETIME",
            Scalar::Timestamptz => "TIMESTAMP",
            Scalar::Json => "JSON",
            Scalar::Time => "TIME",
            Scalar::DecimalText => {
                return Err(Refusal::illegal_state(format!("{s} cast reached a dialect without {s} support")));
            }
        }
        .to_owned())
    }

    // ---- DDL

    fn ddl_type(&self, t: &ColumnType) -> Result<String, Refusal> {
        aggregates::ddl_type(t)
    }

    // ---- values: what the driver hands back

    /// Its JSON_TYPE answers DuckDB's names (a CASE over JSON_TYPE).
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

    /// Refused, as TYPEOF is here (its type names are BigQuery's).
    fn typeof_name(&self, scalar: Scalar) -> Result<String, Refusal> {
        let _ = scalar;
        Err(Refusal::capability("typeof reached BigQuery: its type names are BigQuery's"))
    }
}

impl BigQuery {
    fn list_predicate(
        &self,
        w: &mut SqlWriter,
        args: &[SqlExpr],
        agg: &str,
        empty: &str,
        f: SqlFn,
    ) -> Result<(), Refusal> {
        let l = lambda_of(arg(args, 1)?, f)?;
        let xs = arg(args, 0)?;
        w.append(&format!("COALESCE((SELECT {agg}("));
        let body = lambda_body(xs, l)?;
        w.expr(self, &body, 0)?.append(") FROM ");
        let elements = self.elements(xs, &l.params[0], &offset_of(l))?;
        w.piece(&*elements)?.append(&format!("), {empty})"));
        Ok(())
    }

    /// `schema.table` as a statement names a stored table (Java's `qualified`).
    fn qualified(&self, schema: Option<&str>, table: &str, quoting: NameQuoting) -> String {
        match schema {
            None | Some("") | Some("default") => AnsiSqlRenderer::stored_name(self, table, quoting.table),
            Some(s) => format!(
                "{}.{}",
                AnsiSqlRenderer::stored_name(self, s, quoting.schema),
                AnsiSqlRenderer::stored_name(self, table, quoting.table)
            ),
        }
    }
}

#[cfg(test)]
mod tests;

// ==================================================================
// SqlDialect
// ==================================================================

impl crate::dialect::SqlDialect for BigQuery {
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
        <BigQuery as AnsiSqlRenderer>::reads_stored(t)
    }

    fn table_name_spelled(&self, name: &str, quoting: NameQuoting) -> String {
        AnsiSqlRenderer::table_name(self, name, quoting)
    }

    /// Nothing to set: a session's zone is UTC unless a statement names one.
    fn session_setup(&self) -> Vec<String> {
        Vec::new()
    }

    /// A transaction needs a session script.
    fn keys_snapshot(&self) -> Result<KeysSnapshot, Refusal> {
        Ok(KeysSnapshot::None)
    }

    /// 10,000 columns in a query result (its documented quota).
    fn result_column_limit(&self) -> Result<Option<i32>, Refusal> {
        Ok(Some(10000))
    }

    /// No native pivot is used: a static pivot is emulated, so a dynamic pivot is discovered first.
    fn needs_static_pivot(&self) -> bool {
        true
    }

    /// INT64, FLOAT64 and DATETIME are its own names.
    fn wire_type_name(&self, driver_type_name: Option<&str>) -> String {
        crate::wire_names::fold(driver_type_name.map(|n| match n {
            "INT64" => "BIGINT",
            "FLOAT64" => "DOUBLE",
            "DATETIME" => "TIMESTAMP",
            other => other,
        }))
    }

    /// Its exact decimal carrier, BIGNUMERIC; a map carried as its entries, an ARRAY at the wire.
    fn delivers_own(&self, label: &SqlType, wire: &str) -> bool {
        if matches!(label, SqlType::Map { .. }) && wire == "ARRAY" {
            return true;
        }
        if wire != "BIGNUMERIC" {
            return false;
        }
        match label {
            SqlType::Scalar(Scalar::Hugeint | Scalar::Double) => true,
            SqlType::Decimal { precision, scale } => *scale <= 38 && precision - scale <= 38,
            _ => false,
        }
    }
}
