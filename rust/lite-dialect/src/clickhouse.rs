// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `ClickHouse` (`core/src/main/java/com/legend/sql/dialect/ClickHouse.java`), the ClickHouse 25.8 execution
//! dialect (its Memory engine): the base renderer with every identifier double-quoted, SQL's semantics as SETTINGS
//! every root statement carries (`join_use_nulls`, `union_default_mode`, `cast_keep_nullable`, the UTC zone, no merge
//! of a subquery's expressions, and the correlated subqueries' setting on exactly where each one's filter is
//! null-rejecting), every cast to a `Nullable(T)`, a list never NULL (the empty collection the empty array), an
//! integer literal an `Int64` and a Float literal an exact `Decimal`, a lateral explosion as an `ARRAY JOIN`, an outer
//! join's one-sided or residual condition re-keyed, a lateral filter as a join, and -- as its [`crate::SqlDialect`] --
//! no session, no keys snapshot, no column limit, a static pivot, and its own wire type names. Every `SqlFn` and
//! aggregate is decided by an exhaustive match: `src/clickhouse/functions.rs` (`call`, `listCall` and the list, variant
//! and temporal helpers), `src/clickhouse/aggregates.rs` (the hooks of the base, the aggregates and windows, the JSON
//! constructors, the date formats and the DDL types) and `src/clickhouse/passes.rs` (the two passes no other dialect
//! installs: `ArrayJoinColumnsNamed` and `OuterJoinResidualsAsMatches`).
//!
//! Java's two per-render counters (`nesting`, `correlatedExists`) are [`Cell`]s of the dialect object, reset exactly
//! where Java resets them; the seam makes a fresh dialect per call, as the oracles' fresh JVM dialects. Java's
//! `SqlWriter.Piece` is [`P`], the shared closure of [`crate::pieces`] (as Databricks').

use crate::ansi::{self, cast_type_name, query_outputs};
use crate::dialect::KeysSnapshot;
use crate::explosion_markers::{self, is_marker, marker_column, unnest_list};
use crate::pieces::{Ex, P, pc, text};
use crate::refusal::{Refusal, arg};
use crate::renderer::{AnsiSqlRenderer, AnsiState, Lexicon};
use crate::rewriter::{Rw, SqlRewriter};
use crate::statement::RenderedStatement;
use crate::typing::{source_alias, unscaled};
use crate::writer::SqlWriter;
use crate::{build, carrier, passes, shared_passes};
use lite_sql::{
    AggFn, ColumnType, DateFmt, Frame, JoinKind, JsonArrayAggKey, JsonKind, Lambda, NameQuoting, Origin, OutputCol,
    Projection, Quantifier, Reducer, Scalar, SortKey, SqlAgg, SqlDdl, SqlDml, SqlExpr, SqlFn, SqlQuery, SqlSelect,
    SqlSource, SqlType, StructField, StructLitField, TypeFact, When,
};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

mod aggregates;
mod functions;
pub(crate) mod passes_own;

/// The settings every statement carries (Java's `SETTINGS`).
pub const SETTINGS: &str = "SETTINGS join_use_nulls = 1, union_default_mode = 'DISTINCT', cast_keep_nullable = 1, \
                            session_timezone = 'UTC', query_plan_merge_expressions = 0";

/// The correlated subqueries' setting on (Java's `CORRELATED`) and off (`UNCORRELATED`).
pub const CORRELATED: &str = ", allow_experimental_correlated_subqueries = 1";
pub const UNCORRELATED: &str = ", allow_experimental_correlated_subqueries = 0";

/// The ClickHouse dialect: one per render, as Java's `new ClickHouse()`.
#[derive(Debug, Default)]
pub struct ClickHouse {
    ansi: AnsiState,
    /// Java's `nesting`: how deep the writer is in `query` (the root writes the SETTINGS clause).
    nesting: Cell<usize>,
    /// Java's `correlatedExists`: whether the root statement being written holds a correlated subquery.
    correlated_exists: Cell<bool>,
}

impl ClickHouse {
    /// Java's `new ClickHouse()`.
    pub fn new() -> ClickHouse {
        ClickHouse::default()
    }
}

/// ClickHouse's words (Java's `LEXICON`'s). ClickHouse quotes every identifier, so this list decides no spelling.
pub const RESERVED: &[&str] = &[
    "all", "and", "any", "array", "as", "asc", "asof", "between", "by", "case", "cast", "cross", "desc", "distinct",
    "else", "end", "except", "exists", "false", "final", "format", "from", "full", "global", "group", "having", "in",
    "inner", "interval", "into", "is", "join", "left", "like", "limit", "not", "null", "offset", "on", "or", "order",
    "outer", "prewhere", "qualify", "right", "sample", "select", "settings", "then", "true", "union", "using", "when",
    "where", "window", "with",
];

/// Java's `ClickHouse.LEXICON`: double quotes and [`RESERVED`].
static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| Lexicon::new('"', RESERVED));

/// Java's `ClickHouse.CAPS`: typed native arrays with lambdas; FULL OUTER, the as-of join and QUALIFY native.
pub const CAPS: carrier::Caps =
    carrier::Caps { native_lists: true, correlated_explode: true, json_carrier: true, native_full_outer: true };

/// A list of no element type (Java's `EMPTY_UNTYPED`).
const EMPTY_UNTYPED: &str = "CAST([] AS Array(Nullable(Nothing)))";

/// The comparisons a null-rejecting conjunct may be (Java's `COMPARISONS`).
const COMPARISONS: &[SqlFn] =
    &[SqlFn::Equal, SqlFn::NotEqual, SqlFn::Less, SqlFn::LessEqual, SqlFn::Greater, SqlFn::GreaterEqual];

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

/// `typeOf(e) == SqlType.Scalar.s`.
fn is_scalar(e: &SqlExpr, s: Scalar) -> bool {
    matches!(type_of(e), Some(SqlType::Scalar(t)) if *t == s)
}

fn is_scalar_type(t: &SqlType, s: Scalar) -> bool {
    matches!(t, SqlType::Scalar(x) if *x == s)
}

/// Java's `isRaise(e)`: a call of ERROR.
fn is_raise(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::Call { r#fn: SqlFn::Error, .. })
}

/// `x` as Java's `(SqlExpr.Lambda) x` reads it.
fn lambda_arg(x: &SqlExpr) -> Result<&Lambda, Refusal> {
    match x {
        SqlExpr::Lambda(l) => Ok(l),
        other => Err(Refusal::class_cast(crate::java::simple_name(other), "Lambda")),
    }
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

/// A BigDecimal's text as its precision and scale (Java's `precision()`, `scale()`).
fn precision_scale(text: &str) -> (i64, i64) {
    let (_, digits, scale) = unscaled(text);
    (digits.len() as i64, scale)
}

/// `new BigDecimal(text).stripTrailingZeros().scale()` (zero's is 0).
fn stripped_scale(text: &str) -> i64 {
    let (_, digits, scale) = unscaled(text);
    if digits == "0" {
        return 0;
    }
    let zeros = digits.len() - digits.trim_end_matches('0').len();
    scale - zeros as i64
}

/// Java's `String.matches("[0-9]+")`.
fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// The aliases `src` defines: each side of a join, else its own (Java's `aliasesOf`; a Dual's `alias()` refuses).
fn aliases_of(src: &SqlSource, into: &mut HashSet<String>) -> Result<(), Refusal> {
    if let SqlSource::Join { left, right, .. } = src {
        aliases_of(left, into)?;
        aliases_of(right, into)
    } else {
        into.insert(source_alias(src)?.to_owned());
        Ok(())
    }
}

/// The table aliases an expression's columns read (Java's `TablesRead.of`: the rewriter's whole walk, its
/// subqueries included; an unqualified column reads none).
fn tables_read(e: &SqlExpr) -> Result<HashSet<String>, Refusal> {
    let mut tables = HashSet::new();
    shared_passes::walk(e.clone(), &mut |x| {
        if let SqlExpr::Column(c) = &x
            && let Some(t) = &c.table
        {
            tables.insert(t.clone());
        }
        Ok(Rw::Same(x))
    })?;
    Ok(tables)
}

/// `own.containsAll(TablesRead.of(e))`.
fn reads_only(e: &SqlExpr, own: &HashSet<String>) -> Result<bool, Refusal> {
    Ok(tables_read(e)?.iter().all(|t| own.contains(t)))
}

/// `e`'s conjuncts: an AND's operands, each flattened; any other expression is its own one (Java's `conjuncts`).
fn conjuncts(e: &SqlExpr) -> Vec<&SqlExpr> {
    match e {
        SqlExpr::Call { r#fn: SqlFn::And, args, .. } => args.iter().flat_map(conjuncts).collect(),
        other => vec![other],
    }
}

/// A select that only filters its source: no grouping, ordering, limit, distinct or window qualification.
fn filter_only(s: &SqlSelect) -> bool {
    s.group_by.is_empty()
        && s.having.is_none()
        && s.qualify.is_none()
        && s.order_by.is_empty()
        && s.limit.is_none()
        && s.offset.is_none()
        && !s.distinct
}

/// A column `src` defines (its first output; a join's from its left side) (Java's `anyColumnOf`).
fn any_column_of(src: &SqlSource) -> Result<SqlExpr, Refusal> {
    match src {
        SqlSource::Join { left, .. } => any_column_of(left),
        SqlSource::Values { columns, alias, .. } => {
            let first = columns.first().ok_or_else(|| Refusal {
                class: Refusal::INDEX_OUT_OF_BOUNDS,
                message: format!("Index 0 out of bounds for length {}", columns.len()),
            })?;
            Ok(build::derived(Some(alias), first))
        }
        SqlSource::Subselect { inner, alias, .. } if !query_outputs(inner).is_empty() => {
            Ok(SqlExpr::Column(build::column_of(Some(alias), &query_outputs(inner)[0])))
        }
        SqlSource::Table { alias, outputs, .. } if !outputs.is_empty() => {
            Ok(SqlExpr::Column(build::column_of(Some(alias), &outputs[0])))
        }
        other => Err(Refusal::capability(format!(
            "a join reading one side only, to a {} of undeclared columns, reached ClickHouse",
            source_class(other)
        ))),
    }
}

/// Does `s` read a column of a source it does not define (a correlated subquery)? (Java's `correlated`.)
fn correlated(s: &SqlSelect) -> Result<bool, Refusal> {
    let mut own = HashSet::new();
    aliases_of(&s.from, &mut own)?;
    let mut read = HashSet::new();
    if let Some(w) = &s.r#where {
        read.extend(tables_read(w)?);
    }
    for p in &s.projections {
        read.extend(tables_read(&p.expr)?);
    }
    Ok(!read.iter().all(|t| own.contains(t)))
}

/// Whether `e` is NULL wherever an outer column it reads is (Java's `nullRejecting`).
fn null_rejecting(e: &SqlExpr, own: &HashSet<String>) -> Result<bool, Refusal> {
    if reads_only(e, own)? || matches!(e, SqlExpr::Column(_)) {
        return Ok(true);
    }
    let SqlExpr::Call { r#fn, args, .. } = e else {
        return Ok(false);
    };
    if args.len() != 2 || !COMPARISONS.contains(r#fn) {
        return Ok(false);
    }
    for a in args {
        if !(reads_only(a, own)? || matches!(a, SqlExpr::Column(_))) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether a correlated subquery's answer is right here: it has a filter, and each of its conjuncts is
/// null-rejecting (Java's `correlatedRight`).
fn correlated_right(s: &SqlSelect) -> Result<bool, Refusal> {
    let mut own = HashSet::new();
    aliases_of(&s.from, &mut own)?;
    let Some(w) = &s.r#where else {
        return Ok(false);
    };
    for x in conjuncts(w) {
        if !null_rejecting(x, &own)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// EXISTS a row of `s` under its filter `w` and `cond` (none: `w` alone) (Java's `someRow`).
fn some_row(s: &SqlSelect, w: &SqlExpr, cond: Option<SqlExpr>) -> Result<SqlExpr, Refusal> {
    let filter = match cond {
        None => w.clone(),
        Some(c) => build::call(SqlFn::And, vec![w.clone(), c]),
    };
    let one = build::projection(build::int_lit(1), Some("__one".to_owned()), None)?;
    let q = build::with_projections(build::with_where(s.clone(), Some(filter))?, vec![one])?;
    Ok(build::exists(build::query(q)))
}

/// An inner column read as the lateral subquery's own output of it (Java's `InnerAsOutput`).
fn inner_as_output(
    e: &SqlExpr,
    inner: &HashSet<String>,
    output_of: &HashMap<String, String>,
    alias: &str,
) -> Result<SqlExpr, Refusal> {
    shared_passes::walk(e.clone(), &mut |x| {
        if let SqlExpr::Column(c) = &x
            && let Some(t) = &c.table
            && inner.contains(t)
        {
            let Some(out) = output_of.get(&format!("{t}.{}", c.name)) else {
                return Err(Refusal::capability(format!(
                    "a lateral filter on a column its subquery does not output reached ClickHouse, which has no \
                     LATERAL: {t}.{}",
                    c.name
                )));
            };
            return Ok(Rw::New(match &c.r#type {
                TypeFact::Typed { r#type, nullable } => {
                    build::typed_column(Some(alias), out, r#type.clone(), *nullable, Origin::Derived)
                }
                _ => build::derived(Some(alias), out),
            }));
        }
        Ok(Rw::Same(x))
    })
}

/// A date's text, refused outside ClickHouse's dates, 1900 to 2299 (Java's `inRange`; its year `^(-?\d+)-`, read by
/// `Integer.parseInt`).
fn in_range(iso: &str) -> Result<&str, Refusal> {
    let bytes = iso.as_bytes();
    let start = usize::from(bytes.first() == Some(&b'-'));
    let digits = bytes[start..].iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || bytes.get(start + digits) != Some(&b'-') {
        return Err(Refusal::illegal_state(format!("a date literal with no year reached ClickHouse: {iso}")));
    }
    let group = &iso[..start + digits];
    let year: i32 = group
        .parse()
        .map_err(|_| Refusal { class: Refusal::NUMBER_FORMAT, message: format!("For input string: \"{group}\"") })?;
    if !(1900..=2299).contains(&year) {
        return Err(Refusal::capability(format!(
            "the date {iso} reached ClickHouse, whose dates are 1900 to 2299 (it clamps a date past either end)"
        )));
    }
    Ok(iso)
}

/// `CAST('d' AS Decimal(p, s))` for its own precision and scale, at most 76 digits (Java's `decimal`).
fn decimal(text: &str) -> Result<String, Refusal> {
    let (precision, scale) = precision_scale(text);
    let scale = scale.max(0);
    let precision = precision.max(scale + 1);
    if precision > 76 {
        return Err(Refusal::capability(format!(
            "a decimal literal of {precision} digits reached ClickHouse, whose Decimal holds 76"
        )));
    }
    Ok(format!("CAST('{}' AS Decimal({precision}, {scale}))", crate::java::big_decimal_plain(text)))
}

/// A struct field's spelled type: a Tuple's is its Variant, anything else itself (Java's `fieldTypeName`).
fn field_type_name(t: &str) -> String {
    if t.starts_with("Tuple(") { format!("Variant({t})") } else { t.to_owned() }
}

fn not_nullable(t: &str) -> &str {
    if t.starts_with("Nullable(") && t.ends_with(')') { &t[9..t.len() - 1] } else { t }
}

/// A struct field's type: its value's, or the type its class declares (Java's `fieldType`).
fn field_type(f: &StructLitField) -> Result<&SqlType, Refusal> {
    type_of(&f.value).or(f.declared.as_ref()).ok_or_else(|| {
        Refusal::capability("a struct field of unknown type reached ClickHouse: a Tuple's fields are typed")
    })
}

/// Whether `e` is a struct-typed field of an instance (by name), held as its Variant (Java's `variantField`).
fn variant_field(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::StructGet { field, r#type: TypeFact::Typed { r#type: SqlType::Struct { .. }, .. }, .. }
        if !all_digits(field))
}

/// An unnest of the empty list of no element type, filtered or not (Java's `unnestOfUntypedEmpty`).
fn unnest_of_untyped_empty(e: &SqlExpr) -> bool {
    let SqlExpr::Call { r#fn: SqlFn::Unnest, args, .. } = e else {
        return false;
    };
    if args.len() != 1 {
        return false;
    }
    let mut xs = &args[0];
    loop {
        match xs {
            SqlExpr::CompactList { list, .. } => xs = list,
            SqlExpr::Call { r#fn: SqlFn::ListFilter, args, .. } => match args.first() {
                Some(first) => xs = first,
                None => return false,
            },
            _ => break,
        }
    }
    match xs {
        SqlExpr::ArrayLit { elements, .. } => elements.is_empty(),
        SqlExpr::NullLit { .. } => list_element(xs).is_none(),
        _ => false,
    }
}

/// Whether a value is a Float literal or Float-typed member beside a Decimal one (Java's `unified`'s test).
fn needs_unifying(members: &[SqlExpr]) -> bool {
    let dbl = members.iter().any(|e| is_scalar(e, Scalar::Double));
    let dec = members.iter().any(|e| matches!(type_of(e), Some(SqlType::Decimal { .. })));
    dbl && dec
}

// ==================================================================
// The dialect's own helpers (Java's private methods)
// ==================================================================

impl ClickHouse {
    /// A list argument: the empty collection (a NULL) as the empty array -- of its element's type when known (Java's
    /// `list(xs)`).
    fn list<'a>(&'a self, xs: &'a SqlExpr) -> P<'a> {
        if let SqlExpr::NullLit { .. } = xs {
            return match list_element(xs) {
                None => text(EMPTY_UNTYPED),
                Some(element) => pc(move |w| {
                    let t = self.array_type_name(&cast_type_name(self, element)?)?;
                    w.append(&format!("CAST([] AS {t})"));
                    Ok(())
                }),
            };
        }
        self.ex(xs, 0)
    }

    /// `xs` less its NULL elements (Java's `present`).
    fn present<'a>(&'a self, xs: &'a SqlExpr) -> P<'a> {
        let list = self.list(xs);
        pc(move |w| {
            w.append("arrayFilter(\"__x\" -> isNotNull(\"__x\"), ").piece(&*list)?.append(")");
            Ok(())
        })
    }

    /// A list an ARRAY JOIN explodes: the empty one of no element type is the empty array of the exploded column's
    /// type (Java's `exploded`).
    fn exploded<'a>(&'a self, xs: &'a SqlExpr, column: &Projection) -> Result<P<'a>, Refusal> {
        let untyped_empty = match xs {
            SqlExpr::ArrayLit { elements, .. } => elements.is_empty() && list_element(xs).is_none(),
            SqlExpr::NullLit { .. } => list_element(xs).is_none(),
            _ => false,
        };
        let t = type_of(&column.expr).or(column.out.as_ref().map(|o| &o.r#type));
        if untyped_empty
            && let Some(t) = t
            && !matches!(t, SqlType::Array { .. })
        {
            let empty = format!("CAST([] AS {})", self.array_type_name(&cast_type_name(self, t)?)?);
            return Ok(text(empty));
        }
        Ok(self.list(xs))
    }

    /// Members written to share a type: where a Float64 and a Decimal meet, each Decimal member is its Float64 (Java's
    /// `unified`).
    fn unified_member(&self, w: &mut SqlWriter, e: &SqlExpr, unify: bool) -> Result<(), Refusal> {
        if unify && matches!(type_of(e), Some(SqlType::Decimal { .. })) {
            w.append("toFloat64(").expr(self, e, 0)?.append(")");
        } else {
            w.expr(self, e, 0)?;
        }
        Ok(())
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

    /// A lambda's arrays: the list, and its 1-based positions when the lambda takes the index (Java's
    /// `lambdaArrays`).
    fn lambda_arrays<'a>(&'a self, xs: &'a SqlExpr, l: &Lambda) -> P<'a> {
        let list = self.list(xs);
        if l.params.len() > 1 {
            return pc(move |w| {
                w.piece(&*list)?.append(", arrayEnumerate(").piece(&*list)?.append(")");
                Ok(())
            });
        }
        list
    }

    /// A branch that IS the raise, as the CASE's own type (Java's `branch`).
    fn branch<'a>(&'a self, e: &'a SqlExpr, slot: Option<&SqlType>) -> Result<P<'a>, Refusal> {
        if is_raise(e)
            && let Some(slot @ SqlType::Array { .. }) = slot
        {
            let t = cast_type_name(self, slot)?;
            return Ok(pc(move |w| {
                w.append(&format!("arrayResize(CAST([] AS {t}), ")).expr(self, e, 0)?.append(")");
                Ok(())
            }));
        }
        if is_raise(e)
            && let Some(slot) = slot
        {
            let t = cast_type_name(self, slot)?;
            return Ok(pc(move |w| {
                w.append("CAST(").expr(self, e, 0)?.append(&format!(" AS {t})"));
                Ok(())
            }));
        }
        Ok(self.ex(e, 0))
    }

    /// EXISTS; over a select of constants, the rows' own columns selected instead (Java's `exists`).
    fn exists(&self, w: &mut SqlWriter, ex: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        let SqlExpr::Exists { subquery, .. } = ex else { unreachable!("an EXISTS") };
        if let SqlQuery::Select(q) = subquery.as_ref()
            && q.group_by.is_empty()
            && q.having.is_none()
            && !q.distinct
            && !matches!(q.from, SqlSource::Dual)
            && !q.projections.is_empty()
            && q.projections.iter().all(|p| matches!(p.expr, SqlExpr::IntLit { .. } | SqlExpr::BoolLit { .. }))
        {
            w.append("EXISTS (");
            let star = build::select(SqlSelect {
                projections: Vec::new(),
                distinct: false,
                from: q.from.clone(),
                r#where: q.r#where.clone(),
                group_by: q.group_by.clone(),
                having: q.having.clone(),
                qualify: q.qualify.clone(),
                order_by: q.order_by.clone(),
                limit: q.limit,
                offset: q.offset,
                outputs: Vec::new(),
            })?;
            ansi::inline(self, w, &build::query(star))?;
            w.append(")");
            return Ok(());
        }
        ansi::expr(self, w, ex, parent_prec)
    }

    /// A projection's label: its alias, else the implicit one.
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

    /// A date literal's spelling (Java's `dateLit`).
    fn date_text(&self, iso: &str) -> Result<String, Refusal> {
        Ok(format!("toDate32('{}')", in_range(iso)?))
    }

    /// `IN (correlated subquery)` as SQL's three-valued IN by EXISTS (Java's `expr` InSubquery arm).
    fn in_correlated(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        s: &SqlSelect,
        parent_prec: i32,
    ) -> Result<(), Refusal> {
        let c = &s.projections[0].expr;
        let mut own = HashSet::new();
        aliases_of(&s.from, &mut own)?;
        let refuse = || {
            Refusal::capability(
                "a correlated subquery as IN's argument reached ClickHouse, whose correlated subqueries answer false \
                 for an outer row with a NULL (measured 25.8): its filter reads the outer row other than as a \
                 comparison's operand",
            )
        };
        let Some(filter) = &s.r#where else {
            return Err(refuse());
        };
        if !reads_only(c, &own)? || !null_rejecting(value, &own)? {
            return Err(refuse());
        }
        for x in conjuncts(filter) {
            if !null_rejecting(x, &own)? {
                return Err(refuse());
            }
        }
        // true where a row matches; else NULL where a row is NULL, or v is and a row is there; else false
        let three_valued = build::case(
            vec![
                build::when(
                    some_row(s, filter, Some(build::call(SqlFn::Equal, vec![c.clone(), value.clone()])))?,
                    build::bool_lit(true),
                ),
                build::when(some_row(s, filter, Some(build::call(SqlFn::IsNull, vec![c.clone()])))?, build::null_lit()),
                build::when(
                    build::call(
                        SqlFn::And,
                        vec![build::call(SqlFn::IsNull, vec![value.clone()]), some_row(s, filter, None)?],
                    ),
                    build::null_lit(),
                ),
            ],
            Some(build::bool_lit(false)),
        );
        self.correlated_exists.set(true);
        self.expr(w, &three_valued, parent_prec)
    }

    /// A lateral explosion marker as an ARRAY JOIN of its lists (Java's `source`, its first branch; the caller has
    /// refused a join condition).
    fn array_join(
        &self,
        w: &mut SqlWriter,
        left: &SqlSource,
        kind: JoinKind,
        marker: &SqlSelect,
        sub_alias: &str,
        depth: usize,
    ) -> Result<(), Refusal> {
        let ps = &marker.projections;
        let lists = ps.iter().map(|p| unnest_list(&p.expr)).collect::<Result<Vec<_>, _>>()?;
        self.source(w, left, depth)?;
        self.nl(w, depth);
        w.append(if kind == JoinKind::LeftLateral { "LEFT ARRAY JOIN " } else { "ARRAY JOIN " });
        let n = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("greatest(").join(&lists, ", ", |v, l| {
                v.append("length(").piece(&*self.list(l))?.append(")");
                Ok(())
            })?;
            w.append(")");
            Ok(())
        };
        for (i, p) in ps.iter().enumerate() {
            let l = lists[i];
            w.append(if i > 0 { ", " } else { "" });
            if lists.len() == 1 {
                w.piece(&*self.exploded(l, p)?)?;
            } else {
                w.append("arrayResize(arrayMap(\"__z\" -> toNullable(\"__z\"), ");
                w.piece(&*self.exploded(l, p)?)?.append("), ").piece(&n)?.append(", NULL)");
            }
            let name = self.alias_ident(&format!("{sub_alias}.{}", marker_column(p)?));
            w.append(" AS ").append(&name);
        }
        Ok(())
    }

    /// An as-of join whose ON is its inequality alone, keyed by its operands' hashes times zero (Java's `source`,
    /// its second branch).
    fn as_of_keyed(
        &self,
        w: &mut SqlWriter,
        left: &SqlSource,
        right: &SqlSource,
        on: &SqlExpr,
        depth: usize,
    ) -> Result<(), Refusal> {
        // the inequality whose operands read the two sides, where there is one (a stable sort, the first kept)
        let mut candidates = Vec::new();
        for c in conjuncts(on) {
            if let SqlExpr::Call {
                r#fn: SqlFn::Less | SqlFn::LessEqual | SqlFn::Greater | SqlFn::GreaterEqual,
                args,
                ..
            } = c
            {
                let a = tables_read(arg(args, 0)?)?;
                let b = tables_read(arg(args, 1)?)?;
                let key = if a.is_empty() || b.is_empty() || a == b { 1 } else { 0 };
                candidates.push((key, args));
            }
        }
        candidates.sort_by_key(|(key, _)| *key);
        let Some((_, args)) = candidates.first() else {
            return Err(Refusal::capability(
                "an as-of join with neither an equality nor an inequality reached ClickHouse",
            ));
        };
        self.source(w, left, depth)?;
        self.nl(w, depth);
        let clause = self.as_of_join_clause()?;
        w.append(&clause).append(" ");
        self.source(w, right, depth)?;
        w.append(" ON cityHash64(").expr(self, &args[0], 0)?.append(") * 0 = cityHash64(");
        w.expr(self, &args[1], 0)?.append(") * 0 AND ").expr(self, on, 0)?;
        Ok(())
    }

    /// A lateral subquery that only filters its source by the row before it, as that source joined on the filter
    /// (Java's `source`, its fourth branch): false when it is no such subquery.
    #[allow(clippy::too_many_arguments)]
    fn lateral_filter(
        &self,
        w: &mut SqlWriter,
        left: &SqlSource,
        kind: JoinKind,
        on: Option<&SqlExpr>,
        s: &SqlSelect,
        sub_alias: &str,
        depth: usize,
    ) -> Result<bool, Refusal> {
        let Some(filter) = &s.r#where else {
            return Ok(false);
        };
        let mut inner = HashSet::new();
        aliases_of(&s.from, &mut inner)?;
        let mut local = Vec::new();
        let mut correlated = Vec::new();
        for c in conjuncts(filter) {
            if reads_only(c, &inner)? { local.push(c.clone()) } else { correlated.push(c) }
        }
        // its own columns only
        let columns = s
            .projections
            .iter()
            .all(|p| matches!(&p.expr, SqlExpr::Column(c) if c.table.as_ref().is_none_or(|t| inner.contains(t))));
        if correlated.is_empty() || !columns {
            return Ok(false);
        }
        let mut output_of: HashMap<String, String> = HashMap::new();
        if s.projections.is_empty() && inner.len() == 1 {
            // SELECT * over one source: each of its columns is output under its own name
            let only = inner.iter().next().expect("one alias");
            for o in &s.outputs {
                output_of.entry(format!("{only}.{}", o.name)).or_insert_with(|| o.name.clone());
            }
        }
        for p in &s.projections {
            if let SqlExpr::Column(c) = &p.expr
                && let Some(t) = &c.table
            {
                let name = match (&p.alias, &p.out) {
                    (Some(alias), _) => alias.clone(),
                    (None, Some(out)) => out.name.clone(),
                    (None, None) => return Err(Refusal::null_pointer("a projection's output")),
                };
                output_of.entry(format!("{t}.{}", c.name)).or_insert(name);
            }
        }
        let mut joined = Vec::with_capacity(correlated.len() + 1);
        for c in correlated {
            joined.push(inner_as_output(c, &inner, &output_of, sub_alias)?);
        }
        if let Some(on) = on
            && !matches!(on, SqlExpr::BoolLit { value: true, .. })
        {
            joined.push(on.clone());
        }
        let filtered_where = match local.len() {
            0 => None,
            1 => local.pop(),
            _ => Some(build::call(SqlFn::And, local)),
        };
        let filtered = build::with_where(s.clone(), filtered_where)?;
        self.source(w, left, depth)?;
        self.nl(w, depth);
        w.append(if kind == JoinKind::LeftLateral { "LEFT OUTER JOIN " } else { "INNER JOIN " });
        self.subselect_source(w, &build::query(filtered), sub_alias, depth)?;
        let condition = if joined.len() == 1 { joined.pop().expect("one") } else { build::call(SqlFn::And, joined) };
        w.append(" ON ").expr(self, &condition, 0)?;
        Ok(true)
    }
}

// ==================================================================
// The renderer (Java's overrides of AnsiSqlRenderer)
// ==================================================================

impl AnsiSqlRenderer for ClickHouse {
    fn lexicon(&self) -> &Lexicon {
        &LEXICON
    }

    fn ansi_state(&self) -> &AnsiState {
        &self.ansi
    }

    fn simple_name(&self) -> &'static str {
        "ClickHouse"
    }

    /// The engine's H2 text is refused here: ClickHouse has no translation of it.
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        let _ = statement;
        Err(Refusal::capability("the engine's H2 text reached ClickHouse, which has no translation of it"))
    }

    /// Native lists (the carrier strategies stand aside); a static pivot as a grouped CASE aggregation; an outer
    /// join's residual as its matches; the lateral explosions as markers, a star over one unwrapped, their columns
    /// named after a star; the checked defects as lists; the substring start clamp; averages delivered as DOUBLE; a
    /// constant GROUP BY or ORDER BY key as a typed expression; the root's DECIMAL(p,s) and HUGEINT columns cast to
    /// their declared types.
    fn passes(&self) -> Vec<Box<dyn SqlRewriter>> {
        vec![
            Box::new(carrier::CarrierStrategies::new(CAPS)),
            Box::new(shared_passes::PivotAsGroupBy),
            Box::new(passes_own::OuterJoinResidualsAsMatches::default()),
            Box::new(explosion_markers::LateralUnnestAsMarker),
            Box::new(explosion_markers::StarOverExplosionUnwrapped),
            Box::new(passes_own::ArrayJoinColumnsNamed),
            Box::new(passes::CheckedDefectsToLists),
            Box::new(passes::SubstringClamp),
            Box::new(shared_passes::AggregatesDeliverDouble::new(&[AggFn::Avg])),
            Box::new(passes::ConstantKeysAsExpressions::default()),
            Box::new(shared_passes::RootNumericTypes),
        ]
    }

    /// The root statement closes with its SETTINGS clause (the writer's one statement).
    fn query(&self, w: &mut SqlWriter, q: &SqlQuery, depth: usize) -> Result<(), Refusal> {
        let root = self.nesting.get() == 0;
        if root {
            self.correlated_exists.set(false);
        }
        self.nesting.set(self.nesting.get() + 1);
        let written = ansi::query(self, w, q, depth);
        self.nesting.set(self.nesting.get() - 1);
        written?;
        if root {
            self.nl(w, 0);
            w.append(SETTINGS).append(if self.correlated_exists.get() { CORRELATED } else { UNCORRELATED });
        }
        Ok(())
    }

    /// Several lists exploded in one select align, as DuckDB's unnests do: one ARRAY JOIN of the lists zipped.
    fn select(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        self.select_arm(w, s, depth)
    }

    fn source(&self, w: &mut SqlWriter, src: &SqlSource, depth: usize) -> Result<(), Refusal> {
        let SqlSource::Join { left, right, kind, on, .. } = src else {
            return ansi::source(self, w, src, depth);
        };
        let on = on.as_deref();
        let lateral = matches!(kind, JoinKind::CrossLateral | JoinKind::LeftLateral);
        // a lateral explosion MARKER is an ARRAY JOIN of its lists
        if let SqlSource::Subselect { inner, alias, .. } = right.as_ref()
            && let SqlQuery::Select(marker) = inner.as_ref()
            && is_marker(inner)
            && lateral
        {
            if let Some(on) = on
                && !matches!(on, SqlExpr::BoolLit { value: true, .. })
            {
                return Err(Refusal::capability("a lateral explosion with a join condition reached ClickHouse"));
            }
            return self.array_join(w, left, *kind, marker, alias, depth);
        }
        // an as-of join whose ON is its inequality alone gets an equality that always holds where it can
        if *kind == JoinKind::AsofLeft
            && let Some(on) = on
            && !conjuncts(on).iter().any(|c| matches!(c, SqlExpr::Call { r#fn: SqlFn::Equal, .. }))
        {
            return self.as_of_keyed(w, left, right, on, depth);
        }
        // an outer join whose ON reads one side only is keyed by an equality that holds exactly where it does
        if let Some(on) = on
            && matches!(kind, JoinKind::Left | JoinKind::Right | JoinKind::Full)
            && !matches!(on, SqlExpr::BoolLit { .. })
        {
            let mut lefts = HashSet::new();
            let mut rights = HashSet::new();
            aliases_of(left, &mut lefts)?;
            aliases_of(right, &mut rights)?;
            let read = tables_read(on)?;
            let left_only = !read.is_empty() && read.iter().all(|t| lefts.contains(t));
            let right_only = !read.is_empty() && read.iter().all(|t| rights.contains(t));
            if left_only || right_only {
                let other = any_column_of(if left_only { right } else { left })?;
                let cond = |w: &mut SqlWriter| -> Result<(), Refusal> {
                    w.append("if(").expr(self, on, 0)?.append(", 0, 1)");
                    Ok(())
                };
                let zero = |w: &mut SqlWriter| -> Result<(), Refusal> {
                    w.append("coalesce(cityHash64(").expr(self, &other, 0)?.append("), 0) * 0");
                    Ok(())
                };
                self.source(w, left, depth)?;
                self.nl(w, depth);
                let keyword = self.join_keyword(*kind)?;
                w.append(&keyword).append(" ");
                self.source(w, right, depth)?;
                w.append(" ON ");
                if left_only {
                    w.piece(&cond)?.append(" = ").piece(&zero)?;
                } else {
                    w.piece(&zero)?.append(" = ").piece(&cond)?;
                }
                return Ok(());
            }
        }
        // a lateral subquery that only FILTERS its source by the row before it is that source joined on the filter
        if lateral
            && let SqlSource::Subselect { inner, alias, .. } = right.as_ref()
            && let SqlQuery::Select(s) = inner.as_ref()
            && s.r#where.is_some()
            && filter_only(s)
            && self.lateral_filter(w, left, *kind, on, s, alias, depth)?
        {
            return Ok(());
        }
        // a null-safe equality between a join's two sides is a join key only as isNotDistinctFrom
        if let Some(on) = on
            && matches!(kind, JoinKind::Inner | JoinKind::Left)
        {
            let keys = conjuncts(on);
            let all_keys =
                keys.iter().all(|c| matches!(c, SqlExpr::Call { r#fn: SqlFn::NullSafeEqual | SqlFn::Equal, .. }));
            if all_keys && keys.iter().any(|c| matches!(c, SqlExpr::Call { r#fn: SqlFn::NullSafeEqual, .. })) {
                self.source(w, left, depth)?;
                self.nl(w, depth);
                let keyword = self.join_keyword(*kind)?;
                w.append(&keyword).append(" ");
                self.source(w, right, depth)?;
                w.append(" ON ");
                for (i, key) in keys.iter().enumerate() {
                    w.append(if i > 0 { " AND " } else { "" });
                    match key {
                        SqlExpr::Call { r#fn: SqlFn::NullSafeEqual, args, .. } => {
                            w.append("isNotDistinctFrom(").expr(self, arg(args, 0)?, 0)?.append(", ");
                            w.expr(self, arg(args, 1)?, 0)?.append(")");
                        }
                        other => {
                            w.append("(").expr(self, other, 0)?.append(")");
                        }
                    }
                }
                return Ok(());
            }
        }
        if lateral {
            return Err(Refusal::capability(
                "a LATERAL join reached ClickHouse, which has none (a source correlated with the one before it)",
            ));
        }
        ansi::source(self, w, src, depth)
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

    /// An inline table: the `values` table function, its structure the columns' nullable types.
    fn values_source(
        &self,
        w: &mut SqlWriter,
        rows: &[Vec<SqlExpr>],
        columns: &[String],
        alias: &str,
        outputs: &[OutputCol],
    ) -> Result<(), Refusal> {
        if outputs.len() != columns.len() {
            return Err(Refusal::capability("a VALUES source without a declared type per column reached ClickHouse"));
        }
        let mut structure = String::new();
        for (i, column) in columns.iter().enumerate() {
            // a Decimal widened to its literals' largest scale
            let declared = &outputs[i].r#type;
            let t = match declared {
                SqlType::Decimal { scale: d_scale, .. } => {
                    let mut scale = *d_scale;
                    for row in rows {
                        if let Some(SqlType::Decimal { scale: cell, .. }) = type_of(arg(row, i)?) {
                            scale = scale.max(*cell);
                        }
                    }
                    if scale == *d_scale { declared.clone() } else { SqlType::Decimal { precision: 38, scale } }
                }
                other => other.clone(),
            };
            let name = cast_type_name(self, &t)?;
            structure.push_str(if i > 0 { ", " } else { "" });
            structure.push('`');
            structure.push_str(&column.replace('`', "\\`"));
            structure.push_str("` ");
            structure.push_str(&name);
        }
        let lit = self.string_lit(&structure)?;
        w.append("values(").append(&lit);
        for row in rows {
            w.append(", (").list(self, row)?.append(")");
        }
        let a = self.alias_ident(alias);
        w.append(") AS ").append(&a);
        Ok(())
    }

    fn row_order_column(&self) -> Result<String, Refusal> {
        Err(Refusal::capability("row order reached ClickHouse, which has no row-order column"))
    }

    /// ClickHouse's own as-of join.
    fn as_of_join_clause(&self) -> Result<String, Refusal> {
        Ok("ASOF LEFT JOIN".to_owned())
    }

    fn pivot_source(&self, w: &mut SqlWriter, pivot: &SqlSource, depth: usize) -> Result<(), Refusal> {
        let _ = (w, pivot, depth);
        Err(Refusal::illegal_state(
            "a pivot reached ClickHouse's writer: PivotAsGroupBy writes it as a grouped aggregation (a dynamic pivot \
             is discovered first)",
        ))
    }

    // ---- lexical: identifiers, literals, types

    /// Every identifier quoted; a double-quoted declaration is its own spelling.
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

    /// A backslash and a quote escaped by a backslash, a NUL as `\0`.
    fn string_lit(&self, value: &str) -> Result<String, Refusal> {
        let mut sb = String::with_capacity(value.len() + 2);
        sb.push('\'');
        for c in value.chars() {
            match c {
                '\\' => sb.push_str("\\\\"),
                '\'' => sb.push_str("\\'"),
                '\0' => sb.push_str("\\0"),
                other => sb.push(other),
            }
        }
        sb.push('\'');
        Ok(sb)
    }

    fn date_lit(&self, iso: &str) -> Result<String, Refusal> {
        self.date_text(iso)
    }

    fn time_lit(&self, iso: &str) -> Result<String, Refusal> {
        let _ = iso;
        Err(Refusal::capability("a time of day: ClickHouse's Time type is experimental"))
    }

    /// A naive UTC date-time of six digits (finer ones cut); a `T` is a space.
    fn timestamp_lit(&self, iso: &str) -> Result<String, Refusal> {
        let t = iso.replacen('T', " ", 1);
        // by UTF-16 unit, a digit as Java's Character.isDigit says
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
        Ok(format!("toDateTime64('{}', 6, 'UTC')", in_range(&t)?))
    }

    /// A Float literal is an exact Decimal of its own digits; NaN and the infinities are Float64.
    fn float_literal(&self, v: f64) -> Result<String, Refusal> {
        if v.is_nan() {
            return Ok("nan".to_owned());
        }
        if v.is_infinite() {
            return Ok(if v > 0.0 { "inf" } else { "-inf" }.to_owned());
        }
        match crate::java::float_decimal(v)? {
            Some(plain) => decimal(&plain),
            None => Ok(format!("toFloat64('{}')", lite_javacompat::double_to_string(v))),
        }
    }

    fn huge_int_literal(&self, v: &lite_sql::BigDecimal) -> String {
        format!("toInt128('{}')", crate::java::big_decimal_plain(v.as_str()))
    }

    fn array_type_name(&self, element: &str) -> Result<String, Refusal> {
        Ok(format!("Array({element})"))
    }

    /// A map's key is never nullable.
    fn map_type_name(&self, key: &str, value: &str) -> String {
        format!("Map({}, {value})", not_nullable(key))
    }

    /// A Tuple; a field that is itself a struct is its Variant.
    fn struct_type_name(&self, fields: &[StructField]) -> Result<String, Refusal> {
        let fields = ansi::struct_fields(self, fields)?;
        let mut sb = String::from("Tuple(");
        for (i, (name, t)) in fields.iter().enumerate() {
            sb.push_str(if i > 0 { ", " } else { "" });
            sb.push_str(name);
            sb.push(' ');
            sb.push_str(&field_type_name(t));
        }
        sb.push(')');
        Ok(sb)
    }

    /// Alias-less projections label EXPLICITLY from the declared output.
    fn implicit_label(&self, p: &Projection) -> Result<Option<String>, Refusal> {
        match &p.out {
            Some(out) => Ok(Some(self.alias_ident(&out.name))),
            None => ansi::implicit_label(self, p),
        }
    }

    /// A projection whose declared slot is a Boolean is a Bool; one that can only be NULL, or a raise, is its slot's
    /// nullable type.
    fn projection(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        if let Some(out) = &p.out
            && ((is_scalar_type(&out.r#type, Scalar::Boolean) && !matches!(p.expr, SqlExpr::Column(_)))
                || matches!(p.expr.r#type(), TypeFact::Bottom)
                || is_raise(&p.expr)
                || (matches!(p.expr, SqlExpr::NullLit { .. }) && !matches!(out.r#type, SqlType::Array { .. })))
        {
            if unnest_of_untyped_empty(&p.expr) && !matches!(out.r#type, SqlType::Array { .. }) {
                let t = self.array_type_name(&cast_type_name(self, &out.r#type)?)?;
                w.append(&format!("arrayJoin(CAST([] AS {t}))"));
            } else {
                w.append("CAST(").expr(self, &p.expr, 0)?.append(" AS ");
                let t = cast_type_name(self, &out.r#type)?;
                w.append(&t).append(")");
            }
            return self.label(w, p);
        }
        ansi::projection(self, w, p)
    }

    /// A branch that IS the raise is cast to the CASE's own type.
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
            // an Int64: a bare literal is typed by its magnitude
            SqlExpr::IntLit { value, .. } => {
                w.append(&format!("toInt64({value})"));
                Ok(())
            }
            // the empty list whose element type is known is that type's empty array
            SqlExpr::ArrayLit { elements, .. } if elements.is_empty() && list_element(e).is_some() => {
                let element = list_element(e).expect("an element type");
                let t = self.array_type_name(&cast_type_name(self, element)?)?;
                w.append(&format!("CAST([] AS {t})"));
                Ok(())
            }
            // a correlated subquery as IN's argument: SQL's three-valued IN of v, by EXISTS
            SqlExpr::InSubquery { value, subquery, .. }
                if matches!(subquery.as_ref(), SqlQuery::Select(s) if s.projections.len() == 1)
                    && correlated_of(subquery)? =>
            {
                let SqlQuery::Select(s) = subquery.as_ref() else { unreachable!("a select") };
                self.in_correlated(w, value, s, parent_prec)
            }
            // a quantified comparison over no rows is SQL's ALL true and ANY false
            SqlExpr::Quantified { quantifier, subquery, .. } => {
                if let SqlQuery::Select(s) = subquery.as_ref()
                    && correlated(s)?
                {
                    if !correlated_right(s)? {
                        return Err(Refusal::capability(
                            "a correlated quantified comparison reached ClickHouse, whose correlated subqueries \
                             answer false for an outer row with a NULL (measured 25.8): its filter reads the outer \
                             row other than as a comparison's operand",
                        ));
                    }
                    self.correlated_exists.set(true);
                }
                w.append("(CASE WHEN (SELECT count() FROM (");
                ansi::inline(self, w, subquery)?;
                w.append(") AS \"__qa\") = 0 THEN ");
                w.append(if *quantifier == Quantifier::All { "true" } else { "false" }).append(" ELSE ");
                ansi::expr(self, w, e, 0)?;
                w.append(" END)");
                Ok(())
            }
            // a correlated EXISTS: right only where its filter is null-rejecting
            SqlExpr::Exists { subquery, .. } => {
                if let SqlQuery::Select(q) = subquery.as_ref()
                    && correlated(q)?
                {
                    if !correlated_right(q)? {
                        return Err(Refusal::capability(
                            "a correlated EXISTS reached ClickHouse, whose correlated subqueries answer false for an \
                             outer row with a NULL (measured 25.8): its filter reads the outer row other than as a \
                             comparison's operand",
                        ));
                    }
                    self.correlated_exists.set(true);
                }
                self.exists(w, e, parent_prec)
            }
            SqlExpr::DecimalLit { value, r#type }
                if !matches!(r#type, TypeFact::Typed { r#type: SqlType::Scalar(Scalar::Hugeint), .. }) =>
            {
                let text = decimal(value.as_str())?;
                w.append(&text);
                Ok(())
            }
            SqlExpr::OrderedListAgg { value, order_by, .. } => {
                let keys = vec![aggregates::Key {
                    key: self.ex(order_by, 0),
                    ascending: true,
                    nulls_first: false,
                    r#type: type_of(order_by),
                }];
                self.collected(self.ex(value, 0), keys, None)(w)
            }
            other => ansi::expr(self, w, other, parent_prec),
        }
    }

    // ---- lists: native arrays with lambdas; never NULL

    fn array_lit(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        if elements.is_empty() {
            w.append(EMPTY_UNTYPED);
            return Ok(());
        }
        let unify = needs_unifying(elements);
        w.append("[").join(elements, ", ", |w, e| self.unified_member(w, e, unify))?.append("]");
        Ok(())
    }

    fn struct_lit(&self, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
        let mut types = Vec::with_capacity(fields.len());
        for f in fields {
            types.push(field_type_name(&cast_type_name(self, field_type(f)?)?));
        }
        w.append("CAST(tuple(");
        for (i, f) in fields.iter().enumerate() {
            w.append(if i > 0 { ", " } else { "" });
            let ft = field_type(f)?;
            if matches!(f.value, SqlExpr::NullLit { .. }) && matches!(ft, SqlType::Array { .. }) {
                // an absent list is the empty one (a list is never NULL here)
                w.append(&format!("CAST([] AS {})", types[i]));
            } else if matches!(ft, SqlType::Struct { .. }) {
                // an instance as its Variant: NULL where it is absent
                w.append("CAST(").expr(self, &f.value, 0)?.append(&format!(" AS {})", types[i]));
            } else {
                w.expr(self, &f.value, 0)?;
            }
        }
        w.append(") AS Tuple(");
        for (i, f) in fields.iter().enumerate() {
            let name = self.ident(&f.name);
            w.append(if i > 0 { ", " } else { "" }).append(&name).append(" ").append(&types[i]);
        }
        w.append("))");
        Ok(())
    }

    /// A field by name; a field NUMBER by its index; a struct-typed field of an instance read through its Variant.
    fn struct_get(&self, w: &mut SqlWriter, source: &SqlExpr, field: &str, r#type: &TypeFact) -> Result<(), Refusal> {
        let _ = r#type;
        let field = if all_digits(field) { field.to_owned() } else { self.string_lit(field)? };
        w.append("tupleElement(");
        if variant_field(source)
            && let Some(st @ SqlType::Struct { .. }) = type_of(source)
        {
            w.append("variantElement(").expr(self, source, 0)?.append(", ");
            let lit = self.string_lit(&cast_type_name(self, st)?)?;
            w.append(&lit).append(")");
        } else {
            w.expr(self, source, 0)?;
        }
        w.append(", ").append(&field).append(")");
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
        let l = lambda_arg(arg(args, 1)?)?;
        w.append("arrayExists(").expr(self, &args[1], 0)?.append(", ");
        w.piece(&*self.lambda_arrays(arg(args, 0)?, l))?.append(")");
        Ok(())
    }

    fn list_for_all(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let l = lambda_arg(arg(args, 1)?)?;
        w.append("arrayAll(").expr(self, &args[1], 0)?.append(", ");
        w.piece(&*self.lambda_arrays(arg(args, 0)?, l))?.append(")");
        Ok(())
    }

    fn all_distinct(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let list = self.list(arg(args, 0)?);
        w.append("(length(arrayDistinct(").piece(&*list)?.append(")) = length(").piece(&*list)?.append("))");
        Ok(())
    }

    fn membership(&self, w: &mut SqlWriter, needle: &SqlExpr, collection: &SqlExpr) -> Result<(), Refusal> {
        w.append("has(").piece(&*self.list(collection))?.append(", ").expr(self, needle, 0)?.append(")");
        Ok(())
    }

    fn reduce_collection(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        collection: &SqlExpr,
        extras: &[SqlExpr],
    ) -> Result<(), Refusal> {
        self.reduce_collection_arm(w, reducer, collection, extras)
    }

    /// One list exploded in a select list: `arrayJoin`.
    fn unnest_projection(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        if args.len() != 1 {
            return Err(Refusal::capability(format!("an unnest of {} lists reached ClickHouse", args.len())));
        }
        w.append("arrayJoin(").piece(&*self.list(&args[0]))?.append(")");
        Ok(())
    }

    /// Pure's fold, `(element, acc)`, as `arrayFold((acc, x) -> ..., xs, init)`.
    fn fold_call(&self, w: &mut SqlWriter, fold: &SqlExpr) -> Result<(), Refusal> {
        let SqlExpr::FoldCall { source, lambda, init, r#type, .. } = fold else { unreachable!("a fold") };
        let TypeFact::Typed { r#type: t, .. } = r#type else {
            return Err(Refusal::capability("a fold of unknown type reached ClickHouse"));
        };
        let ty = cast_type_name(self, t)?;
        let elem = self.param(param_name(&lambda.params, 0)?)?;
        let acc = self.param(param_name(&lambda.params, 1)?)?;
        w.append(&format!("arrayFold(({acc}, {elem}) -> CAST(")).expr(self, &lambda.body, 0)?;
        w.append(&format!(" AS {ty}), ")).piece(&*self.list(source))?.append(", CAST(").expr(self, init, 0)?;
        w.append(&format!(" AS {ty}))"));
        Ok(())
    }

    /// Pure's splitPart: the non-empty tokens, 1-based, NULL past the end.
    fn split_part_call(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let tokens = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("arrayFilter(\"__t\" -> \"__t\" != '', splitByString(").expr(self, arg(a, 1)?, 0)?;
            w.append(", ifNull(").expr(self, arg(a, 0)?, 0)?.append(", '')))");
            Ok(())
        };
        let n = arg(a, 2)?;
        w.append("(CASE WHEN ").expr(self, n, 0)?.append(" < 1 OR ").expr(self, n, 0)?.append(" > length(");
        w.piece(&tokens)?.append(") THEN NULL ELSE arrayElement(").piece(&tokens)?.append(", ").expr(self, n, 0)?;
        w.append(") END)");
        Ok(())
    }

    fn struct_insert(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.struct_insert_arm(w, args)
    }

    // ---- variant: a JSON value's text (String)

    fn variant_construct(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.variant_construct_arm(w, a)
    }

    fn variant_get(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.variant_get_arm(w, args)
    }

    fn variant_elements(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let a0 = arg(args, 0)?;
        if list_element(a0).is_some() {
            w.expr(self, a0, 0)?;
            return Ok(());
        }
        w.append("JSONExtractArrayRaw(coalesce(").expr(self, a0, 0)?.append(", 'null'))");
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

    // ---- the hooks of the base, the aggregates and windows: `aggregates.rs`

    /// Pure's hashCode is a signed 64-bit integer: cityHash64's bits, read signed.
    fn hash_signed(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("coalesce(reinterpretAsInt64(cityHash64(").list(self, a)?.append(")), toInt64(0))");
        Ok(())
    }

    /// Pure round is half-EVEN: ClickHouse's round of a Float is; of a Decimal it is roundBankers.
    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let dbl = is_scalar(arg(a, 0)?, Scalar::Double);
        w.append(if dbl { "round(" } else { "roundBankers(" }).expr(self, &a[0], 0)?;
        if a.len() > 1 {
            w.append(", ").expr(self, &a[1], 0)?;
        }
        w.append(")");
        Ok(())
    }

    fn bit_op(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.bit_op_arm(w, f, a)
    }

    fn reducer(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        self.reducer_in(w, r, None)
    }

    /// A window function's name: lowercase.
    fn aggregate_name(&self, f: AggFn) -> String {
        f.as_str().to_ascii_lowercase()
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
        self.json_object_arm(w, kv)
    }

    fn json_array(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        self.json_array_arm(w, elements)
    }

    fn json_array_agg(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        order_keys: &[JsonArrayAggKey],
    ) -> Result<(), Refusal> {
        self.json_array_agg_arm(w, value, order_keys)
    }

    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal> {
        aggregates::format_text(parts)
    }

    /// This database's name for each scalar type, a refusal where it has none.
    fn scalar_type_name(&self, s: Scalar) -> Result<String, Refusal> {
        Ok(match s {
            Scalar::Boolean => "Nullable(Bool)",
            Scalar::Integer => "Nullable(Int32)",
            Scalar::Bigint => "Nullable(Int64)",
            Scalar::Hugeint => "Nullable(Int128)",
            Scalar::Double => "Nullable(Float64)",
            Scalar::Varchar | Scalar::Json | Scalar::TemporalText => "Nullable(String)",
            Scalar::Date => "Nullable(Date32)",
            Scalar::Timestamp | Scalar::Timestamptz => "Nullable(DateTime64(6, 'UTC'))",
            // ClickHouse's Time type is experimental (25.6, behind a setting)
            Scalar::Time => return Err(Refusal::capability("a time of day: ClickHouse's Time type is experimental")),
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

    /// Its JSON_TYPE answers DuckDB's names (a multiIf over JSONType).
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

    /// A DATE is `Date32`; no other type's name is compared here.
    fn typeof_name(&self, scalar: Scalar) -> Result<String, Refusal> {
        if scalar == Scalar::Date {
            return Ok("Date32".to_owned());
        }
        Err(Refusal::capability(format!("a TYPEOF compared with {scalar}'s name reached ClickHouse")))
    }
}

/// `params.get(i)`, refused past the end as Java's list read words it.
fn param_name(params: &[String], i: usize) -> Result<&str, Refusal> {
    params.get(i).map(String::as_str).ok_or_else(|| Refusal {
        class: Refusal::INDEX_OUT_OF_BOUNDS,
        message: format!("Index {i} out of bounds for length {}", params.len()),
    })
}

/// `correlated((SqlSelect) q)` of a subquery known to be a select.
fn correlated_of(q: &SqlQuery) -> Result<bool, Refusal> {
    match q {
        SqlQuery::Select(s) => correlated(s),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests;

// ==================================================================
// SqlDialect
// ==================================================================

impl crate::dialect::SqlDialect for ClickHouse {
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
        <ClickHouse as AnsiSqlRenderer>::reads_stored(t)
    }

    fn table_name_spelled(&self, name: &str, quoting: NameQuoting) -> String {
        AnsiSqlRenderer::table_name(self, name, quoting)
    }

    /// A session's own table is a TEMPORARY table (the host opens the HTTP session that keeps it): the table and its
    /// INSERT, this interface's own renders.
    fn session_table(&self, table: &SqlDdl, rows: Option<&SqlDml>) -> Result<Vec<String>, Refusal> {
        let SqlDdl::CreateTable { table: name, temporary, .. } = table else {
            unreachable!("the seam hands a session table a createTable")
        };
        if !temporary {
            return Err(Refusal::illegal_argument(format!("a session table is temporary: {name}")));
        }
        let mut out = vec![AnsiSqlRenderer::render_ddl(self, table)?];
        if let Some(rows) = rows {
            out.push(AnsiSqlRenderer::render_dml(self, rows)?);
        }
        Ok(out)
    }

    /// No transactions across statements.
    fn keys_snapshot(&self) -> Result<KeysSnapshot, Refusal> {
        Ok(KeysSnapshot::None)
    }

    /// No column limit documented.
    fn result_column_limit(&self) -> Result<Option<i32>, Refusal> {
        Ok(None)
    }

    /// No native PIVOT: a static pivot is a grouped CASE aggregation, its values discovered first.
    fn needs_static_pivot(&self) -> bool {
        true
    }

    /// Every scalar is Nullable(T); its own names for the same types, a date-time's precision and zone in its name,
    /// a struct a (named) Tuple.
    fn wire_type_name(&self, driver_type_name: Option<&str>) -> String {
        aggregates::wire_type_name(driver_type_name)
    }

    /// A Variant is its JSON text, a String.
    fn delivers_own(&self, label: &SqlType, wire: &str) -> bool {
        matches!(label, SqlType::Scalar(Scalar::Json)) && wire == "VARCHAR"
    }
}
