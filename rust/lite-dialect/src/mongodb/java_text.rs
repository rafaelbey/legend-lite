// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The MIR's records as Java's record `toString` writes them (`Name[component=value, ...]`, a list `[a, b]`, a null
//! `null`, an enum its `name()`), which MongoDb's lowering-bug refusals embed whole: an as-of join's match, a pivot
//! value that is no literal, a list operator's argument that is no lambda. Every expression record is written
//! component for component; a subquery's records likewise, a table's stored types as a map's `{name=type}` in their
//! stored order.

use crate::java::{column_type_text, output_cols_text, sql_type_text, type_fact_text};
use lite_sql::{
    Bound, Column, DateFmt, Frame, JavaNumber, Lambda, Projection, Reducer, SortKey, SqlAgg, SqlExpr, SqlQuery,
    SqlSelect, SqlSource, ValueTyping,
};

/// A list of strings as Java's `List.toString` writes it.
pub fn strings(items: &[String]) -> String {
    format!("[{}]", items.join(", "))
}

/// A list of expressions as Java's `List.toString` writes it.
pub fn exprs(items: &[SqlExpr]) -> String {
    list(items, expr)
}

fn list<T>(items: &[T], each: impl Fn(&T) -> String) -> String {
    let parts: Vec<String> = items.iter().map(each).collect();
    format!("[{}]", parts.join(", "))
}

fn opt(s: Option<&str>) -> String {
    s.unwrap_or("null").to_owned()
}

fn opt_expr(e: Option<&SqlExpr>) -> String {
    e.map_or_else(|| "null".to_owned(), expr)
}

fn column(c: &Column) -> String {
    format!(
        "Column[table={}, name={}, type={}, origin={}]",
        opt(c.table.as_deref()),
        c.name,
        type_fact_text(&c.r#type),
        c.origin.map_or_else(|| "null".to_owned(), |o| o.as_str().to_owned())
    )
}

fn lambda(l: &Lambda) -> String {
    format!("Lambda[params={}, body={}, type={}]", strings(&l.params), expr(&l.body), type_fact_text(&l.r#type))
}

fn sort_key(k: &SortKey) -> String {
    format!(
        "SortKey[expr={}, ascending={}, nullOrder={}, outputName={}]",
        expr(&k.expr),
        k.ascending,
        k.null_order.map_or_else(|| "null".to_owned(), |o| o.as_str().to_owned()),
        opt(k.output_name.as_deref())
    )
}

fn reducer(r: &Reducer) -> String {
    format!(
        "Reducer[fn={}, args={}, distinct={}, orderBy={}, type={}]",
        r.r#fn,
        exprs(&r.args),
        r.distinct,
        list(&r.order_by, sort_key),
        type_fact_text(&r.r#type)
    )
}

fn agg(a: &SqlAgg) -> String {
    match a {
        SqlAgg::Reducer(r) => reducer(r),
        SqlAgg::RankingFn { r#fn, args } => format!("RankingFn[fn={fn}, args={}]", exprs(args), fn = r#fn),
        SqlAgg::ValueFn { r#fn, args } => format!("ValueFn[fn={fn}, args={}]", exprs(args), fn = r#fn),
    }
}

fn number(n: &JavaNumber) -> String {
    match n {
        JavaNumber::Long(v) => v.to_string(),
        JavaNumber::Integer(v) => v.to_string(),
        JavaNumber::Double(v) => v.text().to_owned(),
        JavaNumber::BigDecimal(v) => v.as_str().to_owned(),
    }
}

fn bound(b: &Bound) -> String {
    match b {
        Bound::UnboundedPreceding => "UnboundedPreceding[]".to_owned(),
        Bound::Preceding { n } => format!("Preceding[n={}]", number(n)),
        Bound::CurrentRow => "CurrentRow[]".to_owned(),
        Bound::Following { n } => format!("Following[n={}]", number(n)),
        Bound::UnboundedFollowing => "UnboundedFollowing[]".to_owned(),
        Bound::IntervalPreceding { n, unit } => format!("IntervalPreceding[n={n}, unit={unit}]"),
        Bound::IntervalFollowing { n, unit } => format!("IntervalFollowing[n={n}, unit={unit}]"),
    }
}

fn frame(f: &Frame) -> String {
    format!("Frame[kind={}, from={}, to={}]", f.kind, bound(&f.from), bound(&f.to))
}

fn date_fmt(p: &DateFmt) -> String {
    match p {
        DateFmt::Part(part) => part.as_str().to_owned(),
        DateFmt::Text { s } => format!("Text[s={s}]"),
    }
}

fn value_typing(v: &ValueTyping) -> String {
    format!("ValueTyping[kinds={}, absent={}]", list(&v.kinds, |k| k.as_str().to_owned()), v.absent)
}

/// An expression's record `toString`.
pub fn expr(e: &SqlExpr) -> String {
    let t = |f| type_fact_text(f);
    match e {
        SqlExpr::RowOrder { table, r#type } => format!("RowOrder[table={}, type={}]", opt(table.as_deref()), t(r#type)),
        SqlExpr::StoredRead { column: c, stored, r#type } => {
            format!("StoredRead[column={}, stored={}, type={}]", column(c), column_type_text(stored), t(r#type))
        }
        SqlExpr::ReduceCollection { reducer, collection, extras, r#type } => format!(
            "ReduceCollection[reducer={reducer}, collection={}, extras={}, type={}]",
            expr(collection),
            exprs(extras),
            t(r#type)
        ),
        SqlExpr::Membership { needle, collection, r#type } => {
            format!("Membership[needle={}, collection={}, type={}]", expr(needle), expr(collection), t(r#type))
        }
        SqlExpr::Column(c) => column(c),
        SqlExpr::StarExcept { table, except, r#type } => {
            format!("StarExcept[table={}, except={}, type={}]", opt(table.as_deref()), strings(except), t(r#type))
        }
        SqlExpr::Star { table, r#type } => format!("Star[table={}, type={}]", opt(table.as_deref()), t(r#type)),
        SqlExpr::StringLit { value, r#type } => format!("StringLit[value={value}, type={}]", t(r#type)),
        SqlExpr::IntLit { value, r#type } => format!("IntLit[value={value}, type={}]", t(r#type)),
        SqlExpr::FloatLit { value, r#type } => {
            format!("FloatLit[value={}, type={}]", lite_javacompat::double_to_string(value.value()), t(r#type))
        }
        SqlExpr::DecimalLit { value, r#type } => {
            format!("DecimalLit[value={}, type={}]", crate::json::big_decimal_text(value.as_str()), t(r#type))
        }
        SqlExpr::BoolLit { value, r#type } => format!("BoolLit[value={value}, type={}]", t(r#type)),
        SqlExpr::NullLit { r#type } => format!("NullLit[type={}]", t(r#type)),
        SqlExpr::DateLit { iso, r#type } => format!("DateLit[iso={iso}, type={}]", t(r#type)),
        SqlExpr::Group { inner, r#type } => format!("Group[inner={}, type={}]", expr(inner), t(r#type)),
        SqlExpr::KeyCell { cell, r#type } => format!("KeyCell[cell={cell}, type={}]", t(r#type)),
        SqlExpr::PlanParam { name, kind, optional, enum_map_fn, r#type, by_value } => format!(
            "PlanParam[name={name}, kind={kind}, optional={optional}, enumMapFn={}, type={}, byValue={}]",
            opt(enum_map_fn.as_deref()),
            t(r#type),
            by_value.as_ref().map_or_else(|| "null".to_owned(), value_typing)
        ),
        SqlExpr::FormatLit { parts, r#type } => {
            format!("FormatLit[parts={}, type={}]", list(parts, date_fmt), t(r#type))
        }
        SqlExpr::DatePartLit { part, r#type } => format!("DatePartLit[part={part}, type={}]", t(r#type)),
        SqlExpr::JsonKindLit { kind, r#type } => format!("JsonKindLit[kind={kind}, type={}]", t(r#type)),
        SqlExpr::TypeNameLit { scalar, r#type } => format!("TypeNameLit[scalar={scalar}, type={}]", t(r#type)),
        SqlExpr::TimestampLit { iso, r#type } => format!("TimestampLit[iso={iso}, type={}]", t(r#type)),
        SqlExpr::TimeLit { iso, r#type } => format!("TimeLit[iso={iso}, type={}]", t(r#type)),
        SqlExpr::OrderedListAgg { value, order_by, r#type } => {
            format!("OrderedListAgg[value={}, orderBy={}, type={}]", expr(value), expr(order_by), t(r#type))
        }
        SqlExpr::ArrayLit { elements, r#type } => format!("ArrayLit[elements={}, type={}]", exprs(elements), t(r#type)),
        SqlExpr::StructLit { fields, r#type } => format!(
            "StructLit[fields={}, type={}]",
            list(fields, |f| format!(
                "Field[name={}, value={}, declared={}]",
                f.name,
                expr(&f.value),
                f.declared.as_ref().map_or_else(|| "null".to_owned(), sql_type_text)
            )),
            t(r#type)
        ),
        SqlExpr::StructGet { source, field, r#type } => {
            format!("StructGet[source={}, field={field}, type={}]", expr(source), t(r#type))
        }
        SqlExpr::Call { r#fn, args, r#type } => {
            format!("Call[fn={fn}, args={}, type={}]", exprs(args), t(r#type), fn = r#fn)
        }
        SqlExpr::Case { whens, otherwise, r#type } => format!(
            "Case[whens={}, otherwise={}, type={}]",
            list(whens, |w| format!("When[condition={}, then={}]", expr(&w.condition), expr(&w.then))),
            opt_expr(otherwise.as_deref()),
            t(r#type)
        ),
        SqlExpr::Exists { subquery, r#type } => format!("Exists[subquery={}, type={}]", query(subquery), t(r#type)),
        SqlExpr::InSubquery { value, subquery, r#type } => {
            format!("InSubquery[value={}, subquery={}, type={}]", expr(value), query(subquery), t(r#type))
        }
        SqlExpr::CheckedDefects { own, hoists, r#type } => format!(
            "CheckedDefects[own={}, hoists={}, type={}]",
            expr(own),
            list(hoists, |h| format!(
                "Hoist[path={}, envelope={}, toMany={}]",
                strings(&h.path),
                expr(&h.envelope),
                h.to_many
            )),
            t(r#type)
        ),
        SqlExpr::CheckedChildValue { envelope, to_many, r#type } => {
            format!("CheckedChildValue[envelope={}, toMany={to_many}, type={}]", expr(envelope), t(r#type))
        }
        SqlExpr::Quantified { value, comparison, quantifier, subquery, r#type } => format!(
            "Quantified[value={}, comparison={comparison}, quantifier={quantifier}, subquery={}, type={}]",
            expr(value),
            query(subquery),
            t(r#type)
        ),
        SqlExpr::ScalarSubquery { subquery, r#type } => {
            format!("ScalarSubquery[subquery={}, type={}]", query(subquery), t(r#type))
        }
        SqlExpr::CheckedOne { list: l, scalar_carrier, at_least_only, r#type } => format!(
            "CheckedOne[list={}, scalarCarrier={scalar_carrier}, atLeastOnly={at_least_only}, type={}]",
            expr(l),
            t(r#type)
        ),
        SqlExpr::CompactList { list: l, r#type } => format!("CompactList[list={}, type={}]", expr(l), t(r#type)),
        SqlExpr::DeferredTdsString { inner, alias, id, form, render_tds_null, r#type } => format!(
            "DeferredTdsString[inner={}, alias={alias}, id={id}, form={form}, renderTdsNull={render_tds_null}, \
             type={}]",
            select(inner),
            t(r#type)
        ),
        SqlExpr::TempTableInSplice { temp_table_name, r#type } => {
            format!("TempTableInSplice[tempTableName={temp_table_name}, type={}]", t(r#type))
        }
        SqlExpr::JsonObject { kv, r#type } => format!("JsonObject[kv={}, type={}]", exprs(kv), t(r#type)),
        SqlExpr::JsonArray { elements, r#type } => {
            format!("JsonArray[elements={}, type={}]", exprs(elements), t(r#type))
        }
        SqlExpr::JsonArrayAgg { value, order_keys, r#type } => format!(
            "JsonArrayAgg[value={}, orderKeys={}, type={}]",
            expr(value),
            list(order_keys, |k| format!("Key[expr={}, desc={}]", expr(&k.expr), k.desc)),
            t(r#type)
        ),
        SqlExpr::WindowCall { r#fn, partition_by, order_by, frame: f, r#type } => format!(
            "WindowCall[fn={}, partitionBy={}, orderBy={}, frame={}, type={}]",
            agg(r#fn),
            exprs(partition_by),
            list(order_by, sort_key),
            f.as_ref().map_or_else(|| "null".to_owned(), frame),
            t(r#type)
        ),
        SqlExpr::Lambda(l) => lambda(l),
        SqlExpr::Cast { value, target, conform, r#type } => format!(
            "Cast[value={}, target={}, conform={conform}, type={}]",
            expr(value),
            sql_type_text(target),
            t(r#type)
        ),
        SqlExpr::FoldCall { source, lambda: l, init, acc_is_list, homogeneous, r#type } => format!(
            "FoldCall[source={}, lambda={}, init={}, accIsList={acc_is_list}, homogeneous={homogeneous}, type={}]",
            expr(source),
            lambda(l),
            expr(init),
            t(r#type)
        ),
        SqlExpr::Reducer(r) => reducer(r),
    }
}

fn projection(p: &Projection) -> String {
    format!(
        "Projection[expr={}, alias={}, out={}]",
        expr(&p.expr),
        opt(p.alias.as_deref()),
        p.out.as_ref().map_or_else(|| "null".to_owned(), crate::java::output_col_text)
    )
}

fn opt_long(v: Option<i64>) -> String {
    v.map_or_else(|| "null".to_owned(), |n| n.to_string())
}

fn select(s: &SqlSelect) -> String {
    format!(
        "SqlSelect[projections={}, distinct={}, from={}, where={}, groupBy={}, having={}, qualify={}, orderBy={}, \
         limit={}, offset={}, outputs={}]",
        list(&s.projections, projection),
        s.distinct,
        source(&s.from),
        opt_expr(s.r#where.as_ref()),
        exprs(&s.group_by),
        opt_expr(s.having.as_ref()),
        opt_expr(s.qualify.as_ref()),
        list(&s.order_by, sort_key),
        opt_long(s.limit),
        opt_long(s.offset),
        output_cols_text(&s.outputs)
    )
}

/// A query's record `toString`.
pub fn query(q: &SqlQuery) -> String {
    match q {
        SqlQuery::Select(s) => select(s),
        SqlQuery::Union(u) => format!(
            "SqlUnion[branches={}, all={}, outputs={}]",
            list(&u.branches, query),
            u.all,
            output_cols_text(&u.outputs)
        ),
        SqlQuery::With(w) => format!(
            "SqlWith[ctes={}, body={}, recursive={}]",
            list(&w.ctes, |c| format!(
                "Cte[name={}, query={}, materialized={}]",
                c.name,
                query(&c.query),
                c.materialized
            )),
            query(&w.body),
            w.recursive
        ),
    }
}

/// A source's record `toString`.
pub fn source(s: &SqlSource) -> String {
    match s {
        SqlSource::Table { name, alias, outputs, call, stored_types, quoting } => format!(
            "Table[name={name}, alias={alias}, outputs={}, call={call}, storedTypes={{{}}}, \
             quoting=NameQuoting[schema={}, table={}]]",
            output_cols_text(outputs),
            stored_types.iter().map(|(k, v)| format!("{k}={}", column_type_text(v))).collect::<Vec<_>>().join(", "),
            quoting.schema,
            quoting.table
        ),
        SqlSource::Subselect { inner, alias, frame_name } => {
            format!("Subselect[inner={}, alias={alias}, frameName={}]", query(inner), opt(frame_name.as_deref()))
        }
        SqlSource::Cte { name, alias, outputs } => {
            format!("Cte[name={name}, alias={alias}, outputs={}]", output_cols_text(outputs))
        }
        SqlSource::Join { left, right, kind, on, outputs } => format!(
            "Join[left={}, right={}, kind={kind}, on={}, outputs={}]",
            source(left),
            source(right),
            opt_expr(on.as_deref()),
            output_cols_text(outputs)
        ),
        SqlSource::Pivot { source: src, on, r#in, named, usings, alias, outputs } => format!(
            "Pivot[source={}, on={}, in={}, named={}, usings={}, alias={alias}, outputs={}]",
            source(src),
            exprs(on),
            exprs(r#in),
            exprs(named),
            list(usings, |u| format!(
                "Using[agg={}, alias={}, type={}]",
                reducer(&u.agg),
                u.alias,
                u.r#type.as_ref().map_or_else(|| "null".to_owned(), sql_type_text)
            )),
            output_cols_text(outputs)
        ),
        SqlSource::Values { rows, columns, alias, outputs } => format!(
            "Values[rows={}, columns={}, alias={alias}, outputs={}]",
            list(rows, |r| exprs(r)),
            strings(columns),
            output_cols_text(outputs)
        ),
        SqlSource::Dual => "Dual[]".to_owned(),
        SqlSource::SourceUrl { url, alias, outputs } => {
            format!("SourceUrl[url={url}, alias={alias}, outputs={}]", output_cols_text(outputs))
        }
        SqlSource::RawSql { sql, alias, outputs, written_for } => {
            format!("RawSql[sql={sql}, alias={alias}, outputs={}, writtenFor={written_for}]", output_cols_text(outputs))
        }
        SqlSource::Catalog { grid, schema_pattern, table_pattern, column_pattern, alias, outputs } => format!(
            "Catalog[grid={grid}, schemaPattern={}, tablePattern={}, columnPattern={}, alias={alias}, outputs={}]",
            opt(schema_pattern.as_deref()),
            opt(table_pattern.as_deref()),
            opt(column_pattern.as_deref()),
            output_cols_text(outputs)
        ),
        SqlSource::VarSetPlaceholder { var_name, alias, outputs } => {
            format!("VarSetPlaceholder[varName={var_name}, alias={alias}, outputs={}]", output_cols_text(outputs))
        }
    }
}
