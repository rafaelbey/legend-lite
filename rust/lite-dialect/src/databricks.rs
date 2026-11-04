// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `Databricks` (`core/src/main/java/com/legend/sql/dialect/Databricks.java`), the Databricks SQL execution
//! dialect (a SQL warehouse): the base renderer with every identifier in backticks, a string literal's backslash and
//! quote escaped by a backslash, every integer literal a BIGINT (`1L`), typed `ARRAY<T>` lists with lambdas (an index
//! parameter 0-based, read as DuckDB's 1-based one), `STRUCT`, `MAP` and `VARIANT`, native QUALIFY, PIVOT and `SELECT *
//! EXCEPT`, an as-of join as a LEFT JOIN picked by its QUALIFY, several unnests as one `inline(arrays_zip(...))`, a
//! quantified comparison as its truth table, and -- as its [`crate::SqlDialect`] -- ANSI mode on, no keys snapshot,
//! no column limit and a session's table a temporary view over its rows. Every `SqlFn` and aggregate is decided by an
//! exhaustive match: `src/databricks/functions.rs` (`call` and its helpers) and `src/databricks/aggregates.rs` (the
//! hooks of the base, the aggregates and windows, the JSON constructors and the date format).
//!
//! Java's `SqlWriter.Piece` is [`P`], the shared closure of [`crate::pieces`] (as Postgres's and Trino's).

use crate::ansi::{self, cast_type_name};
use crate::dialect::KeysSnapshot;
use crate::explosion_markers::unnest_list;
use crate::java::simple_name;
use crate::pieces::{Ex, P, pc, text};
use crate::refusal::{Refusal, arg};
use crate::renderer::{AnsiSqlRenderer, AnsiState, Lexicon};
use crate::rewriter::SqlRewriter;
use crate::shared_passes::{self, AsOfWords, as_of_pick};
use crate::statement::{RenderedStatement, list_text};
use crate::writer::SqlWriter;
use crate::{build, carrier, passes};
use lite_sql::{
    AggFn, ColumnKind, ColumnType, DateFmt, Frame, JoinKind, JsonArrayAggKey, JsonKind, Lambda, NameQuoting, Origin,
    OutputCol, Projection, Quantifier, Reducer, Scalar, SortKey, SqlAgg, SqlDdl, SqlDml, SqlExpr, SqlFn, SqlQuery,
    SqlSelect, SqlSource, SqlType, StructField, StructLitField, TypeFact, When,
};
use std::sync::LazyLock;

mod aggregates;
mod functions;

/// The Databricks dialect: one per render, as Java's `new Databricks()`. It carries no state across renders.
#[derive(Debug, Default)]
pub struct Databricks {
    ansi: AnsiState,
}

impl Databricks {
    /// Java's `new Databricks()`.
    pub fn new() -> Databricks {
        Databricks::default()
    }
}

/// Databricks' documented reserved set (ANSI mode's reserved words; Java's `LEXICON`'s words). Databricks quotes EVERY
/// identifier, so this list never decides a spelling there.
pub const RESERVED: &[&str] = &[
    "all",
    "alter",
    "and",
    "any",
    "array",
    "as",
    "at",
    "authorization",
    "between",
    "both",
    "by",
    "case",
    "cast",
    "check",
    "collate",
    "column",
    "commit",
    "constraint",
    "create",
    "cross",
    "cube",
    "current",
    "current_date",
    "current_time",
    "current_timestamp",
    "current_user",
    "delete",
    "describe",
    "distinct",
    "drop",
    "else",
    "end",
    "escape",
    "except",
    "exists",
    "external",
    "false",
    "fetch",
    "filter",
    "for",
    "foreign",
    "from",
    "full",
    "function",
    "global",
    "grant",
    "group",
    "grouping",
    "having",
    "in",
    "inner",
    "insert",
    "intersect",
    "interval",
    "into",
    "is",
    "join",
    "lateral",
    "leading",
    "left",
    "like",
    "local",
    "natural",
    "no",
    "not",
    "null",
    "of",
    "on",
    "only",
    "or",
    "order",
    "out",
    "outer",
    "overlaps",
    "partition",
    "position",
    "primary",
    "range",
    "references",
    "revoke",
    "right",
    "rollback",
    "rollup",
    "row",
    "rows",
    "select",
    "session_user",
    "set",
    "some",
    "start",
    "table",
    "tablesample",
    "then",
    "time",
    "to",
    "trailing",
    "true",
    "union",
    "unique",
    "unknown",
    "update",
    "user",
    "using",
    "values",
    "when",
    "where",
    "window",
    "with",
];

/// Java's `Databricks.LEXICON`: backticks and [`RESERVED`].
static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| Lexicon::new('`', RESERVED));

/// Java's `Databricks.CAPS`: typed native arrays with lambdas, as DuckDB's.
pub const CAPS: carrier::Caps =
    carrier::Caps { native_lists: true, correlated_explode: true, json_carrier: true, native_full_outer: true };

/// `Databricks.asOf`'s refusals.
const AS_OF: AsOfWords = AsOfWords {
    no_key: "an as-of join with no inequality between its sides reached Databricks",
    filtered: "an as-of join under a select that filters, groups or windows reached Databricks: its pick is that \
               select's QUALIFY",
    no_left: "an as-of join whose left side has no alias and columns reached Databricks",
};

// ==================================================================
// Pieces and the type facts the arms read
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

/// Java's `listElement(e)`.
fn list_element(e: &SqlExpr) -> Option<&SqlType> {
    match type_of(e) {
        Some(SqlType::Array { element }) => Some(element),
        _ => None,
    }
}

fn is_scalar(e: &SqlExpr, s: Scalar) -> bool {
    matches!(type_of(e), Some(SqlType::Scalar(t)) if *t == s)
}

/// Java's `isJson(e)`.
fn is_json(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Json)
}

/// Java's `isText(e)`.
fn is_text(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Varchar)
}

/// Java's `isRaise(e)`: a call of ERROR.
fn is_raise(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::Call { r#fn: SqlFn::Error, .. })
}

fn is_unnest(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::Call { r#fn: SqlFn::Unnest, .. })
}

/// `x` as Java's `(SqlExpr.Lambda) x` reads it.
fn lambda_arg(x: &SqlExpr) -> Result<&Lambda, Refusal> {
    match x {
        SqlExpr::Lambda(l) => Ok(l),
        other => Err(Refusal::class_cast(simple_name(other), "Lambda")),
    }
}

/// Java's `SqlExpr.Column.param(name, collection)`.
fn column_param(name: &str, collection: &SqlExpr) -> SqlExpr {
    match list_element(collection) {
        Some(element) => build::typed_column(None, name, element.clone(), true, Origin::Derived),
        None => build::derived(None, name),
    }
}

/// `params.get(i)`, refused past the end as Java's list read words it.
fn param_name(params: &[String], i: usize) -> Result<&str, Refusal> {
    params.get(i).map(String::as_str).ok_or_else(|| Refusal {
        class: Refusal::INDEX_OUT_OF_BOUNDS,
        message: format!("Index {i} out of bounds for length {}", params.len()),
    })
}

/// Java's `p.outputName()`: its alias, else a column's (or a stored read's) name.
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

/// A string literal's body (Java's `quoted`): a backslash and a quote escaped by a backslash.
fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Java's `Pattern.quote(s)`: `\Q...\E`, each `\E` inside closed and reopened.
fn pattern_quote(s: &str) -> String {
    if !s.contains("\\E") {
        return format!("\\Q{s}\\E");
    }
    let mut sb = String::from("\\Q");
    let mut current = 0;
    while let Some(at) = s[current..].find("\\E") {
        let at = current + at;
        sb.push_str(&s[current..at]);
        sb.push_str("\\E\\\\E\\Q");
        current = at + 2;
    }
    sb.push_str(&s[current..]);
    sb.push_str("\\E");
    sb
}

/// `e` with each unqualified reference to `name` replaced by `with` (Java's `substitute`): a node rebuilt through
/// `withChildren` only where a child changed; `None` where nothing did.
fn substitute(e: &SqlExpr, name: &str, with: &SqlExpr) -> Result<Option<SqlExpr>, Refusal> {
    if let SqlExpr::Column(c) = e
        && c.table.is_none()
        && c.name == name
    {
        return Ok(Some(with.clone()));
    }
    carrier::map_children(e, &mut |k| substitute(k, name, with))
}

/// `l` with its index parameter (DuckDB's, 1-based) read from Spark's 0-based one (Java's `oneBasedIndex`).
fn one_based_index(l: &Lambda) -> Result<Option<Lambda>, Refusal> {
    if l.params.len() < 2 {
        return Ok(None);
    }
    let index = &l.params[1];
    let zero = format!("{index}__0");
    let plus = build::call(SqlFn::Plus, vec![build::derived(None, &zero), build::int_lit(1)]);
    let body = substitute(&l.body, index, &plus)?.unwrap_or_else(|| l.body.as_ref().clone());
    Ok(Some(build::lambda_of(vec![l.params[0].clone(), zero], body)))
}

// ==================================================================
// The dialect's own helpers (Java's private methods)
// ==================================================================

impl Databricks {
    /// `a`'s `i`-th argument, written at `prec`, as a piece, read when written.
    fn arg_ex<'a>(&'a self, a: &'a [SqlExpr], i: usize, prec: i32) -> P<'a> {
        pc(move |w| {
            w.expr(self, arg(a, i)?, prec)?;
            Ok(())
        })
    }

    /// A list argument: the empty collection (a bare NULL, untyped) as a NULL array whose VOID elements coerce to any
    /// type (Java's `list(xs)`).
    fn list<'a>(&'a self, xs: &'a SqlExpr) -> P<'a> {
        if matches!(xs, SqlExpr::NullLit { .. }) {
            return text("CAST(NULL AS ARRAY<VOID>)");
        }
        self.ex(xs, 0)
    }

    /// A lambda parameter as its references spell it (Java's `param(name)`).
    fn param(&self, name: &str) -> Result<String, Refusal> {
        self.column_ref(&lite_sql::Column {
            table: None,
            name: name.to_owned(),
            r#type: TypeFact::Unknown,
            origin: Some(Origin::Derived),
        })
    }

    /// `(SELECT agg FROM explode(xs) AS __v(__e))`: a list reduced as rows; an empty or NULL list reduces over no rows
    /// (Java's `overRows`).
    fn over_rows<'a>(&'a self, xs: &'a SqlExpr, f: AggFn, extras: &[SqlExpr]) -> P<'a> {
        let e = if list_element(xs).is_none() { build::derived(None, "__e") } else { column_param("__e", xs) };
        let mut args = vec![e];
        args.extend(extras.iter().cloned());
        let r = build::reducer(f, args, false, Vec::new());
        let list = self.list(xs);
        pc(move |w| {
            w.append("(SELECT ");
            self.reducer(w, &r)?;
            w.append(" FROM explode(").piece(&*list)?.append(") AS `__v`(");
            let e = self.param("__e")?;
            w.append(&e).append("))");
            Ok(())
        })
    }

    /// A value as a VARIANT (Java's `toVariant`): a STRING a variant string; a struct, map or list converted
    /// recursively; a scalar cast.
    fn to_variant<'a>(&'a self, x: &'a SqlExpr) -> P<'a> {
        match type_of(x) {
            Some(SqlType::Scalar(Scalar::Json)) => self.ex(x, 0),
            Some(SqlType::Struct { .. } | SqlType::Map { .. } | SqlType::Array { .. }) => pc(move |w| {
                w.append("to_variant_object(").expr(self, x, 0)?.append(")");
                Ok(())
            }),
            _ => pc(move |w| {
                w.append("CAST(").expr(self, x, 0)?.append(" AS VARIANT)");
                Ok(())
            }),
        }
    }

    /// `s` split on the LITERAL text `d`: Spark's split takes a regex, so the delimiter is quoted (Java's `split`).
    fn split<'a>(&'a self, s: &'a SqlExpr, d: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            w.append("split(").expr(self, s, 0)?.append(", ");
            if let SqlExpr::StringLit { value, .. } = d {
                let lit = self.string_lit(&pattern_quote(value))?;
                w.append(&lit);
            } else {
                w.append(r"concat('\\Q', replace(").expr(self, d, 0)?.append(r", '\\E', '\\E\\\\E\\Q'), '\\E')");
            }
            w.append(", -1)");
            Ok(())
        })
    }

    /// A branch that IS the raise, cast to the CASE's own type (Java's `branch`).
    fn branch<'a>(&'a self, e: &'a SqlExpr, slot: Option<&SqlType>) -> Result<P<'a>, Refusal> {
        if is_raise(e)
            && let Some(slot) = slot
        {
            let t = cast_type_name(self, slot)?;
            return Ok(pc(move |w| {
                w.append("CAST(").expr(self, e, 0)?.append(" AS ").append(&t).append(")");
                Ok(())
            }));
        }
        Ok(self.ex(e, 0))
    }

    /// A projection's label: its alias, else the implicit one (Java's `p.alias() != null ? aliasIdent(p.alias()) :
    /// implicitLabel(p)`).
    fn label(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        let label = match &p.alias {
            Some(alias) => Some(self.alias_ident(alias)),
            None => self.implicit_label(p)?,
        };
        if let Some(label) = label {
            w.append(" AS ").append(&label);
        }
        Ok(())
    }

    /// `x op ALL|ANY (subquery)` as its SQL truth table over the subquery's one column (Java's `quantified`): Spark
    /// has no quantified comparison.
    fn quantified(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        comparison: SqlFn,
        quantifier: Quantifier,
        subquery: &SqlQuery,
    ) -> Result<(), Refusal> {
        let op = match comparison {
            SqlFn::Equal => " = ",
            SqlFn::NotEqual => " <> ",
            SqlFn::Less => " < ",
            SqlFn::LessEqual => " <= ",
            SqlFn::Greater => " > ",
            SqlFn::GreaterEqual => " >= ",
            other => return Err(Refusal::capability(format!("a quantified {other} reached Databricks"))),
        };
        // the subquery's one column named by a derived-column alias (its outputs may be undeclared)
        let column = "`__q`.`__c`";
        let all = quantifier == Quantifier::All;
        let cmp = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("(").expr(self, value, 5)?.append(op).append(column).append(")");
            Ok(())
        };
        w.append("(CASE WHEN EXISTS (SELECT 1 FROM (");
        ansi::inline(self, w, subquery)?;
        w.append(") AS `__q`(`__c`) WHERE ").append(if all { "NOT " } else { "" }).piece(&cmp)?.append(") THEN ");
        w.append(if all { "FALSE" } else { "TRUE" }).append(" WHEN EXISTS (SELECT 1 FROM (");
        ansi::inline(self, w, subquery)?;
        w.append(") AS `__q`(`__c`) WHERE ").piece(&cmp)?.append(" IS NULL) THEN NULL ELSE ");
        w.append(if all { "TRUE" } else { "FALSE" }).append(" END)");
        Ok(())
    }

    /// A list's variance or standard deviation as DuckDB computes it (Java's `moments`): its own update, in its order
    /// of operations, an aggregate over the present elements whose finish is the result.
    fn moments<'a>(present: P<'a>, f: AggFn) -> P<'a> {
        let x = "CAST(`__x` AS DOUBLE)";
        let count = "(`__a`.`n` + 1)";
        let mean = format!("(`__a`.`m` + ({x} - `__a`.`m`) / {count})");
        let step =
            format!("named_struct('n', {count}, 'm', {mean}, 'q', `__a`.`q` + ({x} - {mean}) * ({x} - `__a`.`m`))");
        let sample = matches!(f, AggFn::StddevSamp | AggFn::VarSamp | AggFn::Variance | AggFn::Stddev);
        let root = matches!(f, AggFn::StddevSamp | AggFn::StddevPop | AggFn::Stddev);
        let variance = if sample {
            "CASE WHEN `__a`.`n` > 1 THEN `__a`.`q` / (`__a`.`n` - 1) END"
        } else {
            "CASE WHEN `__a`.`n` > 1 THEN `__a`.`q` / `__a`.`n` WHEN `__a`.`n` = 1 THEN CAST(0 AS DOUBLE) END"
        };
        let finish = if root { format!("sqrt({variance})") } else { variance.to_owned() };
        pc(move |w| {
            w.append("aggregate(").piece(&*present)?;
            w.append(&format!(
                ", named_struct('n', CAST(0 AS DOUBLE), 'm', CAST(0 AS DOUBLE), 'q', CAST(0 AS DOUBLE)), (`__a`, \
                 `__x`) -> {step}, `__a` -> {finish})"
            ));
            Ok(())
        })
    }

    /// A pivot value as the platform names its column (Java's `pivotValueText`).
    fn pivot_value_text(v: &SqlExpr) -> Result<String, Refusal> {
        match v {
            SqlExpr::StringLit { value, .. } => Ok(value.clone()),
            SqlExpr::IntLit { value, .. } => Ok(value.to_string()),
            SqlExpr::BoolLit { value, .. } => Ok(value.to_string()),
            other => Err(Refusal::capability(format!(
                "a pivot value {} reached Databricks without a column name",
                simple_name(other)
            ))),
        }
    }
}

// ==================================================================
// The renderer (Java's overrides of AnsiSqlRenderer)
// ==================================================================

impl AnsiSqlRenderer for Databricks {
    fn lexicon(&self) -> &Lexicon {
        &LEXICON
    }

    fn ansi_state(&self) -> &AnsiState {
        &self.ansi
    }

    fn simple_name(&self) -> &'static str {
        "Databricks"
    }

    /// The engine's H2 text is refused here: Databricks has no translation of it.
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        let _ = statement;
        Err(Refusal::capability("the engine's H2 text reached Databricks, which has no translation of it"))
    }

    /// Native lists (the carrier strategies stand aside, as on DuckDB), the pivot's unqualified arguments, the
    /// checked defects as lists, the substring start clamp, averages delivered as DOUBLE, a constant GROUP BY or ORDER
    /// BY key as a typed expression, and the root's DECIMAL(p,s) and HUGEINT columns cast to their declared types.
    fn passes(&self) -> Vec<Box<dyn SqlRewriter>> {
        vec![
            Box::new(carrier::CarrierStrategies::new(CAPS)),
            Box::new(passes::UnqualifyPivotArgs),
            Box::new(passes::CheckedDefectsToLists),
            Box::new(passes::SubstringClamp),
            Box::new(shared_passes::AggregatesDeliverDouble::new(&[AggFn::Avg])),
            Box::new(passes::ConstantKeysAsExpressions::default()),
            Box::new(shared_passes::RootNumericTypes),
        ]
    }

    /// An as-of join's select as Spark runs it (its pick a QUALIFY); several lists exploded side by side as one
    /// `inline(arrays_zip(...))`, which aligns them as DuckDB's unnests are -- the form where every projection is such
    /// an unnest, any other mix refused by name.
    fn select(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        if matches!(s.from, SqlSource::Join { kind: JoinKind::AsofLeft, .. }) {
            let picked = as_of_pick(s.clone(), &AS_OF)?;
            return self.select(w, &picked, depth);
        }
        let unnests = s.projections.iter().filter(|p| is_unnest(&p.expr)).count();
        if unnests < 2 {
            return ansi::select(self, w, s, depth);
        }
        if unnests != s.projections.len()
            || !matches!(s.from, SqlSource::Dual)
            || s.r#where.is_some()
            || !s.group_by.is_empty()
            || s.qualify.is_some()
            || !s.order_by.is_empty()
            || s.limit.is_some()
            || s.offset.is_some()
            || s.distinct
        {
            return Err(Refusal::capability("several unnests beside other columns or clauses reached Databricks"));
        }
        w.append("SELECT inline(arrays_zip(").join(&s.projections, ", ", |w, p| {
            let list = self.list(unnest_list(&p.expr)?);
            w.piece(&*list)?;
            Ok(())
        })?;
        w.append(")) AS (").join(&s.projections, ", ", |w, p| {
            let name = output_name(p).ok_or_else(|| Refusal::null_pointer("an unnest's column name"))?;
            let a = self.alias_ident(&name);
            w.append(&a);
            Ok(())
        })?;
        w.append(")");
        Ok(())
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

    fn star_except_keyword(&self) -> Result<String, Refusal> {
        Ok("EXCEPT".to_owned())
    }

    fn row_order_column(&self) -> Result<String, Refusal> {
        Err(Refusal::capability("row order reached Databricks, which has no row-order column"))
    }

    fn as_of_join_clause(&self) -> Result<String, Refusal> {
        Err(Refusal::illegal_state("an as-of join reached the join clause: Databricks writes its select (asOf)"))
    }

    /// An inline table, `VALUES (..), (..) AS t(a, b)`: a parenthesized VALUES is a syntax error here.
    fn values_source(
        &self,
        w: &mut SqlWriter,
        rows: &[Vec<SqlExpr>],
        columns: &[String],
        alias: &str,
        outputs: &[OutputCol],
    ) -> Result<(), Refusal> {
        w.append("VALUES ");
        // a column of NULLs alone is VOID to Spark: its first row says its declared type
        let all_null =
            |c: usize| -> Result<bool, Refusal> {
                for row in rows {
                    if !matches!(arg(row, c)?, SqlExpr::NullLit { .. }) {
                        return Ok(false);
                    }
                }
                Ok(true)
            };
        for (r, row) in rows.iter().enumerate() {
            w.append(if r > 0 { ", (" } else { "(" });
            for (c, cell) in row.iter().enumerate() {
                w.append(if c > 0 { ", " } else { "" });
                if r == 0 && c < outputs.len() && all_null(c)? {
                    let t = cast_type_name(self, &outputs[c].r#type)?;
                    w.append("CAST(NULL AS ").append(&t).append(")");
                } else {
                    w.expr(self, cell, 0)?;
                }
            }
            w.append(")");
        }
        let a = self.alias_ident(alias);
        let names: Vec<String> = columns.iter().map(|c| self.alias_ident(c)).collect();
        w.append(" AS ").append(&a).append("(").append(&names.join(", ")).append(")");
        Ok(())
    }

    /// Native PIVOT over the source; each value's column is named `value__|__agg`, as the platform names it: with one
    /// aggregate the value's alias is the whole name, with several Spark joins the value's alias and the aggregate's
    /// with `_`, so the aggregate's carries the tail.
    fn pivot_source(&self, w: &mut SqlWriter, pivot: &SqlSource, depth: usize) -> Result<(), Refusal> {
        let SqlSource::Pivot { source, on, r#in, named, usings, alias, .. } = pivot else {
            unreachable!("a pivot")
        };
        // a dynamic pivot a cast names pivots on the named values
        let values = if r#in.is_empty() { named } else { r#in };
        if values.is_empty() {
            return Err(Refusal::illegal_state(
                "a dynamic pivot reached Databricks: needsStaticPivot discovers its values",
            ));
        }
        if on.len() != 1 {
            return Err(Refusal::capability(format!("a pivot on {} columns reached Databricks", on.len())));
        }
        let one = usings.len() == 1;
        w.append("(SELECT * FROM ");
        self.source(w, source, depth)?;
        w.append(" PIVOT (").join(usings, ", ", |w, u| {
            self.reducer(w, &u.agg)?;
            let name = self.ident(&if one { u.alias.clone() } else { format!("_|__{}", u.alias) });
            w.append(" AS ").append(&name);
            Ok(())
        })?;
        w.append(" FOR ");
        match &on[0] {
            SqlExpr::Column(c) => {
                let d = self.delimited(&c.name);
                w.append(&d);
            }
            other => {
                w.expr(self, other, 0)?;
            }
        }
        w.append(" IN (").join(values, ", ", |w, v| {
            w.expr(self, v, 0)?.append(" AS ");
            let text = Databricks::pivot_value_text(v)?;
            let name = self.ident(&if one { format!("{text}__|__{}", usings[0].alias) } else { text });
            w.append(&name);
            Ok(())
        })?;
        let a = self.ident(alias);
        w.append("))) AS ").append(&a);
        Ok(())
    }

    // ---- lexical: identifiers, literals, types

    /// Every identifier in backticks; a double-quoted declaration (`"date"`) is its inner name.
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

    /// A backslash and a quote escaped by a backslash; a NUL spliced as `chr(0)` between quoted segments.
    fn string_lit(&self, value: &str) -> Result<String, Refusal> {
        if value.contains('\0') {
            let parts: Vec<String> = value.split('\0').map(quoted).collect();
            return Ok(format!("({})", parts.join(" || chr(0) || ")));
        }
        Ok(quoted(value))
    }

    fn array_type_name(&self, element: &str) -> Result<String, Refusal> {
        Ok(format!("ARRAY<{element}>"))
    }

    fn map_type_name(&self, key: &str, value: &str) -> String {
        format!("MAP<{key}, {value}>")
    }

    fn struct_type_name(&self, fields: &[StructField]) -> Result<String, Refusal> {
        let fields = ansi::struct_fields(self, fields)?;
        let mut sb = String::from("STRUCT<");
        for (i, (name, t)) in fields.iter().enumerate() {
            if i > 0 {
                sb.push_str(", ");
            }
            sb.push_str(name);
            sb.push_str(": ");
            sb.push_str(t);
        }
        sb.push('>');
        Ok(sb)
    }

    /// Alias-less projections label EXPLICITLY from the declared output: Spark labels an expression by its text.
    fn implicit_label(&self, p: &Projection) -> Result<Option<String>, Refusal> {
        match &p.out {
            Some(out) => Ok(Some(self.alias_ident(&out.name))),
            None => ansi::implicit_label(self, p),
        }
    }

    /// A list of unknown element type exploded is an array of VOID to Spark: its elements typed by the column's
    /// declared type; a value that can only be NULL (bottom-typed, or a raise) spelled in its declared slot's type.
    fn projection(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        if let Some(out) = &p.out
            && let SqlExpr::Call { r#fn: SqlFn::Unnest, args, .. } = &p.expr
            && args.len() == 1
            && list_element(&args[0]).is_none()
        {
            w.append("explode(CAST(").piece(&*self.list(&args[0]))?.append(" AS ");
            let t = self.array_type_name(&cast_type_name(self, &out.r#type)?)?;
            w.append(&t).append("))");
            return self.label(w, p);
        }
        // (never a generator: Spark refuses explode inside an expression); a raise is VOID too
        if let Some(out) = &p.out
            && (matches!(p.expr.r#type(), TypeFact::Bottom) || is_raise(&p.expr))
            && !is_unnest(&p.expr)
        {
            w.append("CAST(").expr(self, &p.expr, 0)?.append(" AS ");
            let t = cast_type_name(self, &out.r#type)?;
            w.append(&t).append(")");
            return self.label(w, p);
        }
        ansi::projection(self, w, p)
    }

    /// A branch that IS the raise is cast to the CASE's own type: raise_error is VOID.
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
            b(w)?;
        }
        if let Some(o) = otherwise {
            w.append(" ELSE ");
            let b = self.branch(o, slot)?;
            b(w)?;
        }
        w.append(" END");
        Ok(())
    }

    fn expr(&self, w: &mut SqlWriter, e: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        match e {
            // a BIGINT: a bare literal is a 32-bit INT, whose arithmetic overflows at 2^31
            SqlExpr::IntLit { value, .. } => {
                w.append(&format!("{value}L"));
                Ok(())
            }
            SqlExpr::OrderedListAgg { value, order_by, .. } => {
                let keys = vec![aggregates::Key { key: self.ex(order_by, 0), ascending: true, nulls_first: false }];
                self.collected(self.ex(value, 0), keys)(w)
            }
            SqlExpr::Quantified { value, comparison, quantifier, subquery, .. } => {
                self.quantified(w, value, *comparison, *quantifier, subquery)
            }
            other => ansi::expr(self, w, other, parent_prec),
        }
    }

    // ---- lists: native arrays with lambdas

    fn array_lit(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("array(").list(self, elements)?.append(")");
        Ok(())
    }

    /// A NULL member is VOID to Spark, which a list function over it refuses: a composite member's NULL is written in
    /// its declared type.
    fn struct_lit(&self, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
        w.append("named_struct(").join(fields, ", ", |w, f| {
            let name = self.string_lit(&f.name)?;
            w.append(&name).append(", ");
            if matches!(f.value, SqlExpr::NullLit { .. })
                && let Some(declared @ (SqlType::Array { .. } | SqlType::Struct { .. } | SqlType::Map { .. })) =
                    &f.declared
            {
                let t = cast_type_name(self, declared)?;
                w.append("CAST(NULL AS ").append(&t).append(")");
            } else {
                w.expr(self, &f.value, 0)?;
            }
            Ok(())
        })?;
        w.append(")");
        Ok(())
    }

    fn struct_get(&self, w: &mut SqlWriter, source: &SqlExpr, field: &str, r#type: &TypeFact) -> Result<(), Refusal> {
        let _ = r#type;
        w.append("(").expr(self, source, 8)?.append(").");
        let f = self.ident(field);
        w.append(&f);
        Ok(())
    }

    fn lambda(&self, w: &mut SqlWriter, l: &Lambda) -> Result<(), Refusal> {
        let head = if l.params.len() == 1 {
            self.param(&l.params[0])?
        } else {
            let ps = l.params.iter().map(|p| self.param(p)).collect::<Result<Vec<_>, _>>()?;
            format!("({})", ps.join(", "))
        };
        w.append(&head).append(" -> ").expr(self, &l.body, 0)?;
        Ok(())
    }

    fn list_call(&self, w: &mut SqlWriter, f: SqlFn, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list_call_arm(w, f, args)
    }

    fn list_exists(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("coalesce(exists(").piece(&*self.list(arg(args, 0)?))?.append(", ").expr(self, arg(args, 1)?, 0)?;
        w.append("), FALSE)");
        Ok(())
    }

    fn list_for_all(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("coalesce(forall(").piece(&*self.list(arg(args, 0)?))?.append(", ").expr(self, arg(args, 1)?, 0)?;
        w.append("), TRUE)");
        Ok(())
    }

    fn all_distinct(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let list = self.list(arg(args, 0)?);
        w.append("coalesce(array_size(array_distinct(").piece(&*list)?.append(")) = array_size(").piece(&*list)?;
        w.append("), TRUE)");
        Ok(())
    }

    fn membership(&self, w: &mut SqlWriter, needle: &SqlExpr, collection: &SqlExpr) -> Result<(), Refusal> {
        w.append("array_contains(").piece(&*self.list(collection))?.append(", ").expr(self, needle, 0)?.append(")");
        Ok(())
    }

    /// A list reduced: by a higher-order function where one says it exactly, else as rows. NULL elements are
    /// skipped, as an aggregate skips them.
    fn reduce_collection(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        collection: &SqlExpr,
        extras: &[SqlExpr],
    ) -> Result<(), Refusal> {
        let xs = self.list(collection);
        let present: P<'_> = {
            let xs = xs.clone();
            pc(move |w| {
                w.append("filter(").piece(&*xs)?.append(", `__x` -> `__x` IS NOT NULL)");
                Ok(())
            })
        };
        match reducer {
            // NULL over no present element, as string_agg over no row
            AggFn::StringAgg if extras.len() == 1 => {
                w.append("(CASE WHEN array_size(").piece(&*present)?.append(") = 0 THEN NULL ELSE array_join(");
                w.append("transform(").piece(&*present)?.append(", `__x` -> CAST(`__x` AS STRING)), ");
                w.expr(self, &extras[0], 0)?.append(") END)");
            }
            AggFn::Min => {
                w.append("array_min(").piece(&*xs)?.append(")");
            }
            AggFn::Max => {
                w.append("array_max(").piece(&*xs)?.append(")");
            }
            AggFn::Count => {
                w.append("array_size(").piece(&*present)?.append(")");
            }
            AggFn::StddevSamp
            | AggFn::StddevPop
            | AggFn::VarSamp
            | AggFn::VarPop
            | AggFn::Variance
            | AggFn::Stddev => {
                w.piece(&*Databricks::moments(present, reducer))?;
            }
            _ => {
                w.piece(&*self.over_rows(collection, reducer, extras))?;
            }
        }
        Ok(())
    }

    fn unnest_projection(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        if args.len() != 1 {
            return Err(Refusal::capability(format!("an unnest of {} lists reached Databricks", args.len())));
        }
        w.append("explode(").piece(&*self.list(&args[0]))?.append(")");
        Ok(())
    }

    /// Pure's fold, `(element, acc)`, as Spark's `aggregate(xs, init, (acc, x) -> ...)`: its accumulator may be any
    /// type, a list included; an empty or NULL list folds to the init.
    fn fold_call(&self, w: &mut SqlWriter, fold: &SqlExpr) -> Result<(), Refusal> {
        let SqlExpr::FoldCall { source, lambda, init, r#type, .. } = fold else {
            unreachable!("a fold")
        };
        let TypeFact::Typed { r#type: t, .. } = r#type else {
            return Err(Refusal::capability("a fold of unknown type reached Databricks"));
        };
        let ty = cast_type_name(self, t)?;
        let elem = self.param(param_name(&lambda.params, 0)?)?;
        let acc = self.param(param_name(&lambda.params, 1)?)?;
        w.append("aggregate(coalesce(").piece(&*self.list(source))?.append(", array()), CAST(").expr(self, init, 0)?;
        w.append(&format!(" AS {ty}), ({acc}, {elem}) -> CAST(")).expr(self, &lambda.body, 0)?;
        w.append(&format!(" AS {ty}))"));
        Ok(())
    }

    /// Pure's splitPart: the non-empty tokens, 1-based, NULL past the end.
    fn split_part_call(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let split = self.split(arg(a, 0)?, arg(a, 1)?);
        w.append("try_element_at(filter(").piece(&*split)?.append(", `__t` -> `__t` <> ''), CAST(");
        w.expr(self, arg(a, 2)?, 0)?.append(" AS INT))");
        Ok(())
    }

    /// A pure collection holds no empties: `array_compact` drops the NULL elements.
    fn compact_list(&self, w: &mut SqlWriter, list: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        let _ = parent_prec;
        w.append("array_compact(").piece(&*self.list(list))?.append(")");
        Ok(())
    }

    fn struct_insert(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let st = type_of(arg(args, 0)?);
        let (Some(SqlType::Struct { fields }), SqlExpr::StringLit { value: name, .. }) = (st, arg(args, 1)?) else {
            return Err(Refusal::capability("struct_insert over an untyped struct reached Databricks"));
        };
        w.append("named_struct(");
        let mut first = true;
        for f in fields {
            if f.name == *name {
                continue;
            }
            let lit = self.string_lit(&f.name)?;
            w.append(if first { "" } else { ", " }).append(&lit).append(", (").expr(self, &args[0], 8)?;
            let n = self.ident(&f.name);
            w.append(").").append(&n);
            first = false;
        }
        let lit = self.string_lit(name)?;
        w.append(if first { "" } else { ", " }).append(&lit).append(", ").expr(self, arg(args, 2)?, 0)?.append(")");
        Ok(())
    }

    // ---- variant (VARIANT, Databricks' own semi-structured type)

    fn variant_construct(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() != 1 {
            return Err(Refusal::capability(format!("toVariant of {} arguments reached Databricks", a.len())));
        }
        self.to_variant(&a[0])(w)
    }

    /// A key (a member, or NULL) or a 0-based index (-1 the last, past the end NULL) of a variant: through a cast to
    /// a map or an array of variants, NULL when it is not one.
    fn variant_get(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let key = arg(args, 1)?;
        if let SqlExpr::StringLit { value, .. } = key
            && value == "$"
        {
            w.expr(self, arg(args, 0)?, 0)?;
            return Ok(());
        }
        if is_text(key) || matches!(key, SqlExpr::StringLit { .. }) {
            w.append("try_element_at(try_cast(").expr(self, arg(args, 0)?, 0)?.append(" AS MAP<STRING, VARIANT>), ");
            w.expr(self, key, 0)?.append(")");
            return Ok(());
        }
        w.append("try_element_at(try_cast(").expr(self, arg(args, 0)?, 0)?;
        w.append(" AS ARRAY<VARIANT>), CAST(CASE WHEN ");
        w.expr(self, key, 0)?.append(" >= 0 THEN ").expr(self, key, 6)?.append(" + 1 ELSE ").expr(self, key, 0)?;
        w.append(" END AS INT))");
        Ok(())
    }

    fn variant_elements(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let a0 = arg(args, 0)?;
        if list_element(a0).is_some() {
            w.expr(self, a0, 0)?;
            return Ok(());
        }
        w.append("try_cast(").expr(self, a0, 0)?.append(" AS ARRAY<VARIANT>)");
        Ok(())
    }

    fn variant_aware_cast(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        let target_is = |s: Scalar| matches!(target, SqlType::Scalar(t) if *t == s);
        if target_is(Scalar::TemporalText) || target_is(Scalar::DecimalText) {
            return ansi::variant_aware_cast(self, w, value, target, conform);
        }
        if target_is(Scalar::Json) {
            if is_text(value) {
                w.append("parse_json(").expr(self, value, 0)?.append(")");
                return Ok(());
            }
            return self.to_variant(value)(w);
        }
        // a Float's text: Spark spells the exponent E, DuckDB e, which the lowering reads
        if target_is(Scalar::Varchar) && (is_scalar(value, Scalar::Double) || matches!(value, SqlExpr::FloatLit { .. }))
        {
            w.append("lower(CAST(").expr(self, value, 0)?.append(" AS STRING))");
            return Ok(());
        }
        if is_json(value) {
            // a member's value as text is its text; the whole value as text is its JSON, as DuckDB's cast
            let member = matches!(value, SqlExpr::Call { r#fn: SqlFn::VariantGet, .. });
            if target_is(Scalar::Varchar) && !member {
                w.append("to_json(").expr(self, value, 0)?.append(")");
                return Ok(());
            }
            if matches!(target, SqlType::Array { .. } | SqlType::Map { .. }) {
                w.append("try_cast(").expr(self, value, 0)?.append(" AS ");
                let t = cast_type_name(self, target)?;
                w.append(&t).append(")");
                return Ok(());
            }
        }
        ansi::variant_aware_cast(self, w, value, target, conform)
    }

    // ---- scalar functions: every SqlFn decided in `functions.rs`

    fn call(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact, parent_prec: i32) -> Result<(), Refusal> {
        self.call_arm(w, f, a, t, parent_prec)
    }

    // ---- the hooks of the base, the aggregates and windows: `aggregates.rs`

    /// Pure's hashCode is a signed 64-bit integer: xxhash64 is one.
    fn hash_signed(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("xxhash64(").list(self, a)?.append(")");
        Ok(())
    }

    /// Pure round is half-EVEN: bround.
    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("bround(").expr(self, arg(a, 0)?, 0)?;
        if a.len() > 1 {
            w.append(", CAST(").expr(self, &a[1], 0)?.append(" AS INT)");
        }
        w.append(")");
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
        w.append("to_variant_object(named_struct(").list(self, kv)?.append("))");
        Ok(())
    }

    fn json_array(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("to_variant_object(array(").join(elements, ", ", |w, e| self.to_variant(e)(w))?.append("))");
        Ok(())
    }

    fn json_array_agg(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        order_keys: &[JsonArrayAggKey],
    ) -> Result<(), Refusal> {
        let keys = order_keys
            .iter()
            .map(|k| aggregates::Key { key: self.ex(&k.expr, 0), ascending: !k.desc, nulls_first: false })
            .collect();
        w.append("to_variant_object(").piece(&*self.collected(self.to_variant(value), keys))?.append(")");
        Ok(())
    }

    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal> {
        Ok(aggregates::format_text(parts))
    }

    /// This database's name for each scalar type, a refusal where it has none.
    fn scalar_type_name(&self, s: Scalar) -> Result<String, Refusal> {
        Ok(match s {
            Scalar::Boolean => "BOOLEAN",
            Scalar::Integer => "INT",
            Scalar::Bigint => "BIGINT",
            Scalar::Hugeint => "DECIMAL(38,0)",
            Scalar::Double => "DOUBLE",
            Scalar::Varchar | Scalar::TemporalText => "STRING",
            Scalar::Date => "DATE",
            Scalar::Timestamp | Scalar::Timestamptz => "TIMESTAMP",
            Scalar::Json => "VARIANT",
            // Databricks SQL has no TIME type
            Scalar::Time => return Err(Refusal::capability("a time of day: Databricks has no TIME type")),
            Scalar::DecimalText => {
                return Err(Refusal::illegal_state(format!("{s} cast reached a dialect without {s} support")));
            }
        }
        .to_owned())
    }

    // ---- DDL

    /// Spark DDL: a nullable column says nothing (an explicit NULL is a syntax error).
    fn ddl_nullability(&self, not_null: bool) -> String {
        if not_null { " NOT NULL" } else { "" }.to_owned()
    }

    /// Spark DDL: STRING for an unbounded VARCHAR, BOOLEAN for BIT.
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
                ColumnKind::Timestamp => "TIMESTAMP",
                ColumnKind::Date => "DATE",
                ColumnKind::Json => "JSON",
                ColumnKind::Varchar => "STRING",
                // a time of day: Databricks has no TIME type; a nanosecond timestamp: its TIMESTAMP keeps microseconds
                ColumnKind::Time
                | ColumnKind::TimestampNs
                | ColumnKind::Other
                | ColumnKind::Distinct
                | ColumnKind::Array
                | ColumnKind::Object => {
                    return Err(Refusal::illegal_state(format!("no DDL spelling for declared column type {kind}")));
                }
            }
            .to_owned(),
            ColumnType::Sized { kind, size } => format!("{kind}({size})"),
            ColumnType::Scaled { kind, precision, scale } => format!("{kind}({precision}, {scale})"),
        })
    }

    // ---- values: what the driver hands back as JSON text

    /// Its JSON_TYPE answers DuckDB's names (its emulation).
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

    /// A DATE is `date` (Spark's typeof); no other type's name is compared here.
    fn typeof_name(&self, scalar: Scalar) -> Result<String, Refusal> {
        if scalar == Scalar::Date {
            return Ok("date".to_owned());
        }
        Err(Refusal::capability(format!("a TYPEOF compared with {scalar}'s name reached Databricks")))
    }
}

#[cfg(test)]
mod tests;

// ==================================================================
// SqlDialect
// ==================================================================

impl crate::dialect::SqlDialect for Databricks {
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
        <Databricks as AnsiSqlRenderer>::reads_stored(t)
    }

    fn table_name_spelled(&self, name: &str, quoting: NameQuoting) -> String {
        AnsiSqlRenderer::table_name(self, name, quoting)
    }

    /// The platform's naive-UTC temporal contract, and ANSI mode: overflow, a bad cast and a division by zero raise,
    /// as DuckDB's do.
    fn session_setup(&self) -> Vec<String> {
        vec!["SET TIME ZONE 'UTC'".to_owned(), "SET ANSI_MODE = true".to_owned()]
    }

    /// A session's read-only table is a TEMPORARY VIEW over its rows (a temporary TABLE is not read inside an EXISTS),
    /// one statement; each column cast to its declared type in the view's projection, so a column whose cells are all
    /// NULL, or a view with no rows, is still typed.
    fn session_table(&self, table: &SqlDdl, rows: Option<&SqlDml>) -> Result<Vec<String>, Refusal> {
        let SqlDdl::CreateTable { table: name, quoting, columns, temporary, .. } = table else {
            unreachable!("the seam hands a session table a createTable")
        };
        if !temporary {
            return Err(Refusal::illegal_argument(format!("a session table is temporary: {name}")));
        }
        let rows = match rows {
            None => None,
            Some(SqlDml::InsertValues { rows, .. }) => Some(rows),
            Some(_) => unreachable!("the seam hands a session table's rows an insertValues"),
        };
        let mut w = SqlWriter::new();
        let stored = AnsiSqlRenderer::stored_name(self, name, quoting.table);
        w.append("CREATE TEMPORARY VIEW ").append(&stored).append(" AS SELECT ");
        for (c, col) in columns.iter().enumerate() {
            w.append(if c > 0 { ", " } else { "" }).append("CAST(");
            match rows {
                None => w.append("NULL"),
                Some(_) => w.append(&format!("`c{c}`")),
            };
            let ty = self.ddl_type(&col.r#type)?;
            let id = self.ddl_identifier(&col.name, col.declared_quoted);
            w.append(&format!(" AS {ty}) AS ")).append(&id);
        }
        match rows {
            None => {
                w.append(" WHERE FALSE");
            }
            Some(rows) => {
                w.append(" FROM VALUES ");
                for (r, row) in rows.iter().enumerate() {
                    w.append(if r == 0 { "(" } else { ", (" }).list(self, row)?.append(")");
                }
                w.append(" AS `t`(");
                for c in 0..columns.len() {
                    w.append(if c > 0 { ", `c" } else { "`c" }).append(&format!("{c}`"));
                }
                w.append(")");
            }
        }
        let view = w.into_statement()?;
        if !view.binds.is_empty() {
            return Err(Refusal::capability(format!(
                "plan parameters {} in a session table's rows: a row holds values, and a DDL statement is text",
                list_text(&view.binds)
            )));
        }
        Ok(vec![view.sql])
    }

    /// No multi-statement transaction.
    fn keys_snapshot(&self) -> Result<KeysSnapshot, Refusal> {
        Ok(KeysSnapshot::None)
    }

    /// No result column limit documented.
    fn result_column_limit(&self) -> Result<Option<i32>, Refusal> {
        Ok(None)
    }

    /// Native PIVOT takes its values from the query: a dynamic pivot is discovered first.
    fn needs_static_pivot(&self) -> bool {
        true
    }
}
