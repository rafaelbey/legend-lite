// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The base renderer's bodies (Java's `AnsiSqlRenderer`, `core/src/main/java/com/legend/sql/dialect/
//! AnsiSqlRenderer.java`): every hook's default, which a dialect's override reaches as Java's `super.x(...)`, and the
//! base's `final` helpers a dialect's `call` arms name. Each function is Java's method of the same name, its text and
//! its refusals Java's, in Java's order of evaluation.

use crate::float_text;
use crate::java::{big_decimal_plain, column_type_text, expr_text, output_cols_text, plain, simple_name};
use crate::passes::{FloatCanonExpansion, FloatPrintExpansion, StoredReads};
use crate::raw_sql;
use crate::refusal::{Refusal, arg, tail};
use crate::renderer::{AnsiSqlRenderer, PlanParam};
use crate::rewriter::SqlRewriter;
use crate::statement::{Bind, Binding, Digits, RenderedStatement, TypeSpelling, enum_map, list_text};
use crate::writer::SqlWriter;
use lite_sql::{
    BigDecimal, Bound, Column, ColumnKind, ColumnType, DateFmt, DateFmtPart, DatePart, Frame, JavaNumber, JoinKind,
    JsonArrayAggKey, Lambda, NameQuoting, NullOrder, Origin, OutputCol, ParamKind, Projection, Reducer, Scalar,
    SortKey, SqlAgg, SqlDdl, SqlDml, SqlExpr, SqlFn, SqlQuery, SqlSelect, SqlSource, SqlType, StructField,
    StructLitField, TypeFact, ValueKind, When,
};

// ==================================================================
// The entry points
// ==================================================================

/// Java's `render(SqlQuery)`: the query after this dialect's passes, written, as text.
pub fn render<D: AnsiSqlRenderer>(d: &D, query: SqlQuery) -> Result<String, Refusal> {
    write(d, query)?.into_text()
}

/// Java's `renderStatement(SqlQuery)`.
pub fn render_statement<D: AnsiSqlRenderer>(d: &D, query: SqlQuery) -> Result<RenderedStatement, Refusal> {
    write(d, query)?.into_statement()
}

/// Java's `renderInline`: this dialect's own `render`, in the single-line mode `inline` writes a subquery in.
pub fn render_inline<D: AnsiSqlRenderer>(d: &D, query: SqlQuery) -> Result<String, Refusal> {
    let previous = d.ansi_state().inline.replace(true);
    let rendered = d.render(query);
    d.ansi_state().inline.set(previous);
    rendered
}

/// The query after this dialect's passes, written.
fn write<D: AnsiSqlRenderer>(d: &D, query: SqlQuery) -> Result<SqlWriter, Refusal> {
    let q = passed(d, query)?;
    let mut w = SqlWriter::new();
    d.query(&mut w, &q, 0)?;
    Ok(w)
}

/// The query after this dialect's passes (Java's `passed`).
pub fn passed<D: AnsiSqlRenderer>(d: &D, query: SqlQuery) -> Result<SqlQuery, Refusal> {
    let mut q = query;
    for mut pass in render_passes(d) {
        q = pass.rewrite_root(q)?;
    }
    Ok(q)
}

/// What `render` runs (Java's `renderPasses`): a Float's print forms and its literal spelling expanded first unless
/// the dialect spells them itself, then the dialect's `passes`, then the stored reads last.
pub fn render_passes<D: AnsiSqlRenderer>(d: &D) -> Vec<Box<dyn SqlRewriter>> {
    let mut ps: Vec<Box<dyn SqlRewriter>> = Vec::new();
    if !d.spells_float_print() {
        ps.push(Box::new(FloatPrintExpansion::new(d.binds_float_print())));
    }
    if !d.spells_float_canon() {
        ps.push(Box::new(FloatCanonExpansion::new(d.binds_float_text())));
    }
    ps.extend(d.passes());
    ps.push(Box::new(StoredReads::new(D::reads_stored)));
    ps
}

/// Java's `raw`: the engine's H2 text is the one translated language.
pub fn raw<D: AnsiSqlRenderer>(d: &D, statement: &str, written_for: Option<&str>) -> Result<String, Refusal> {
    if written_for == Some(raw_sql::ENGINE_TEXT) { d.from_engine_text(statement) } else { Ok(statement.to_owned()) }
}

/// A HUGEINT-fact integer literal: bare.
pub fn huge_int_literal(v: &BigDecimal) -> String {
    big_decimal_plain(v.as_str())
}

/// The read of a column whose stored type `reads_stored` names: base, as text.
pub fn stored_read<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    column: &Column,
    stored: &ColumnType,
) -> Result<(), Refusal> {
    if reads_as_text(stored) {
        w.append("CAST(").expr(d, &SqlExpr::Column(column.clone()), 0)?.append(" AS ");
        let t = cast_type_name(d, &SqlType::Scalar(Scalar::Varchar))?;
        w.append(&t).append(")");
        return Ok(());
    }
    Err(Refusal::capability(format!(
        "this dialect reads a column stored as {} as the database holds it: a stored read of it is a pass defect",
        column_type_text(stored)
    )))
}

/// A type Pure cannot name: a Pure String, read as text on every dialect.
pub fn reads_as_text(t: &ColumnType) -> bool {
    matches!(t, ColumnType::Plain { kind: ColumnKind::Other | ColumnKind::Distinct })
}

// ==================================================================
// Queries and clause assembly
// ==================================================================

pub fn query<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, q: &SqlQuery, depth: usize) -> Result<(), Refusal> {
    match q {
        SqlQuery::With(with) => {
            w.append(if with.recursive { "WITH RECURSIVE " } else { "WITH " });
            for (i, c) in with.ctes.iter().enumerate() {
                if i > 0 {
                    w.append(", ");
                }
                let head = d.cte_name(&c.name) + &d.cte_as(c);
                w.append(&head);
                d.query(w, &c.query, depth + 1)?;
                w.push(')');
            }
            d.nl(w, depth);
            d.query(w, &with.body, depth)
        }
        SqlQuery::Select(s) => d.select(w, s, depth),
        SqlQuery::Union(u) => {
            let op = if u.all { "UNION ALL" } else { "UNION" };
            for (i, b) in u.branches.iter().enumerate() {
                if i > 0 {
                    d.nl(w, depth);
                    w.append(op);
                    d.nl(w, depth);
                }
                d.query(w, b, depth)?;
            }
            Ok(())
        }
    }
}

pub fn select<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
    if s.qualify.is_some() && !d.supports_qualify() {
        return Err(Refusal::illegal_state(
            "QUALIFY reached a writer without QUALIFY support — the QualifyToSubselect pass must run",
        ));
    }
    w.append("SELECT ");
    if s.distinct {
        w.append("DISTINCT ");
    }
    if s.projections.is_empty() {
        w.append("*");
    } else {
        d.projections(w, &s.projections)?;
    }
    if !matches!(s.from, SqlSource::Dual) {
        d.nl(w, depth);
        w.append("FROM ");
        d.source(w, &s.from, depth)?;
    }
    if let Some(wh) = &s.r#where {
        d.nl(w, depth);
        w.append("WHERE ").expr(d, wh, 0)?;
    }
    if !s.group_by.is_empty() {
        d.nl(w, depth);
        w.append("GROUP BY ").list(d, &s.group_by)?;
    }
    if let Some(h) = &s.having {
        d.nl(w, depth);
        w.append("HAVING ").expr(d, h, 0)?;
    }
    if s.qualify.is_some() {
        d.append_qualify(w, s, depth)?;
    }
    if !s.order_by.is_empty() {
        d.nl(w, depth);
        w.append("ORDER BY ").join(&s.order_by, ", ", |w, k| d.sort_key(w, k))?;
    }
    if let Some(limit) = s.limit {
        d.nl(w, depth);
        w.append("LIMIT ").append_long(limit);
    }
    if let Some(offset) = s.offset {
        d.nl(w, depth);
        w.append("OFFSET ").append_long(offset);
    }
    Ok(())
}

/// No ANSI spelling of an external source exists.
pub fn source_url(url: &str) -> Result<String, Refusal> {
    Err(Refusal::capability(format!("sourceUrl reached a dialect without an external-source encoding: {url}")))
}

pub fn append_qualify<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    s: &SqlSelect,
    depth: usize,
) -> Result<(), Refusal> {
    let _ = (d, w, s, depth);
    Err(Refusal::capability("QUALIFY reached a dialect without native support"))
}

/// A column reference, qualified by its source's alias when it has one.
pub fn column_ref<D: AnsiSqlRenderer>(d: &D, c: &Column) -> Result<String, Refusal> {
    Ok(match &c.table {
        None => d.column_name(c)?,
        Some(t) => format!("{}.{}", d.alias_ident(t), d.column_name(c)?),
    })
}

pub fn projection<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
    d.projection_value(w, p)?;
    if let Some(out) = &p.out
        && out.origin == Origin::PerKey
    {
        w.append(" AS ");
        return d.per_key_name(w, &out.name);
    }
    if let Some(alias) = &p.alias {
        let a = d.alias_ident(alias);
        w.append(" AS ").append(&a);
        return Ok(());
    }
    if let Some(label) = d.implicit_label(p)? {
        w.append(" AS ").append(&label);
    }
    Ok(())
}

/// A projection's value, before its label: a NULL under a typed slot spells its type.
pub fn projection_value<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
    if let (SqlExpr::NullLit { .. }, Some(out)) = (&p.expr, &p.out)
        && typed_null_slot(&out.r#type)
    {
        let t = cast_type_name(d, &out.r#type)?;
        w.append("CAST(NULL AS ").append(&t).append(")");
        return Ok(());
    }
    d.expr(w, &p.expr, 0)
}

/// A select's projections, a run of per-key projections one per-key region written last.
pub fn projections<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, ps: &[Projection]) -> Result<(), Refusal> {
    projections_with(w, ps, &|w, i| d.projection(w, &ps[i]))
}

/// `projections`, each projection written by `each` from its position (Java's two-argument overload).
pub fn projections_with(
    w: &mut SqlWriter,
    ps: &[Projection],
    each: &dyn Fn(&mut SqlWriter, usize) -> Result<(), Refusal>,
) -> Result<(), Refusal> {
    let at: Vec<usize> = (0..ps.len()).collect();
    let Some(run) = ps.iter().position(per_key) else {
        w.join(&at, ", ", |w, i| each(w, *i))?;
        return Ok(());
    };
    for p in &ps[run..] {
        if !per_key(p) {
            return Err(Refusal::illegal_state(format!(
                "a per-key projection run is followed by '{}': the run is written last, after the select's other \
                 projections",
                p.alias.as_deref().unwrap_or("null")
            )));
        }
    }
    let (fixed, keyed) = at.split_at(run);
    w.join(fixed, ", ", |w, i| each(w, *i))?;
    w.region(if fixed.is_empty() { "" } else { ", " }, ", ", if fixed.is_empty() { None } else { Some("") }, |w| {
        w.join(keyed, ", ", |w, i| each(w, *i)).map(|_| ())
    })?;
    Ok(())
}

fn per_key(p: &Projection) -> bool {
    p.out.as_ref().is_some_and(|o| o.origin == Origin::PerKey)
}

/// A per-key name: delimited, the key's position written into it by the runner after its `_pk`.
pub fn per_key_name<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, stem: &str) -> Result<(), Refusal> {
    let q = d.lexicon().quote_char().to_string();
    w.append(&format!("{q}_pk")).counter()?;
    let rest = format!("_{stem}").replace(&q, &format!("{q}{q}")) + &q;
    w.append(&rest);
    Ok(())
}

/// The label an alias-less projection spells: a stored read keeps its column's own label.
pub fn implicit_label<D: AnsiSqlRenderer>(d: &D, p: &Projection) -> Result<Option<String>, Refusal> {
    Ok(match &p.expr {
        SqlExpr::StoredRead { column, .. } => Some(d.column_name(column)?),
        _ => None,
    })
}

/// The scalar slot types a typed NULL spells.
fn typed_null_slot(t: &SqlType) -> bool {
    matches!(
        t,
        SqlType::Scalar(
            Scalar::Boolean
                | Scalar::Integer
                | Scalar::Bigint
                | Scalar::Hugeint
                | Scalar::Double
                | Scalar::Varchar
                | Scalar::Date
                | Scalar::Timestamp
                | Scalar::Timestamptz
        )
    )
}

pub fn sort_key<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, k: &SortKey) -> Result<(), Refusal> {
    w.expr(d, &k.expr, 0)?.append(if k.ascending { "" } else { " DESC" });
    w.append(match k.null_order {
        Some(NullOrder::NullsFirst) => " NULLS FIRST",
        Some(NullOrder::NullsLast) => " NULLS LAST",
        // a bare key: the engine's one null placement, NULL IS LARGEST
        None if k.ascending => " NULLS LAST",
        None => " NULLS FIRST",
    });
    Ok(())
}

// ==================================================================
// Sources
// ==================================================================

pub fn source<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, src: &SqlSource, depth: usize) -> Result<(), Refusal> {
    match src {
        SqlSource::Dual => Err(Refusal::illegal_state("Dual renders as FROM-clause omission — caller bug")),
        SqlSource::Table { name, alias, call, quoting, .. } => {
            let t = d.table_name(name, *quoting);
            w.append(&t).append(if *call { "()" } else { "" });
            let a = d.alias_ident(alias);
            w.append(" AS ").append(&a);
            Ok(())
        }
        SqlSource::Cte { name, alias, .. } => {
            let text = format!("{} AS {}", d.cte_name(name), d.alias_ident(alias));
            w.append(&text);
            Ok(())
        }
        SqlSource::Subselect { inner, alias, .. } => d.subselect_source(w, inner, alias, depth),
        SqlSource::VarSetPlaceholder { var_name, alias, .. } => {
            let a = d.alias_ident(alias);
            w.append("(${").append(var_name).append("}) as ").append(&a);
            Ok(())
        }
        SqlSource::Values { rows, columns, alias, outputs } => d.values_source(w, rows, columns, alias, outputs),
        SqlSource::RawSql { sql, alias, written_for, .. } => {
            let text = raw(d, sql, Some(written_for))?;
            let a = d.alias_ident(alias);
            w.append("(").append(&text).append(") AS ").append(&a);
            Ok(())
        }
        SqlSource::Catalog { alias, .. } => {
            w.append("(");
            d.catalog(w, src)?;
            let a = d.alias_ident(alias);
            w.append(") AS ").append(&a);
            Ok(())
        }
        SqlSource::SourceUrl { url, alias, .. } => {
            w.append("(");
            d.nl(w, depth + 1);
            let text = d.source_url(url)?;
            w.append(&text);
            d.nl(w, depth);
            let a = d.alias_ident(alias);
            w.append(") AS ").append(&a);
            Ok(())
        }
        SqlSource::Pivot { .. } => d.pivot_source(w, src, depth),
        SqlSource::Join { left, right, kind, on, .. } => {
            d.source(w, left, depth)?;
            d.nl(w, depth);
            let keyword = d.join_keyword(*kind)?;
            w.append(&keyword).append(" ");
            d.source(w, right, depth)?;
            if let Some(on) = on {
                let condition = d.join_condition(*kind, on);
                w.append(" ON ").expr(d, &condition, 0)?;
            }
            Ok(())
        }
    }
}

pub fn subselect_source<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    inner: &SqlQuery,
    alias: &str,
    depth: usize,
) -> Result<(), Refusal> {
    w.append("(");
    d.nl(w, depth + 1);
    d.query(w, inner, depth + 1)?;
    d.nl(w, depth);
    let a = d.alias_ident(alias);
    w.append(") AS ").append(&a);
    Ok(())
}

/// ANSI row-constructor VALUES with column aliases.
pub fn values_source<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    rows: &[Vec<SqlExpr>],
    columns: &[String],
    alias: &str,
) -> Result<(), Refusal> {
    w.append("(VALUES ");
    for (r, row) in rows.iter().enumerate() {
        if r > 0 {
            w.append(", ");
        }
        w.append("(").list(d, row)?.append(")");
    }
    let names: Vec<String> = columns.iter().map(|c| d.alias_ident(c)).collect();
    let a = d.alias_ident(alias);
    w.append(") AS ").append(&a).append("(").append(&names.join(", ")).append(")");
    Ok(())
}

/// A join's keywords: the ANSI forms, and an AS-OF join's own.
pub fn join_keyword<D: AnsiSqlRenderer>(d: &D, kind: JoinKind) -> Result<String, Refusal> {
    Ok(match kind {
        JoinKind::Inner => "JOIN".to_owned(),
        JoinKind::Left => "LEFT OUTER JOIN".to_owned(),
        JoinKind::Right => "RIGHT OUTER JOIN".to_owned(),
        JoinKind::Full => "FULL OUTER JOIN".to_owned(),
        JoinKind::Cross => "CROSS JOIN".to_owned(),
        JoinKind::CrossLateral => "CROSS JOIN LATERAL".to_owned(),
        JoinKind::AsofLeft => d.as_of_join_clause()?,
        JoinKind::LeftLateral => "LEFT JOIN LATERAL".to_owned(),
    })
}

// ==================================================================
// Expressions
// ==================================================================

/// The checked-narrowing spelling: execution dialects emit Pure's toOne size guard.
pub fn checked_one<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    list: &SqlExpr,
    scalar_carrier: bool,
    at_least_only: bool,
    parent_prec: i32,
) -> Result<(), Refusal> {
    use crate::build::{call, case, cast, int_lit, string_lit, when};
    let bound = if at_least_only { "[1..*]" } else { "[1]" };
    if scalar_carrier {
        // a scalar ([0..1]) carrier: NULL is the empty collection
        let guard = case(
            vec![when(
                call(SqlFn::IsNull, vec![list.clone()]),
                call(
                    SqlFn::Error,
                    vec![string_lit(&format!("Cannot cast a collection of size 0 to multiplicity {bound}"))],
                ),
            )],
            Some(list.clone()),
        );
        return d.expr(w, &guard, parent_prec);
    }
    let len = call(SqlFn::ListLength, vec![list.clone()]);
    let size_err = call(
        SqlFn::Error,
        vec![call(
            SqlFn::Concat,
            vec![
                string_lit("Cannot cast a collection of size "),
                cast(call(SqlFn::Coalesce, vec![len.clone(), int_lit(0)]), SqlType::Scalar(Scalar::Varchar)),
                string_lit(&format!(" to multiplicity {bound}")),
            ],
        )],
    );
    let guard = if at_least_only {
        // toOneMany: at least one -- the list rides through intact
        case(
            vec![when(
                call(
                    SqlFn::Or,
                    vec![call(SqlFn::IsNull, vec![list.clone()]), call(SqlFn::Equal, vec![len, int_lit(0)])],
                ),
                size_err,
            )],
            Some(list.clone()),
        )
    } else {
        // exactly one: size != 1 raises, 1 extracts
        case(
            vec![when(
                call(
                    SqlFn::Or,
                    vec![call(SqlFn::IsNull, vec![list.clone()]), call(SqlFn::NotEqual, vec![len, int_lit(1)])],
                ),
                size_err,
            )],
            Some(call(SqlFn::ListGet, vec![list.clone(), int_lit(1)])),
        )
    };
    d.expr(w, &guard, parent_prec)
}

/// Pure-collection carrier compaction: SQL NULL elements stripped with the list-filter spelling.
pub fn compact_list<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    list: &SqlExpr,
    parent_prec: i32,
) -> Result<(), Refusal> {
    use crate::build::{call, derived, lambda};
    let filtered =
        call(SqlFn::ListFilter, vec![list.clone(), lambda(&["x"], call(SqlFn::IsNotNull, vec![derived(None, "x")]))]);
    d.expr(w, &filtered, parent_prec)
}

/// An expression, written: a leaf spelled as text, a sub-expression written into the same writer.
pub fn expr<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, e: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
    match e {
        SqlExpr::Group { inner, .. } => {
            w.append("(").expr(d, inner, 0)?.append(")");
        }
        SqlExpr::TempTableInSplice { temp_table_name, .. } => {
            return Err(Refusal::illegal_state(format!(
                "temp-table IN splice '{temp_table_name}' reached an executable dialect — plan-text vocabulary only"
            )));
        }
        // a plan parameter is BOUND where it is written
        SqlExpr::PlanParam { .. } => {
            let p = PlanParam::of(e).expect("a plan parameter");
            return d.placeholder(w, &one_value(&p)?);
        }
        SqlExpr::KeyCell { cell, .. } => {
            w.bind_key(*cell)?;
        }
        SqlExpr::RowOrder { table, .. } => {
            let text = match table {
                None => d.row_order_column()?,
                Some(t) => format!("{}.{}", d.alias_ident(t), d.row_order_column()?),
            };
            w.append(&text);
        }
        SqlExpr::Column(c) if c.origin == Some(Origin::PerKey) => {
            if let Some(t) = &c.table {
                let a = d.alias_ident(t);
                w.append(&a).append(".");
            }
            return d.per_key_name(w, &c.name);
        }
        SqlExpr::Column(c) => {
            let text = d.column_ref(c)?;
            w.append(&text);
        }
        SqlExpr::StoredRead { column, stored, .. } => return d.stored_read(w, column, stored),
        SqlExpr::Star { table, .. } => {
            let text = match table {
                None => "*".to_owned(),
                Some(t) => format!("{}.*", d.alias_ident(t)),
            };
            w.append(&text);
        }
        SqlExpr::StarExcept { table, except, .. } => {
            let head = match table {
                None => "*".to_owned(),
                Some(t) => format!("{}.*", d.alias_ident(t)),
            };
            let names: Vec<String> = except.iter().map(|n| d.star_except_name(n)).collect();
            let text = format!("{head} {} ({})", d.star_except_keyword()?, names.join(", "));
            w.append(&text);
        }
        SqlExpr::StringLit { value, .. } => {
            let text = d.string_lit(value)?;
            w.append(&text);
        }
        SqlExpr::FormatLit { parts, .. } => {
            let text = d.format_text(parts)?;
            let lit = d.string_lit(&text)?;
            w.append(&lit);
        }
        SqlExpr::DatePartLit { part, .. } => {
            return Err(Refusal::illegal_state(format!(
                "a date part ({part}) reached the expression writer: the arm of its call spells it"
            )));
        }
        SqlExpr::JsonKindLit { kind, .. } => {
            let name = d.json_kind_name(*kind)?;
            let lit = d.string_lit(&name)?;
            w.append(&lit);
        }
        SqlExpr::TypeNameLit { scalar, .. } => {
            let name = d.typeof_name(*scalar)?;
            let lit = d.string_lit(&name)?;
            w.append(&lit);
        }
        SqlExpr::IntLit { value, .. } => {
            w.append_long(*value);
        }
        // the numeric charter's Rule 1: a Float literal renders BARE in the plain Float spelling
        SqlExpr::FloatLit { value, .. } => {
            let text = d.float_literal(value.value())?;
            w.append(&text);
        }
        // a scale-0 DECIMAL-fact literal casts so the wire reads DECIMAL; a HUGEINT-fact big integer and a
        // fractional decimal render bare
        SqlExpr::DecimalLit { value, r#type } => {
            let text = match r#type {
                TypeFact::Typed { r#type: SqlType::Decimal { precision, scale: 0 }, .. } => {
                    format!("CAST({} AS DECIMAL({precision},0))", big_decimal_plain(value.as_str()))
                }
                TypeFact::Typed { r#type: SqlType::Scalar(Scalar::Hugeint), .. } => d.huge_int_literal(value),
                _ => big_decimal_plain(value.as_str()),
            };
            w.append(&text);
        }
        SqlExpr::BoolLit { value, .. } => {
            let text = d.bool_lit(*value);
            w.append(&text);
        }
        SqlExpr::NullLit { .. } => {
            w.append("NULL");
        }
        SqlExpr::DateLit { iso, .. } => {
            let text = d.date_lit(iso);
            w.append(&text);
        }
        SqlExpr::TimestampLit { iso, .. } => {
            let text = d.timestamp_lit(iso);
            w.append(&text);
        }
        SqlExpr::TimeLit { iso, .. } => {
            let text = d.time_lit(iso)?;
            w.append(&text);
        }
        SqlExpr::OrderedListAgg { value, order_by, .. } => {
            w.append("list(").expr(d, value, 0)?.append(" ORDER BY ").expr(d, order_by, 0)?.append(")");
        }
        SqlExpr::ArrayLit { elements, .. } => return d.array_lit(w, elements),
        SqlExpr::StructLit { fields, .. } => return d.struct_lit(w, fields),
        SqlExpr::StructGet { source, field, r#type } => return d.struct_get(w, source, field, r#type),
        SqlExpr::Call { r#fn, args, r#type } => return d.call(w, *r#fn, args, r#type, parent_prec),
        SqlExpr::Case { whens, otherwise, r#type } => return d.case_expr(w, whens, otherwise.as_deref(), r#type),
        SqlExpr::Exists { subquery, .. } => {
            w.append("EXISTS (");
            inline(d, w, subquery)?;
            w.append(")");
        }
        SqlExpr::InSubquery { value, subquery, .. } => {
            w.expr(d, value, 4)?.append(" IN (");
            inline(d, w, subquery)?;
            w.append(")");
        }
        SqlExpr::CheckedDefects { .. } => {
            return Err(Refusal::capability("nested checked defects reached a dialect without list lambdas"));
        }
        SqlExpr::CheckedChildValue { .. } => {
            return Err(Refusal::capability("a checked child's value reached a dialect without list lambdas"));
        }
        SqlExpr::Quantified { value, comparison, quantifier, subquery, .. } => {
            w.expr(d, value, 4)?;
            let Some((op, _)) = infix_of(*comparison) else {
                return Err(Refusal::null_pointer(format!(
                    "quantified comparison must be an infix operator: {comparison}"
                )));
            };
            w.append(" ").append(op).append(" ").append(quantifier.as_str()).append(" (");
            inline(d, w, subquery)?;
            w.append(")");
        }
        SqlExpr::ScalarSubquery { subquery, .. } => {
            w.append("(");
            inline(d, w, subquery)?;
            w.append(")");
        }
        SqlExpr::CheckedOne { list, scalar_carrier, at_least_only, .. } => {
            return d.checked_one(w, list, *scalar_carrier, *at_least_only, parent_prec);
        }
        SqlExpr::CompactList { list, .. } => return d.compact_list(w, list, parent_prec),
        SqlExpr::DeferredTdsString { id, .. } => {
            return Err(Refusal::illegal_state(format!(
                "deferred relation-toString reached the renderer — the execution boundary must resolve the dynamic \
                 column list first (DeferredTdsString id {id})"
            )));
        }
        SqlExpr::WindowCall { r#fn, partition_by, order_by, frame, .. } => {
            return d.window_call(w, r#fn, partition_by, order_by, frame.as_ref());
        }
        SqlExpr::Lambda(l) => return d.lambda(w, l),
        // a cast to the tagged carrier is the identity
        SqlExpr::Cast { value, target: SqlType::Tagged { .. }, .. } => return d.expr(w, value, parent_prec),
        SqlExpr::Cast { value, target, conform, .. } => return d.variant_aware_cast(w, value, target, *conform),
        SqlExpr::FoldCall { .. } => return d.fold_call(w, e),
        SqlExpr::JsonObject { kv, .. } => return d.json_object(w, kv),
        SqlExpr::JsonArray { elements, .. } => return d.json_array(w, elements),
        SqlExpr::JsonArrayAgg { value, order_keys, .. } => return d.json_array_agg(w, value, order_keys),
        SqlExpr::ReduceCollection { reducer, collection, extras, .. } => {
            return d.reduce_collection(w, *reducer, collection, extras);
        }
        SqlExpr::Membership { needle, collection, .. } => {
            return match PlanParam::of(collection) {
                Some(p) => any_of(d, w, needle, &p, parent_prec),
                None => d.membership(w, needle, collection),
            };
        }
        SqlExpr::Reducer(r) => return d.reducer(w, r),
    }
    Ok(())
}

/// Variant navigation: a backend without JSON support refuses it.
pub fn variant_get<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
    let _ = (d, w, args);
    Err(Refusal::capability("variant navigation reached a dialect without JSON support"))
}

/// A struct literal: a backend without struct support refuses it.
pub fn struct_lit<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
    let _ = (d, w, fields);
    Err(Refusal::capability("a struct literal reached a dialect without struct support"))
}

/// A struct's field read: a backend without struct support refuses it.
pub fn struct_get<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, source: &SqlExpr, field: &str) -> Result<(), Refusal> {
    let _ = (d, w, source, field);
    Err(Refusal::capability("a struct extraction reached a dialect without struct support"))
}

/// Collection membership: a backend data-model capability.
pub fn membership<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    needle: &SqlExpr,
    collection: &SqlExpr,
) -> Result<(), Refusal> {
    let _ = (d, w, needle);
    let detail = match collection {
        SqlExpr::Call { r#fn, .. } => format!(" {fn}"),
        _ => String::new(),
    };
    Err(Refusal::capability(format!(
        "collection membership reached a dialect without a list encoding [collection: {}{detail}]",
        simple_name(collection)
    )))
}

/// DuckDB's reference JSON-object constructor: alternating key/value arguments.
pub fn json_object<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, kv: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("json_object(").list(d, kv)?.append(")");
    Ok(())
}

pub fn json_array<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("json_array(").list(d, elements)?.append(")");
    Ok(())
}

/// An aggregate over zero rows is NULL; the graph contract says the empty array.
pub fn json_array_agg<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    value: &SqlExpr,
    order_keys: &[JsonArrayAggKey],
) -> Result<(), Refusal> {
    if order_keys.is_empty() {
        w.append("coalesce(json_group_array(").expr(d, value, 0)?.append("), '[]')");
    } else {
        w.append("coalesce(to_json(list(").expr(d, value, 0)?.append(" ORDER BY ");
        w.join(order_keys, ", ", |w, k| {
            w.expr(d, &k.expr, 0)?.append(if k.desc { " DESC" } else { " ASC" }).append(" NULLS LAST");
            Ok(())
        })?;
        w.append(")), '[]')");
    }
    Ok(())
}

// ---- the helpers a dialect's call arms name (Java's protected final methods)

/// The ANSI infix operators: semantic entry -> (sql, precedence); higher binds tighter.
pub fn infix_of(f: SqlFn) -> Option<(&'static str, i32)> {
    Some(match f {
        SqlFn::Or => ("OR", 1),
        SqlFn::And => ("AND", 2),
        SqlFn::Equal => ("=", 4),
        SqlFn::NotEqual => ("<>", 4),
        SqlFn::Less => ("<", 4),
        SqlFn::LessEqual => ("<=", 4),
        SqlFn::Greater => (">", 4),
        SqlFn::GreaterEqual => (">=", 4),
        SqlFn::Plus => ("+", 5),
        SqlFn::Minus => ("-", 5),
        SqlFn::Times => ("*", 6),
        _ => return None,
    })
}

/// An infix operator, ANSI-spelled, parenthesized by precedence.
pub fn infix<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    f: SqlFn,
    args: &[SqlExpr],
    parent_prec: i32,
) -> Result<(), Refusal> {
    let Some((sql, prec)) = infix_of(f) else {
        return Err(Refusal::null_pointer(format!("{f} is no infix operator")));
    };
    // a trailing operand of the same precedence is its own value (- and + and *); comparisons are non-associative
    let trailing_own_value = matches!(f, SqlFn::Minus | SqlFn::Plus | SqlFn::Times);
    let non_associative = prec == 4;
    let wrap = prec < parent_prec;
    let pad = d.infix_pad(f);
    if wrap {
        w.append("(");
    }
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            w.append(&pad).append(sql).append(&pad);
        }
        w.expr(d, a, if (i > 0 && trailing_own_value) || non_associative { prec + 1 } else { prec })?;
    }
    if wrap {
        w.append(")");
    }
    Ok(())
}

/// A raise through `fn`, its message enveloped in the U+001F provenance sentinel (and an optional source span).
pub fn raise_envelope<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr], f: &str) -> Result<(), Refusal> {
    w.append(f).append("(chr(31) || ");
    if a.len() > 1 {
        w.expr(d, &a[1], 0)?.append(" || chr(30) || ");
    }
    w.append("(").expr(d, arg(a, 0)?, 0)?.append(") || chr(31))");
    Ok(())
}

/// Null-ignoring flat concat: nested concats splice into one call.
pub fn concat_flat<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    concat_of(d, w, &flatten_concat(a))
}

/// `concat` of the arguments, at most `max_function_arguments` to one call.
fn concat_of<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, args: &[&SqlExpr]) -> Result<(), Refusal> {
    let max = d.max_function_arguments();
    if args.len() <= max {
        w.append("concat(");
        for (i, e) in args.iter().enumerate() {
            if i > 0 {
                w.append(", ");
            }
            w.expr(d, e, 0)?;
        }
        w.append(")");
        return Ok(());
    }
    let run = args.len().div_ceil(max);
    w.append("concat(");
    let mut from = 0;
    while from < args.len() {
        if from > 0 {
            w.append(", ");
        }
        concat_of(d, w, &args[from..args.len().min(from + run)])?;
        from += run;
    }
    w.append(")");
    Ok(())
}

/// Nested CONCAT calls splice into one flat argument list.
pub fn flatten_concat(a: &[SqlExpr]) -> Vec<&SqlExpr> {
    let mut out = Vec::new();
    for e in a {
        match e {
            SqlExpr::Call { r#fn: SqlFn::Concat, args, .. } => out.extend(flatten_concat(args)),
            other => out.push(other),
        }
    }
    out
}

/// `NOT x`, parenthesized under an operator that binds tighter.
pub fn not_prefix<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    a: &[SqlExpr],
    parent_prec: i32,
) -> Result<(), Refusal> {
    if 3 < parent_prec {
        w.append("(");
    }
    w.append("NOT ").expr(d, arg(a, 0)?, 3)?;
    if 3 < parent_prec {
        w.append(")");
    }
    Ok(())
}

pub fn negation<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("-").expr(d, arg(a, 0)?, 7)?;
    Ok(())
}

pub fn is_null_test<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.expr(d, arg(a, 0)?, 4)?.append(" IS NULL");
    Ok(())
}

pub fn is_not_null_test<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.expr(d, arg(a, 0)?, 4)?.append(" IS NOT NULL");
    Ok(())
}

/// `x IN (a, b, ...)`; a plan parameter as the whole list is one array, bound once.
pub fn in_list<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr], parent_prec: i32) -> Result<(), Refusal> {
    if a.len() == 2
        && let Some(p) = PlanParam::of(&a[1])
    {
        return any_of(d, w, &a[0], &p, parent_prec);
    }
    w.expr(d, arg(a, 0)?, 4)?.append(" IN (").list(d, tail(a, 1)?)?.append(")");
    Ok(())
}

pub fn distinct_from<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("(").expr(d, arg(a, 0)?, 4)?.append(" IS DISTINCT FROM ").expr(d, arg(a, 1)?, 4)?.append(")");
    Ok(())
}

pub fn not_distinct_from<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("(").expr(d, arg(a, 0)?, 4)?.append(" IS NOT DISTINCT FROM ").expr(d, arg(a, 1)?, 4)?.append(")");
    Ok(())
}

/// Pure's divide is a Float division: both operands cast to DOUBLE before it.
pub fn divide_as_doubles<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("(CAST(")
        .expr(d, arg(a, 0)?, 0)?
        .append(" AS DOUBLE) / CAST(")
        .expr(d, arg(a, 1)?, 0)?
        .append(" AS DOUBLE))");
    Ok(())
}

/// Pure's mod: the remainder with the divisor's sign.
pub fn mod_floored<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("MOD(MOD(").expr(d, arg(a, 0)?, 0)?.append(", ").expr(d, arg(a, 1)?, 0)?.append(") + ");
    w.expr(d, arg(a, 1)?, 0)?.append(", ").expr(d, arg(a, 1)?, 0)?.append(")");
    Ok(())
}

/// Pure's rem: the remainder with the dividend's sign.
pub fn mod_truncated<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("MOD(").expr(d, arg(a, 0)?, 0)?.append(", ").expr(d, arg(a, 1)?, 0)?.append(")");
    Ok(())
}

/// `CAST(fn(x) AS BIGINT)`: ceiling, floor and sign are Integers in Pure.
pub fn bigint_of<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, f: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("CAST(").append(f).append("(").expr(d, arg(a, 0)?, 0)?.append(") AS BIGINT)");
    Ok(())
}

/// `CAST(x AS type)`.
pub fn cast_as<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, x: &SqlExpr, t: &str) -> Result<(), Refusal> {
    w.append("CAST(").expr(d, x, 0)?.append(" AS ").append(t).append(")");
    Ok(())
}

/// Boolean xor as `(x AND NOT y) OR (NOT x AND y)`, wrapped by the walk.
pub fn xor_as_and_or<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    a: &[SqlExpr],
    parent_prec: i32,
) -> Result<(), Refusal> {
    op(w, parent_prec, &|w| {
        w.append("(").expr(d, arg(a, 0)?, 3)?.append(" AND NOT ").expr(d, arg(a, 1)?, 3)?.append(") OR (NOT ");
        w.expr(d, arg(a, 0)?, 3)?.append(" AND ").expr(d, arg(a, 1)?, 3)?.append(")");
        Ok(())
    })
}

/// `lpad`/`rpad`: the pad char is optional in Pure; SQL requires it -- ' '.
pub fn pad_with_space<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, f: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append(f).append("(");
    if a.len() == 2 {
        let space = crate::build::string_lit(" ");
        w.expr(d, &a[0], 0)?.append(", ").expr(d, &a[1], 0)?.append(", ").expr(d, &space, 0)?;
    } else {
        w.list(d, a)?;
    }
    w.append(")");
    Ok(())
}

/// `lpad`/`rpad` where the length is a 32-bit INTEGER: a length that is no literal cast to INTEGER.
pub fn pad_with_space_to_integer<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    f: &str,
    a: &[SqlExpr],
) -> Result<(), Refusal> {
    if matches!(arg(a, 1)?, SqlExpr::IntLit { .. }) {
        return pad_with_space(d, w, f, a);
    }
    w.append(f).append("(").expr(d, arg(a, 0)?, 0)?.append(", CAST(").expr(d, arg(a, 1)?, 0)?.append(" AS INTEGER), ");
    if a.len() == 2 {
        w.expr(d, &crate::build::string_lit(" "), 0)?;
    } else {
        w.expr(d, arg(a, 2)?, 0)?;
    }
    w.append(")");
    Ok(())
}

/// toUpperFirstCharacter / toLowerFirstCharacter: `fn(substr(x, 1, 1)) || substr(x, 2)`, walk-wrapped.
pub fn first_char_cased<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    f: &str,
    a: &[SqlExpr],
    parent_prec: i32,
) -> Result<(), Refusal> {
    op(w, parent_prec, &|w| {
        w.append(f).append("(substr(").expr(d, arg(a, 0)?, 0)?.append(", 1, 1)) || substr(");
        w.expr(d, arg(a, 0)?, 0)?.append(", 2)");
        Ok(())
    })
}

/// Day-grained truncation delivers a DATE: the coarser parts cast the truncation to DATE.
pub fn date_trunc_date_grained<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    a: &[SqlExpr],
    spelled_part: &str,
) -> Result<(), Refusal> {
    let trunc = |w: &mut SqlWriter| -> Result<(), Refusal> {
        w.append("date_trunc(").append(spelled_part).append(", ").expr(d, arg(a, 1)?, 0)?.append(")");
        Ok(())
    };
    match part(arg(a, 0)?)? {
        DatePart::Month | DatePart::Year | DatePart::Week | DatePart::Quarter => {
            w.append("CAST(").piece(&trunc)?.append(" AS DATE)");
        }
        DatePart::Day
        | DatePart::Hour
        | DatePart::Minute
        | DatePart::Second
        | DatePart::Millisecond
        | DatePart::Microsecond
        | DatePart::Nanosecond
        | DatePart::DayOfYear
        | DatePart::IsoDayOfWeek => {
            w.piece(&trunc)?;
        }
    }
    Ok(())
}

/// The part a date call's first argument names.
pub fn part(e: &SqlExpr) -> Result<DatePart, Refusal> {
    match e {
        SqlExpr::DatePartLit { part, .. } => Ok(*part),
        other => Err(Refusal::illegal_state(format!(
            "a date call's first argument is its part, not {}: the lowering's bug",
            expr_text(other)
        ))),
    }
}

/// A Float's literal spelling: the portable decomposition over the double's text.
pub fn float_canon_by_text<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    a: &[SqlExpr],
    parent_prec: i32,
) -> Result<(), Refusal> {
    d.expr(w, &float_text::canon_of(arg(a, 0)?.clone()), parent_prec)
}

/// A Float's print form: the portable decomposition over the double's text.
pub fn float_print_by_text<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    args: &[SqlExpr],
    parent_prec: i32,
) -> Result<(), Refusal> {
    d.expr(w, &float_text::print_of(arg(args, 0)?.clone()), parent_prec)
}

/// CAST rendering; the temporal-text and decimal-text marker casts are label devices that never render.
pub fn variant_aware_cast<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    value: &SqlExpr,
    target: &SqlType,
    conform: bool,
) -> Result<(), Refusal> {
    let _ = conform;
    if matches!(target, SqlType::Scalar(Scalar::TemporalText | Scalar::DecimalText)) {
        return d.expr(w, value, 0);
    }
    w.append("CAST(").expr(d, value, 0)?.append(" AS ");
    let t = cast_type_name(d, target)?;
    w.append(&t).append(")");
    Ok(())
}

// ---- window / aggregate / case

pub fn case_expr<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    whens: &[When],
    otherwise: Option<&SqlExpr>,
) -> Result<(), Refusal> {
    w.append("CASE");
    for wh in whens {
        w.append(" WHEN ").expr(d, &wh.condition, 0)?.append(" THEN ").expr(d, &wh.then, 0)?;
    }
    if let Some(o) = otherwise {
        w.append(" ELSE ").expr(d, o, 0)?;
    }
    w.append(" END");
    Ok(())
}

pub fn window_call<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    f: &SqlAgg,
    partition_by: &[SqlExpr],
    order_by: &[SortKey],
    frame: Option<&Frame>,
) -> Result<(), Refusal> {
    match f {
        SqlAgg::Reducer(r) => d.reducer(w, r)?,
        SqlAgg::RankingFn { r#fn, args } | SqlAgg::ValueFn { r#fn, args } => {
            let name = d.aggregate_name(*r#fn);
            w.append(&name).append("(").list(d, args)?.append(")");
        }
    }
    over(d, w, partition_by, order_by, frame)
}

/// A window's `OVER (...)`: its partition, order and frame.
pub fn over<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    partition_by: &[SqlExpr],
    order_by: &[SortKey],
    frame: Option<&Frame>,
) -> Result<(), Refusal> {
    let kw = |k: &str| d.keyword(k);
    w.append(" ").append(&kw("OVER")).append(" (");
    if !partition_by.is_empty() {
        w.append(&kw("PARTITION BY")).append(" ").list(d, partition_by)?;
    }
    if !order_by.is_empty() {
        if !partition_by.is_empty() {
            w.append(" ");
        }
        w.append(&kw("ORDER BY")).append(" ").join(order_by, ", ", |w, k| d.sort_key(w, k))?;
    }
    if let Some(fr) = frame {
        let text = format!(
            " {} {} {} {} {}",
            kw(fr.kind.as_str()),
            kw("BETWEEN"),
            d.bound(&fr.from)?,
            kw("AND"),
            d.bound(&fr.to)?
        );
        w.append(&text);
    }
    w.append(")");
    Ok(())
}

/// A window frame bound.
pub fn bound<D: AnsiSqlRenderer>(d: &D, b: &Bound) -> String {
    match b {
        Bound::UnboundedPreceding => d.keyword("UNBOUNDED PRECEDING"),
        Bound::Preceding { n } => format!("{} {}", number_text(n), d.keyword("PRECEDING")),
        Bound::CurrentRow => d.keyword("CURRENT ROW"),
        Bound::Following { n } => format!("{} {}", number_text(n), d.keyword("FOLLOWING")),
        Bound::UnboundedFollowing => d.keyword("UNBOUNDED FOLLOWING"),
        Bound::IntervalPreceding { n, unit } => {
            format!("{} {n} {unit} {}", d.keyword("INTERVAL"), d.keyword("PRECEDING"))
        }
        Bound::IntervalFollowing { n, unit } => {
            format!("{} {n} {unit} {}", d.keyword("INTERVAL"), d.keyword("FOLLOWING"))
        }
    }
}

/// A Java `Number`'s `toString`.
fn number_text(n: &JavaNumber) -> String {
    match n {
        JavaNumber::Long(v) => v.to_string(),
        JavaNumber::Integer(v) => v.to_string(),
        JavaNumber::Double(v) => lite_javacompat::double_to_string(v.value()),
        JavaNumber::BigDecimal(v) => v.as_str().to_owned(),
    }
}

/// An aggregate as most databases write it: `name([DISTINCT ]args[ ORDER BY keys])`, `*` for no argument.
pub fn aggregate<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, name: &str, r: &Reducer) -> Result<(), Refusal> {
    let distinct = if r.distinct { format!("{} ", d.keyword("DISTINCT")) } else { String::new() };
    w.append(name).append("(").append(&distinct);
    if r.args.is_empty() {
        w.append("*");
    } else {
        w.list(d, &r.args)?;
    }
    if !r.order_by.is_empty() {
        let kw = d.keyword("ORDER BY");
        w.append(" ").append(&kw).append(" ").join(&r.order_by, ", ", |w, k| aggregate_order_key(d, w, k))?;
    }
    w.append(")");
    Ok(())
}

/// A catalog grid's query: refused unless the dialect's database is proven to answer it.
pub fn catalog<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, catalog: &SqlSource) -> Result<(), Refusal> {
    let _ = (d, w);
    let grid = match catalog {
        SqlSource::Catalog { grid, .. } => grid.as_str(),
        _ => "?",
    };
    Err(Refusal::capability(format!(
        "a {grid} catalog grid (fetchDb*MetaData) reached a dialect without a catalog reading"
    )))
}

/// A lowering-internal pseudo-aggregate reached a renderer: the lowering's bug.
pub fn lowering_marker(r: &Reducer) -> Refusal {
    Refusal::illegal_state(format!("lowering marker {} reached the renderer", r.r#fn))
}

/// A window function as an aggregate: it is a ranking or value function, which `window_call` writes.
pub fn window_function(r: &Reducer) -> Refusal {
    Refusal::illegal_state(format!("{} is a window function: windowCall writes it", r.r#fn))
}

/// A key of an aggregate's own ordering.
pub fn aggregate_order_key<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, k: &SortKey) -> Result<(), Refusal> {
    w.expr(d, &k.expr, 0)?;
    let text = format!(" {}{}", d.keyword(if k.ascending { "ASC" } else { "DESC" }), d.agg_order_null_placement(k));
    w.append(&text);
    Ok(())
}

/// A key with declared null placement keeps it inside the aggregate.
pub fn agg_order_null_placement(k: &SortKey) -> String {
    match k.null_order {
        None => String::new(),
        Some(NullOrder::NullsFirst) => " NULLS FIRST".to_owned(),
        Some(NullOrder::NullsLast) => " NULLS LAST".to_owned(),
    }
}

// ==================================================================
// Lexical
// ==================================================================

/// A string literal; a NUL in the value is spliced as `chr(0)` between quoted segments.
pub fn string_lit(value: &str) -> String {
    if value.contains('\0') {
        let mut sb = String::from("(");
        for (i, part) in value.split('\0').enumerate() {
            if i > 0 {
                sb.push_str(" || chr(0) || ");
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

/// SQL type to CAST spelling: the scalar leaves the dialect's, the composite rules shared.
pub fn cast_type_name<D: AnsiSqlRenderer>(d: &D, t: &SqlType) -> Result<String, Refusal> {
    match t {
        SqlType::Scalar(s) => d.scalar_type_name(*s),
        SqlType::Decimal { precision, scale } => Ok(format!("DECIMAL({precision}, {scale})")),
        SqlType::Array { element } => Ok(d.array_type_name(&cast_type_name(d, element)?)),
        SqlType::Map { key, value } => {
            let k = cast_type_name(d, key)?;
            let v = cast_type_name(d, value)?;
            Ok(d.map_type_name(&k, &v))
        }
        SqlType::Struct { fields } => d.struct_type_name(fields),
        SqlType::Tagged { .. } => Err(Refusal::illegal_state(
            "a tagged value is never cast or stored: its tag and slots are its query's columns \
             (docs/RESULTS_AND_RENDERERS_2026_10_22.md §5a)",
        )),
    }
}

/// A struct type's fields, each its name spelled and its type spelled.
pub fn struct_fields<D: AnsiSqlRenderer>(d: &D, fields: &[StructField]) -> Result<Vec<(String, String)>, Refusal> {
    fields.iter().map(|f| Ok((d.ident(&f.name), cast_type_name(d, &f.r#type)?))).collect()
}

/// A date format in strftime codes.
pub fn strftime_text(parts: &[DateFmt]) -> String {
    let mut out = String::new();
    for p in parts {
        out.push_str(match p {
            DateFmt::Text { s } => s,
            DateFmt::Part(part) => match part {
                DateFmtPart::Year4 => "%Y",
                DateFmtPart::Month2 => "%m",
                DateFmtPart::Day2 => "%d",
                DateFmtPart::Hour2 => "%H",
                DateFmtPart::Min2 => "%M",
                DateFmtPart::Sec2 => "%S",
                DateFmtPart::SubsecMicro => "%f",
                DateFmtPart::SubsecNano => "%n",
                DateFmtPart::SubsecMin => "%g",
                DateFmtPart::MonthAbbrev => "%b",
                DateFmtPart::MonthName => "%B",
                DateFmtPart::WeekdayName => "%A",
                DateFmtPart::Hour12 => "%I",
                DateFmtPart::Hour12Nopad => "%-I",
                DateFmtPart::Ampm => "%p",
            },
        });
    }
    out
}

/// A plan parameter's placeholder: the value bound bare -- but a date-time, cast to a type hole the runner fills.
pub fn placeholder<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, p: &PlanParam<'_>) -> Result<(), Refusal> {
    if let Some(by_value) = p.by_value
        && by_value.kinds.iter().any(|k| matches!(k, ValueKind::DateTime | ValueKind::DateTimeNanos))
    {
        let mut types = Vec::new();
        for kind in &by_value.kinds {
            types.push((*kind, d.hole_type(*kind)?));
        }
        let bind = scalar_bind(p)?;
        w.append("CAST(").bind(bind).append(" AS ").type_hole(enum_map(types), by_value.absent)?.append(")");
        return Ok(());
    }
    let bind = scalar_bind(p)?;
    w.bind(bind);
    Ok(())
}

/// The type this dialect's database gives a literal of a value of `kind`: a date's and a date-time's here.
pub fn hole_type(kind: ValueKind) -> Result<TypeSpelling, Refusal> {
    match kind {
        ValueKind::Date => Ok(TypeSpelling::new("DATE", Digits::None)),
        ValueKind::DateTime => Ok(TypeSpelling::new("TIMESTAMP", Digits::None)),
        ValueKind::DateTimeNanos => {
            Err(Refusal::capability("a date-time finer than a microsecond: this dialect names no type its literal has"))
        }
        ValueKind::Integer | ValueKind::Decimal | ValueKind::Floating => Err(Refusal::capability(format!(
            "a value of kind {kind} is bound bare here, typed by the database from the value"
        ))),
    }
}

/// A plan parameter bound as one value; what one value cannot carry is refused by name.
pub fn scalar_bind(p: &PlanParam<'_>) -> Result<Bind, Refusal> {
    if p.kind == ParamKind::Raw {
        return Err(Refusal::capability(format!(
            "plan parameter '{}' is RAW: it splices plan text, never a bound value",
            p.name
        )));
    }
    if p.enum_map_fn.is_some() {
        return Err(Refusal::capability(format!(
            "enum plan parameter '{}' carries the legacy printer's mapping function (a value table, a plan \
             template's): never a bound value",
            p.name
        )));
    }
    Ok(Bind { parameter: p.name.to_owned(), binding: Binding::One { null_type: null_type(p)?, hole: None } })
}

/// The JDBC type an absent value of `p` is bound as.
pub fn null_type(p: &PlanParam<'_>) -> Result<String, Refusal> {
    if let TypeFact::Typed { r#type, .. } = p.r#type {
        return jdbc_type(r#type, p.name);
    }
    let Some(by_value) = p.by_value else {
        return Err(Refusal::capability(format!(
            "plan parameter '{}' has no type and is not typed by its value: not bound",
            p.name
        )));
    };
    Ok(match by_value.absent {
        ValueKind::Integer => "BIGINT",
        ValueKind::Decimal => "DECIMAL",
        ValueKind::Floating => "DOUBLE",
        ValueKind::Date => "DATE",
        ValueKind::DateTime | ValueKind::DateTimeNanos => "TIMESTAMP",
    }
    .to_owned())
}

/// `t`'s JDBC type name, for a parameter's null.
fn jdbc_type(t: &SqlType, parameter: &str) -> Result<String, Refusal> {
    let refuse = |what: &str| Err(Refusal::capability(format!("plan parameter '{parameter}' is {what}")));
    match t {
        SqlType::Scalar(s) => match s {
            Scalar::Boolean
            | Scalar::Integer
            | Scalar::Bigint
            | Scalar::Double
            | Scalar::Varchar
            | Scalar::Date
            | Scalar::Timestamp
            | Scalar::Time => Ok(s.as_str().to_owned()),
            Scalar::Hugeint | Scalar::Timestamptz | Scalar::Json | Scalar::TemporalText | Scalar::DecimalText => {
                refuse(&format!("typed {s}, which no parameter is bound as"))
            }
        },
        SqlType::Decimal { .. } => Ok("DECIMAL".to_owned()),
        SqlType::Array { .. } => refuse("a list: bound as one array, never as one value"),
        SqlType::Map { .. } => refuse("a map, which no parameter is bound as"),
        SqlType::Struct { .. } => refuse("a struct, which no parameter is bound as"),
        SqlType::Tagged { .. } => refuse("a tagged value, which no parameter is bound as"),
    }
}

/// `needle = ANY(?)`: a list parameter bound as one array of its element type.
pub fn any_of<D: AnsiSqlRenderer>(
    d: &D,
    w: &mut SqlWriter,
    needle: &SqlExpr,
    list: &PlanParam<'_>,
    parent_prec: i32,
) -> Result<(), Refusal> {
    let TypeFact::Typed { r#type: SqlType::Array { element }, .. } = list.r#type else {
        return Err(not_a_list(list));
    };
    let SqlType::Scalar(element) = element.as_ref() else {
        return Err(not_a_list(list));
    };
    let wrap = 4 < parent_prec;
    if wrap {
        w.append("(");
    }
    let bind = Bind {
        parameter: list.name.to_owned(),
        binding: Binding::Array { element_sql_type: element.as_str().to_owned() },
    };
    w.expr(d, needle, 5)?.append(" = ANY(").bind(bind).append(")");
    if wrap {
        w.append(")");
    }
    Ok(())
}

fn not_a_list(list: &PlanParam<'_>) -> Refusal {
    Refusal::capability(format!(
        "plan parameter '{}' is not a list parameter (an array of a named element type): it is not bound as a whole \
         list",
        list.name
    ))
}

/// A parameter written where one value goes: a list parameter is refused by name.
fn one_value<'a>(p: &PlanParam<'a>) -> Result<PlanParam<'a>, Refusal> {
    if let TypeFact::Typed { r#type: SqlType::Array { .. }, .. } = p.r#type {
        return Err(Refusal::capability(format!(
            "list parameter '{}' is bound only as a whole list (in, contains); written where one value goes, it is \
             not bound",
            p.name
        )));
    }
    Ok(*p)
}

/// A composite arm whose spelling expands to operator text: parenthesized whenever an enclosing operator binds.
pub fn op(
    w: &mut SqlWriter,
    parent_prec: i32,
    body: &dyn Fn(&mut SqlWriter) -> Result<(), Refusal>,
) -> Result<(), Refusal> {
    if parent_prec > 0 {
        w.append("(");
    }
    body(w)?;
    if parent_prec > 0 {
        w.append(")");
    }
    Ok(())
}

/// A subquery written inline (EXISTS / scalar position): single-line mode while it is written.
pub fn inline<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, q: &SqlQuery) -> Result<(), Refusal> {
    let previous = d.ansi_state().inline.replace(true);
    let written = d.query(w, q, 0);
    d.ansi_state().inline.set(previous);
    written
}

/// A table name; a schema-qualified one quotes each part unconditionally.
pub fn table_name<D: AnsiSqlRenderer>(d: &D, name: &str, quoting: NameQuoting) -> String {
    match name.find('.') {
        Some(dot) if dot > 0 => format!("{}.{}", d.delimited(&name[..dot]), d.delimited(&name[dot + 1..])),
        _ => {
            if quoting.table {
                d.delimited(name)
            } else {
                d.ident(name)
            }
        }
    }
}

/// A column name at a reference: a declared-quoted physical name delimited, any other by `ident`.
pub fn column_name<D: AnsiSqlRenderer>(d: &D, c: &Column) -> String {
    if c.origin == Some(Origin::PhysicalQuoted) { d.delimited(&c.name) } else { d.ident(&c.name) }
}

/// `name` as a delimited identifier: always quoted, the quote doubled inside.
pub fn delimited<D: AnsiSqlRenderer>(d: &D, name: &str) -> String {
    let q = d.lexicon().quote_char();
    let mut doubled = String::new();
    doubled.push(q);
    doubled.push(q);
    format!("{q}{}{q}", name.replace(q, &doubled))
}

/// Quote only when necessary, per this dialect's lexicon.
pub fn ident<D: AnsiSqlRenderer>(d: &D, name: &str) -> String {
    if is_plain(name) && !d.lexicon().is_reserved(&name.to_ascii_lowercase()) {
        return name.to_owned();
    }
    let q = d.lexicon().quote_char();
    let mut qq = String::new();
    qq.push(q);
    qq.push(q);
    // a quote-bearing identity is already its own spelling, when its interior is a valid quoted body
    if name.len() > 1 && name.starts_with(q) && name.ends_with(q) {
        let interior = &name[q.len_utf8()..name.len() - q.len_utf8()];
        if !interior.replace(&qq, "").contains(q) {
            return name.to_owned();
        }
    }
    format!("{q}{}{q}", name.replace(q, &qq))
}

/// `[A-Za-z_][A-Za-z0-9_]*`, the whole name.
fn is_plain(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(b) if b.is_ascii_alphabetic() || b == b'_' => bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        _ => false,
    }
}

/// A clause separator: a newline and the depth's indent, or one space in single-line mode.
pub fn nl<D: AnsiSqlRenderer>(d: &D, w: &mut SqlWriter, depth: usize) {
    if d.ansi_state().inline.get() {
        w.append(" ");
    } else {
        w.append("\n").append(&"  ".repeat(depth));
    }
}

// ==================================================================
// DDL and DML
// ==================================================================

pub fn render_ddl<D: AnsiSqlRenderer>(d: &D, ddl: &SqlDdl) -> Result<String, Refusal> {
    Ok(match ddl {
        SqlDdl::CreateTable { schema, table, quoting, columns, temporary } => {
            let mut sb = String::from(if *temporary { "Create Temporary Table " } else { "Create Table " });
            sb.push_str(&ddl_qualified(d, schema.as_deref(), table, *quoting));
            sb.push('(');
            for (i, col) in columns.iter().enumerate() {
                if i > 0 {
                    sb.push_str(&d.ddl_column_separator());
                }
                sb.push_str(&d.ddl_identifier(&col.name, col.declared_quoted));
                sb.push(' ');
                sb.push_str(&d.ddl_type(&col.r#type)?);
                sb.push_str(&d.ddl_nullability(col.primary_key || col.not_null));
            }
            let pks: Vec<String> = columns
                .iter()
                .filter(|c| c.primary_key)
                .map(|c| d.ddl_key_identifier(&c.name, c.declared_quoted))
                .collect();
            if !pks.is_empty() {
                // the engine joins the pk names with a bare comma
                sb.push_str(", PRIMARY KEY(");
                sb.push_str(&pks.join(","));
                sb.push(')');
            }
            sb.push_str(");");
            sb
        }
        SqlDdl::DropTable { schema, table, quoting } => {
            format!("Drop table if exists {};", ddl_qualified(d, schema.as_deref(), table, *quoting))
        }
        SqlDdl::CreateSchema { schema, quoted } => {
            format!("Create Schema if not exists {};", d.stored_name(schema, *quoted))
        }
        SqlDdl::DropSchema { schema, quoted } => {
            format!("Drop schema if exists {} cascade;", d.stored_name(schema, *quoted))
        }
    })
}

pub fn render_dml<D: AnsiSqlRenderer>(d: &D, dml: &SqlDml) -> Result<String, Refusal> {
    match dml {
        SqlDml::InsertValues { schema, table, quoting, columns, quoted_columns, rows } => {
            let mut w = SqlWriter::new();
            let head = format!(
                "INSERT INTO {}{} VALUES ",
                ddl_qualified(d, schema.as_deref(), table, *quoting),
                dml_columns(d, columns, quoted_columns)
            );
            w.append(&head);
            for (r, row) in rows.iter().enumerate() {
                w.append(if r == 0 { "(" } else { ", (" }).list(d, row)?.append(")");
            }
            w.append(";");
            let insert = w.into_statement()?;
            if !insert.binds.is_empty() {
                return Err(Refusal::capability(format!(
                    "plan parameters {} in a DML row: a row holds values, and a DML statement is text",
                    list_text(&insert.binds)
                )));
            }
            Ok(insert.sql)
        }
        SqlDml::InsertFromTable { schema, table, quoting, columns, quoted_columns, source } => Ok(format!(
            "INSERT INTO {}{} SELECT * FROM {};",
            ddl_qualified(d, schema.as_deref(), table, *quoting),
            dml_columns(d, columns, quoted_columns),
            d.ident(source)
        )),
        SqlDml::DeleteAll { schema, table, quoting } => {
            Ok(format!("DELETE FROM {};", ddl_qualified(d, schema.as_deref(), table, *quoting)))
        }
        SqlDml::CreateTempTableAs { .. } => d.temp_table_as(dml)?.into_text(),
        SqlDml::InsertSelect { .. } => d.insert_select(dml)?.into_text(),
        SqlDml::Update { .. } => d.update(dml)?.into_text(),
        SqlDml::DeleteWhere { .. } => d.delete_where(dml)?.into_text(),
        SqlDml::Merge { .. } => d.merge(dml)?.into_text(),
    }
}

pub fn render_dml_statement<D: AnsiSqlRenderer>(d: &D, dml: &SqlDml) -> Result<RenderedStatement, Refusal> {
    match dml {
        // a seed's rows, a copy, a table emptied: setup's statements, which bind no value
        SqlDml::InsertValues { .. } | SqlDml::InsertFromTable { .. } | SqlDml::DeleteAll { .. } => {
            Ok(RenderedStatement::plain(d.render_dml(dml)?))
        }
        SqlDml::CreateTempTableAs { .. } => d.temp_table_as(dml)?.into_statement(),
        SqlDml::InsertSelect { .. } => d.insert_select(dml)?.into_statement(),
        SqlDml::Update { .. } => d.update(dml)?.into_statement(),
        SqlDml::DeleteWhere { .. } => d.delete_where(dml)?.into_statement(),
        SqlDml::Merge { .. } => d.merge(dml)?.into_statement(),
    }
}

/// A store write no spelling is measured for on this dialect's database.
pub fn unmeasured_write<D: AnsiSqlRenderer>(d: &D, what: &str) -> Refusal {
    Refusal::capability(format!(
        "{what} on {}: no spelling measured on its database (the store writes run on DuckDB, H2 and Postgres, the \
         writes design's leg 2)",
        d.simple_name()
    ))
}

fn not_the_statement(what: &str) -> Refusal {
    Refusal::illegal_state(format!("{what}: not the statement this writer renders"))
}

/// The standard `INSERT INTO t (columns) query`, the query written with its passes applied.
pub fn standard_insert_select<D: AnsiSqlRenderer>(d: &D, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
    let SqlDml::InsertSelect { schema, table, quoting, columns, quoted_columns, query } = dml else {
        return Err(not_the_statement("an INSERT of a query's rows"));
    };
    let mut w = SqlWriter::new();
    let head = format!(
        "INSERT INTO {}{} ",
        ddl_qualified(d, schema.as_deref(), table, *quoting),
        dml_columns(d, columns, quoted_columns)
    );
    w.append(&head);
    d.query(&mut w, &passed(d, query.clone())?, 0)?;
    w.append(";");
    Ok(w)
}

/// The standard `UPDATE t AS alias SET c = value, ... [WHERE condition]`.
pub fn standard_update<D: AnsiSqlRenderer>(d: &D, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
    let SqlDml::Update { schema, table, quoting, alias, stored_types, assignments, r#where } = dml else {
        return Err(not_the_statement("an UPDATE"));
    };
    let values: Vec<SqlExpr> = assignments.iter().map(|a| a.value.clone()).collect();
    let read = stored_reads_over::<D>(table, alias, stored_types, values, r#where.clone())?;
    let mut w = SqlWriter::new();
    let head =
        format!("UPDATE {} AS {} SET ", ddl_qualified(d, schema.as_deref(), table, *quoting), d.alias_ident(alias));
    w.append(&head);
    for (i, a) in assignments.iter().enumerate() {
        let column = d.stored_name(&a.column, a.quoted);
        w.append(if i == 0 { "" } else { ", " }).append(&column).append(" = ").expr(d, &read.projections[i].expr, 0)?;
    }
    if let Some(wh) = &read.r#where {
        w.append(" WHERE ").expr(d, wh, 0)?;
    }
    w.append(";");
    Ok(w)
}

/// The standard `DELETE FROM t AS alias [WHERE condition]`.
pub fn standard_delete_where<D: AnsiSqlRenderer>(d: &D, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
    let SqlDml::DeleteWhere { schema, table, quoting, alias, stored_types, r#where } = dml else {
        return Err(not_the_statement("a DELETE ... WHERE"));
    };
    let read = stored_reads_over::<D>(table, alias, stored_types, Vec::new(), r#where.clone())?;
    let mut w = SqlWriter::new();
    let head =
        format!("DELETE FROM {} AS {}", ddl_qualified(d, schema.as_deref(), table, *quoting), d.alias_ident(alias));
    w.append(&head);
    if let Some(wh) = &read.r#where {
        w.append(" WHERE ").expr(d, wh, 0)?;
    }
    w.append(";");
    Ok(w)
}

/// An UPDATE's or a DELETE's expressions in the scope of their one table, read as a query reads it: the
/// `StoredReads` pass over `SELECT values FROM table AS alias WHERE where`, its projections and condition read back.
/// (Java's select is `SqlSelect.starOf(table)`, its outputs the table's; the table here has none, and nothing reads
/// them.)
fn stored_reads_over<D: AnsiSqlRenderer>(
    table: &str,
    alias: &str,
    stored_types: &[(String, ColumnType)],
    values: Vec<SqlExpr>,
    r#where: Option<SqlExpr>,
) -> Result<SqlSelect, Refusal> {
    let mut ps: Vec<Projection> = values
        .into_iter()
        .enumerate()
        .map(|(i, v)| Projection { expr: v, alias: Some(format!("__v{i}")), out: None })
        .collect();
    if ps.is_empty() {
        ps.push(Projection { expr: crate::build::int_lit(1), alias: Some("__v".to_owned()), out: None });
    }
    let scope = SqlSelect {
        projections: ps,
        distinct: false,
        from: SqlSource::Table {
            name: table.to_owned(),
            alias: alias.to_owned(),
            outputs: Vec::new(),
            call: false,
            stored_types: stored_types.to_vec(),
            quoting: NameQuoting::default(),
        },
        r#where,
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by: Vec::new(),
        limit: None,
        offset: None,
        outputs: Vec::new(),
    };
    match StoredReads::new(D::reads_stored).rewrite_root(SqlQuery::Select(Box::new(scope)))? {
        SqlQuery::Select(s) => Ok(*s),
        _ => Err(Refusal::class_cast("SqlUnion", "SqlSelect")),
    }
}

/// The standard `MERGE INTO t AS alias USING (query) AS source ON ... WHEN MATCHED ... WHEN NOT MATCHED ...`.
pub fn standard_merge<D: AnsiSqlRenderer>(d: &D, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
    let SqlDml::Merge { schema, table, quoting, columns, quoted_columns, keys, query, alias, source_alias } = dml
    else {
        return Err(not_the_statement("a MERGE"));
    };
    let target = d.alias_ident(alias);
    let outputs = query_outputs(query);
    let read = |name: &str| -> Result<String, Refusal> {
        let Some(oc) = outputs.iter().find(|o| o.name == name) else {
            return Err(Refusal::illegal_state(format!(
                "a MERGE's source has no column '{name}' {}",
                output_cols_text(outputs)
            )));
        };
        d.column_ref(&column_of(Some(source_alias), oc))
    };
    let mut w = SqlWriter::new();
    let head = format!("MERGE INTO {} AS {target} USING (", ddl_qualified(d, schema.as_deref(), table, *quoting));
    w.append(&head);
    d.query(&mut w, &passed(d, query.clone())?, 0)?;
    let source = d.alias_ident(source_alias);
    w.append(") AS ").append(&source).append(" ON ");
    for (i, k) in keys.iter().enumerate() {
        let column = d.stored_name(k, quoted_columns.contains(k));
        let value = read(k)?;
        w.append(if i == 0 { "" } else { " AND " })
            .append(&target)
            .append(".")
            .append(&column)
            .append(" = ")
            .append(&value);
    }
    let others: Vec<&String> = columns.iter().filter(|c| !keys.contains(c)).collect();
    if !others.is_empty() {
        w.append(" WHEN MATCHED THEN UPDATE SET ");
        for (i, c) in others.iter().enumerate() {
            let column = d.stored_name(c, quoted_columns.contains(c));
            let value = read(c)?;
            w.append(if i == 0 { "" } else { ", " }).append(&column).append(" = ").append(&value);
        }
    }
    let insert = dml_columns(d, columns, quoted_columns);
    w.append(" WHEN NOT MATCHED THEN INSERT").append(&insert).append(" VALUES (");
    for (i, c) in columns.iter().enumerate() {
        let value = read(c)?;
        w.append(if i == 0 { "" } else { ", " }).append(&value);
    }
    w.append(");");
    Ok(w)
}

/// Java's `SqlQuery.outputs()`: a select's and a union's own, a WITH's body's.
pub fn query_outputs(q: &SqlQuery) -> &[OutputCol] {
    match q {
        SqlQuery::Select(s) => &s.outputs,
        SqlQuery::Union(u) => &u.outputs,
        SqlQuery::With(w) => query_outputs(&w.body),
    }
}

/// Java's `SqlExpr.Column.of(table, OutputCol)`: the stamped reference to a source's declared output.
pub fn column_of(table: Option<&str>, oc: &OutputCol) -> Column {
    Column {
        table: table.map(str::to_owned),
        name: oc.name.clone(),
        r#type: TypeFact::Typed { r#type: oc.r#type.clone(), nullable: oc.nullable },
        origin: Some(oc.origin),
    }
}

/// The standard `CREATE TEMPORARY TABLE t AS <query>`.
pub fn create_temporary_table_as<D: AnsiSqlRenderer>(
    d: &D,
    dml: &SqlDml,
    named: bool,
    create: &str,
    options: &str,
) -> Result<SqlWriter, Refusal> {
    let SqlDml::CreateTempTableAs { table, query, columns, .. } = dml else {
        return Err(not_the_statement("a temporary table from a query"));
    };
    let mut w = SqlWriter::new();
    let name = d.table_name(table, NameQuoting::default());
    w.append(create).append(&name);
    if named && !columns.is_empty() {
        let names: Vec<String> = columns.iter().map(|c| d.delimited(c)).collect();
        w.append(" (").append(&names.join(", ")).append(")");
    }
    w.append(options).append(" AS ");
    d.query(&mut w, &passed(d, query.clone())?, 0)?;
    w.append(";");
    Ok(w)
}

/// `" (a, b)"`, each column a `stored_name`, or empty for every column.
fn dml_columns<D: AnsiSqlRenderer>(d: &D, columns: &[String], quoted: &[String]) -> String {
    if columns.is_empty() {
        return String::new();
    }
    let names: Vec<String> = columns.iter().map(|c| d.stored_name(c, quoted.contains(c))).collect();
    format!(" ({})", names.join(", "))
}

/// `schema.table`, each name spelled as a query references it; the default schema spells bare.
fn ddl_qualified<D: AnsiSqlRenderer>(d: &D, schema: Option<&str>, table: &str, quoting: NameQuoting) -> String {
    match schema {
        None | Some("") | Some("default") => d.stored_name(table, quoting.table),
        Some(s) => format!("{}.{}", d.stored_name(s, quoting.schema), d.stored_name(table, quoting.table)),
    }
}

/// A DECIMAL's text of `10^-n` and its half and twice, as `BigDecimal.ONE.movePointLeft(n)`, its `divide(2)` and
/// its `multiply(2)` write them (`0.01`, `0.005`, `0.02`).
pub fn decimal_unit(n: i64) -> (String, String, String) {
    (plain(false, "1", n), plain(false, "5", n + 1), plain(false, "2", n))
}

/// The lambda's parameters as DuckDB writes them: one bare, several parenthesized.
pub fn lambda_params(l: &Lambda) -> String {
    if l.params.len() == 1 { l.params[0].clone() } else { format!("({})", l.params.join(", ")) }
}
