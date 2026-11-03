// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The writer: MirJson's `query`, `source`, `expr`, ..., each object's fields in the order Java puts them. Every
//! field is written, a nullable one absent as JSON null; a long or an int as a JSON integer; a double, a BigDecimal
//! and a window bound's number as the text they were read from.

use crate::mir::{
    Bound, Column, DateFmt, Frame, JavaNumber, Lambda, NameQuoting, OutputCol, Projection, Reducer, SortKey, SqlAgg,
    SqlExpr, SqlQuery, SqlSelect, SqlSource, SqlType, TypeFact,
};
use crate::stmt::{ColumnType, SqlDdl, SqlDml};
use lite_json::{Node, Num, Obj};

fn tagged(tag: &str) -> Obj {
    let mut o = Obj::new();
    o.insert("_type", Node::Str(tag.to_owned()));
    o
}

fn s(v: &str) -> Node {
    Node::Str(v.to_owned())
}

fn s_or_null(v: &Option<String>) -> Node {
    v.as_deref().map_or(Node::Null, s)
}

fn int(v: i64) -> Node {
    Node::Num(Num::from_i64(v))
}

fn strings(vs: &[String]) -> Node {
    Node::Arr(vs.iter().map(|v| s(v)).collect())
}

fn exprs(es: &[SqlExpr]) -> Node {
    Node::Arr(es.iter().map(write_expr).collect())
}

fn expr_or_null(e: Option<&SqlExpr>) -> Node {
    e.map_or(Node::Null, write_expr)
}

/// Java's `query`.
pub fn write_query(q: &SqlQuery) -> Node {
    Node::Obj(match q {
        SqlQuery::Select(s) => select(s),
        SqlQuery::Union(u) => {
            let mut o = tagged("union");
            o.insert("branches", Node::Arr(u.branches.iter().map(write_query).collect()));
            o.insert("all", Node::Bool(u.all));
            o.insert("outputs", output_cols(&u.outputs));
            o
        }
        SqlQuery::With(w) => {
            let mut o = tagged("with");
            let ctes = w
                .ctes
                .iter()
                .map(|c| {
                    let mut co = Obj::new();
                    co.insert("name", s(&c.name));
                    co.insert("query", write_query(&c.query));
                    co.insert("materialized", Node::Bool(c.materialized));
                    Node::Obj(co)
                })
                .collect();
            o.insert("ctes", Node::Arr(ctes));
            o.insert("body", write_query(&w.body));
            o.insert("recursive", Node::Bool(w.recursive));
            o
        }
    })
}

fn select(s: &SqlSelect) -> Obj {
    let mut o = tagged("select");
    o.insert("projections", Node::Arr(s.projections.iter().map(projection).collect()));
    o.insert("distinct", Node::Bool(s.distinct));
    o.insert("from", write_source(&s.from));
    o.insert("where", expr_or_null(s.r#where.as_ref()));
    o.insert("groupBy", exprs(&s.group_by));
    o.insert("having", expr_or_null(s.having.as_ref()));
    o.insert("qualify", expr_or_null(s.qualify.as_ref()));
    o.insert("orderBy", sort_keys(&s.order_by));
    o.insert("limit", s.limit.map_or(Node::Null, int));
    o.insert("offset", s.offset.map_or(Node::Null, int));
    o.insert("outputs", output_cols(&s.outputs));
    o
}

fn projection(p: &Projection) -> Node {
    let mut o = Obj::new();
    o.insert("expr", write_expr(&p.expr));
    o.insert("alias", s_or_null(&p.alias));
    o.insert("out", p.out.as_ref().map_or(Node::Null, output_col));
    Node::Obj(o)
}

fn sort_keys(keys: &[SortKey]) -> Node {
    Node::Arr(
        keys.iter()
            .map(|k| {
                let mut o = Obj::new();
                o.insert("expr", write_expr(&k.expr));
                o.insert("ascending", Node::Bool(k.ascending));
                o.insert("nullOrder", k.null_order.map_or(Node::Null, |n| s(n.as_str())));
                o.insert("outputName", s_or_null(&k.output_name));
                Node::Obj(o)
            })
            .collect(),
    )
}

fn output_cols(cols: &[OutputCol]) -> Node {
    Node::Arr(cols.iter().map(output_col).collect())
}

fn output_col(c: &OutputCol) -> Node {
    let mut o = Obj::new();
    o.insert("name", s(&c.name));
    o.insert("type", write_type(&c.r#type));
    o.insert("nullable", Node::Bool(c.nullable));
    o.insert("origin", s(c.origin.as_str()));
    Node::Obj(o)
}

fn quoting(q: &NameQuoting) -> Node {
    let mut o = Obj::new();
    o.insert("schema", Node::Bool(q.schema));
    o.insert("table", Node::Bool(q.table));
    Node::Obj(o)
}

fn stored_types(types: &[(String, ColumnType)]) -> Node {
    let mut o = Obj::new();
    for (name, t) in types {
        o.insert(name.as_str(), write_column_type(t));
    }
    Node::Obj(o)
}

/// Java's `source`.
pub fn write_source(src: &SqlSource) -> Node {
    let o = match src {
        SqlSource::Table { name, alias, outputs, call, stored_types: stored, quoting: q } => {
            let mut o = tagged("table");
            o.insert("name", s(name));
            o.insert("alias", s(alias));
            o.insert("outputs", output_cols(outputs));
            o.insert("call", Node::Bool(*call));
            o.insert("storedTypes", stored_types(stored));
            o.insert("quoting", quoting(q));
            o
        }
        SqlSource::Subselect { inner, alias, frame_name } => {
            let mut o = tagged("subselect");
            o.insert("inner", write_query(inner));
            o.insert("alias", s(alias));
            o.insert("frameName", s_or_null(frame_name));
            o
        }
        SqlSource::Cte { name, alias, outputs } => {
            let mut o = tagged("cte");
            o.insert("name", s(name));
            o.insert("alias", s(alias));
            o.insert("outputs", output_cols(outputs));
            o
        }
        SqlSource::Join { left, right, kind, on, outputs } => {
            let mut o = tagged("join");
            o.insert("left", write_source(left));
            o.insert("right", write_source(right));
            o.insert("kind", s(kind.as_str()));
            o.insert("on", expr_or_null(on.as_deref()));
            o.insert("outputs", output_cols(outputs));
            o
        }
        SqlSource::Pivot { source, on, r#in, named, usings, alias, outputs } => {
            let mut o = tagged("pivot");
            o.insert("source", write_source(source));
            o.insert("on", exprs(on));
            o.insert("in", exprs(r#in));
            o.insert("named", exprs(named));
            let usings = usings
                .iter()
                .map(|u| {
                    let mut uo = Obj::new();
                    uo.insert("agg", Node::Obj(reducer(&u.agg)));
                    uo.insert("alias", s(&u.alias));
                    uo.insert("type", u.r#type.as_ref().map_or(Node::Null, write_type));
                    Node::Obj(uo)
                })
                .collect();
            o.insert("usings", Node::Arr(usings));
            o.insert("alias", s(alias));
            o.insert("outputs", output_cols(outputs));
            o
        }
        SqlSource::Values { rows, columns, alias, outputs } => {
            let mut o = tagged("values");
            o.insert("rows", Node::Arr(rows.iter().map(|r| exprs(r)).collect()));
            o.insert("columns", strings(columns));
            o.insert("alias", s(alias));
            o.insert("outputs", output_cols(outputs));
            o
        }
        SqlSource::Dual => tagged("dual"),
        SqlSource::SourceUrl { url, alias, outputs } => {
            let mut o = tagged("sourceUrl");
            o.insert("url", s(url));
            o.insert("alias", s(alias));
            o.insert("outputs", output_cols(outputs));
            o
        }
        SqlSource::RawSql { sql, alias, outputs, written_for } => {
            let mut o = tagged("rawSql");
            o.insert("sql", s(sql));
            o.insert("alias", s(alias));
            o.insert("outputs", output_cols(outputs));
            o.insert("writtenFor", s(written_for));
            o
        }
        SqlSource::Catalog { grid, schema_pattern, table_pattern, column_pattern, alias, outputs } => {
            let mut o = tagged("catalog");
            o.insert("grid", s(grid.as_str()));
            o.insert("schemaPattern", s_or_null(schema_pattern));
            o.insert("tablePattern", s_or_null(table_pattern));
            o.insert("columnPattern", s_or_null(column_pattern));
            o.insert("alias", s(alias));
            o.insert("outputs", output_cols(outputs));
            o
        }
        SqlSource::VarSetPlaceholder { var_name, alias, outputs } => {
            let mut o = tagged("varSet");
            o.insert("varName", s(var_name));
            o.insert("alias", s(alias));
            o.insert("outputs", output_cols(outputs));
            o
        }
    };
    Node::Obj(o)
}

/// An expression's tag and its type fact, the first two fields of every expression.
fn expr_obj(tag: &str, r#type: &TypeFact) -> Obj {
    let mut o = tagged(tag);
    o.insert("type", write_fact(r#type));
    o
}

/// Java's `expr`.
pub fn write_expr(e: &SqlExpr) -> Node {
    let mut o = match e {
        SqlExpr::Column(c) => return column(c),
        SqlExpr::Lambda(l) => return lambda(l),
        SqlExpr::Reducer(r) => return Node::Obj(reducer(r)),
        e => expr_obj(expr_tag(e), e.r#type()),
    };
    match e {
        SqlExpr::Column(_) | SqlExpr::Lambda(_) | SqlExpr::Reducer(_) => {}
        SqlExpr::RowOrder { table, .. } | SqlExpr::Star { table, .. } => o.insert("table", s_or_null(table)),
        SqlExpr::StoredRead { column: c, stored, .. } => {
            o.insert("column", column(c));
            o.insert("stored", write_column_type(stored));
        }
        SqlExpr::ReduceCollection { reducer, collection, extras, .. } => {
            o.insert("reducer", s(reducer.as_str()));
            o.insert("collection", write_expr(collection));
            o.insert("extras", exprs(extras));
        }
        SqlExpr::Membership { needle, collection, .. } => {
            o.insert("needle", write_expr(needle));
            o.insert("collection", write_expr(collection));
        }
        SqlExpr::StarExcept { table, except, .. } => {
            o.insert("table", s_or_null(table));
            o.insert("except", strings(except));
        }
        SqlExpr::StringLit { value, .. } => o.insert("value", s(value)),
        SqlExpr::IntLit { value, .. } => o.insert("value", int(*value)),
        SqlExpr::FloatLit { value, .. } => o.insert("value", s(value.text())),
        SqlExpr::DecimalLit { value, .. } => o.insert("value", s(value.as_str())),
        SqlExpr::BoolLit { value, .. } => o.insert("value", Node::Bool(*value)),
        SqlExpr::NullLit { .. } => {}
        SqlExpr::DateLit { iso, .. } | SqlExpr::TimestampLit { iso, .. } | SqlExpr::TimeLit { iso, .. } => {
            o.insert("iso", s(iso))
        }
        SqlExpr::Group { inner, .. } => o.insert("inner", write_expr(inner)),
        SqlExpr::KeyCell { cell, .. } => o.insert("cell", int(i64::from(*cell))),
        SqlExpr::PlanParam { name, kind, optional, enum_map_fn, by_value, .. } => {
            o.insert("name", s(name));
            o.insert("kind", s(kind.as_str()));
            o.insert("optional", Node::Bool(*optional));
            o.insert("enumMapFn", s_or_null(enum_map_fn));
            let by_value = by_value.as_ref().map_or(Node::Null, |v| {
                let mut vo = Obj::new();
                vo.insert("kinds", Node::Arr(v.kinds.iter().map(|k| s(k.as_str())).collect()));
                vo.insert("absent", s(v.absent.as_str()));
                Node::Obj(vo)
            });
            o.insert("byValue", by_value);
        }
        SqlExpr::FormatLit { parts, .. } => o.insert("parts", Node::Arr(parts.iter().map(fmt).collect())),
        SqlExpr::DatePartLit { part, .. } => o.insert("part", s(part.as_str())),
        SqlExpr::JsonKindLit { kind, .. } => o.insert("kind", s(kind.as_str())),
        SqlExpr::TypeNameLit { scalar, .. } => o.insert("scalar", s(scalar.as_str())),
        SqlExpr::OrderedListAgg { value, order_by, .. } => {
            o.insert("value", write_expr(value));
            o.insert("orderBy", write_expr(order_by));
        }
        SqlExpr::ArrayLit { elements, .. } | SqlExpr::JsonArray { elements, .. } => {
            o.insert("elements", exprs(elements))
        }
        SqlExpr::StructLit { fields, .. } => {
            let fields = fields
                .iter()
                .map(|f| {
                    let mut fo = Obj::new();
                    fo.insert("name", s(&f.name));
                    fo.insert("value", write_expr(&f.value));
                    fo.insert("declared", f.declared.as_ref().map_or(Node::Null, write_type));
                    Node::Obj(fo)
                })
                .collect();
            o.insert("fields", Node::Arr(fields));
        }
        SqlExpr::StructGet { source, field, .. } => {
            o.insert("source", write_expr(source));
            o.insert("field", s(field));
        }
        SqlExpr::Call { r#fn, args, .. } => {
            o.insert("fn", s(r#fn.as_str()));
            o.insert("args", exprs(args));
        }
        SqlExpr::Case { whens, otherwise, .. } => {
            let whens = whens
                .iter()
                .map(|w| {
                    let mut wo = Obj::new();
                    wo.insert("condition", write_expr(&w.condition));
                    wo.insert("then", write_expr(&w.then));
                    Node::Obj(wo)
                })
                .collect();
            o.insert("whens", Node::Arr(whens));
            o.insert("otherwise", expr_or_null(otherwise.as_deref()));
        }
        SqlExpr::Exists { subquery, .. } | SqlExpr::ScalarSubquery { subquery, .. } => {
            o.insert("subquery", write_query(subquery))
        }
        SqlExpr::InSubquery { value, subquery, .. } => {
            o.insert("value", write_expr(value));
            o.insert("subquery", write_query(subquery));
        }
        SqlExpr::CheckedDefects { own, hoists, .. } => {
            o.insert("own", write_expr(own));
            let hoists = hoists
                .iter()
                .map(|h| {
                    let mut ho = Obj::new();
                    ho.insert("path", strings(&h.path));
                    ho.insert("envelope", write_expr(&h.envelope));
                    ho.insert("toMany", Node::Bool(h.to_many));
                    Node::Obj(ho)
                })
                .collect();
            o.insert("hoists", Node::Arr(hoists));
        }
        SqlExpr::CheckedChildValue { envelope, to_many, .. } => {
            o.insert("envelope", write_expr(envelope));
            o.insert("toMany", Node::Bool(*to_many));
        }
        SqlExpr::Quantified { value, comparison, quantifier, subquery, .. } => {
            o.insert("value", write_expr(value));
            o.insert("comparison", s(comparison.as_str()));
            o.insert("quantifier", s(quantifier.as_str()));
            o.insert("subquery", write_query(subquery));
        }
        SqlExpr::CheckedOne { list, scalar_carrier, at_least_only, .. } => {
            o.insert("list", write_expr(list));
            o.insert("scalarCarrier", Node::Bool(*scalar_carrier));
            o.insert("atLeastOnly", Node::Bool(*at_least_only));
        }
        SqlExpr::CompactList { list, .. } => o.insert("list", write_expr(list)),
        SqlExpr::DeferredTdsString { inner, alias, id, form, render_tds_null, .. } => {
            o.insert("inner", Node::Obj(select(inner)));
            o.insert("alias", s(alias));
            o.insert("id", int(i64::from(*id)));
            o.insert("form", s(form.as_str()));
            o.insert("renderTdsNull", Node::Bool(*render_tds_null));
        }
        SqlExpr::TempTableInSplice { temp_table_name, .. } => o.insert("tempTableName", s(temp_table_name)),
        SqlExpr::JsonObject { kv, .. } => o.insert("kv", exprs(kv)),
        SqlExpr::JsonArrayAgg { value, order_keys, .. } => {
            o.insert("value", write_expr(value));
            let keys = order_keys
                .iter()
                .map(|k| {
                    let mut ko = Obj::new();
                    ko.insert("expr", write_expr(&k.expr));
                    ko.insert("desc", Node::Bool(k.desc));
                    Node::Obj(ko)
                })
                .collect();
            o.insert("orderKeys", Node::Arr(keys));
        }
        SqlExpr::WindowCall { r#fn, partition_by, order_by, frame: f, .. } => {
            o.insert("fn", write_agg(r#fn));
            o.insert("partitionBy", exprs(partition_by));
            o.insert("orderBy", sort_keys(order_by));
            o.insert("frame", f.as_ref().map_or(Node::Null, frame));
        }
        SqlExpr::Cast { value, target, conform, .. } => {
            o.insert("value", write_expr(value));
            o.insert("target", write_type(target));
            o.insert("conform", Node::Bool(*conform));
        }
        SqlExpr::FoldCall { source, lambda: l, init, acc_is_list, homogeneous, .. } => {
            o.insert("source", write_expr(source));
            o.insert("lambda", lambda(l));
            o.insert("init", write_expr(init));
            o.insert("accIsList", Node::Bool(*acc_is_list));
            o.insert("homogeneous", Node::Bool(*homogeneous));
        }
    }
    Node::Obj(o)
}

/// The expression's MirJson tag (`ExprTag`).
pub fn expr_tag(e: &SqlExpr) -> &'static str {
    match e {
        SqlExpr::RowOrder { .. } => "rowOrder",
        SqlExpr::StoredRead { .. } => "storedRead",
        SqlExpr::ReduceCollection { .. } => "reduceCollection",
        SqlExpr::Membership { .. } => "membership",
        SqlExpr::Column(_) => "column",
        SqlExpr::StarExcept { .. } => "starExcept",
        SqlExpr::Star { .. } => "star",
        SqlExpr::StringLit { .. } => "stringLit",
        SqlExpr::IntLit { .. } => "intLit",
        SqlExpr::FloatLit { .. } => "floatLit",
        SqlExpr::DecimalLit { .. } => "decimalLit",
        SqlExpr::BoolLit { .. } => "boolLit",
        SqlExpr::NullLit { .. } => "nullLit",
        SqlExpr::DateLit { .. } => "dateLit",
        SqlExpr::Group { .. } => "group",
        SqlExpr::KeyCell { .. } => "keyCell",
        SqlExpr::PlanParam { .. } => "planParam",
        SqlExpr::FormatLit { .. } => "formatLit",
        SqlExpr::DatePartLit { .. } => "datePartLit",
        SqlExpr::JsonKindLit { .. } => "jsonKindLit",
        SqlExpr::TypeNameLit { .. } => "typeNameLit",
        SqlExpr::TimestampLit { .. } => "timestampLit",
        SqlExpr::TimeLit { .. } => "timeLit",
        SqlExpr::OrderedListAgg { .. } => "orderedListAgg",
        SqlExpr::ArrayLit { .. } => "arrayLit",
        SqlExpr::StructLit { .. } => "structLit",
        SqlExpr::StructGet { .. } => "structGet",
        SqlExpr::Call { .. } => "call",
        SqlExpr::Case { .. } => "caseExpr",
        SqlExpr::Exists { .. } => "exists",
        SqlExpr::InSubquery { .. } => "inSubquery",
        SqlExpr::CheckedDefects { .. } => "checkedDefects",
        SqlExpr::CheckedChildValue { .. } => "checkedChildValue",
        SqlExpr::Quantified { .. } => "quantified",
        SqlExpr::ScalarSubquery { .. } => "scalarSubquery",
        SqlExpr::CheckedOne { .. } => "checkedOne",
        SqlExpr::CompactList { .. } => "compactList",
        SqlExpr::DeferredTdsString { .. } => "deferredTdsString",
        SqlExpr::TempTableInSplice { .. } => "tempTableInSplice",
        SqlExpr::JsonObject { .. } => "jsonObject",
        SqlExpr::JsonArray { .. } => "jsonArray",
        SqlExpr::JsonArrayAgg { .. } => "jsonArrayAgg",
        SqlExpr::WindowCall { .. } => "windowCall",
        SqlExpr::Lambda(_) => "lambda",
        SqlExpr::Cast { .. } => "cast",
        SqlExpr::FoldCall { .. } => "foldCall",
        SqlExpr::Reducer(_) => "reducer",
    }
}

fn lambda(l: &Lambda) -> Node {
    let mut o = expr_obj("lambda", &l.r#type);
    o.insert("params", strings(&l.params));
    o.insert("body", write_expr(&l.body));
    Node::Obj(o)
}

fn column(c: &Column) -> Node {
    let mut o = expr_obj("column", &c.r#type);
    o.insert("table", s_or_null(&c.table));
    o.insert("name", s(&c.name));
    o.insert("origin", c.origin.map_or(Node::Null, |n| s(n.as_str())));
    Node::Obj(o)
}

/// A reducer, as an expression and as an aggregate alike: `_type`, `type`, then its fields.
fn reducer(r: &Reducer) -> Obj {
    let mut o = expr_obj("reducer", &r.r#type);
    o.insert("fn", s(r.r#fn.as_str()));
    o.insert("args", exprs(&r.args));
    o.insert("distinct", Node::Bool(r.distinct));
    o.insert("orderBy", sort_keys(&r.order_by));
    o
}

fn frame(f: &Frame) -> Node {
    let mut o = Obj::new();
    o.insert("kind", s(f.kind.as_str()));
    o.insert("from", bound(&f.from));
    o.insert("to", bound(&f.to));
    Node::Obj(o)
}

/// Java's `agg`.
pub fn write_agg(a: &SqlAgg) -> Node {
    Node::Obj(match a {
        SqlAgg::Reducer(r) => reducer(r),
        SqlAgg::RankingFn { r#fn, args } | SqlAgg::ValueFn { r#fn, args } => {
            let mut o = tagged(if matches!(a, SqlAgg::RankingFn { .. }) { "rankingFn" } else { "valueFn" });
            o.insert("fn", s(r#fn.as_str()));
            o.insert("args", exprs(args));
            o
        }
    })
}

fn bound(b: &Bound) -> Node {
    Node::Obj(match b {
        Bound::UnboundedPreceding => tagged("unboundedPreceding"),
        Bound::Preceding { n } => number(tagged("preceding"), n),
        Bound::CurrentRow => tagged("currentRow"),
        Bound::Following { n } => number(tagged("following"), n),
        Bound::UnboundedFollowing => tagged("unboundedFollowing"),
        Bound::IntervalPreceding { n, unit } | Bound::IntervalFollowing { n, unit } => {
            let tag =
                if matches!(b, Bound::IntervalPreceding { .. }) { "intervalPreceding" } else { "intervalFollowing" };
            let mut o = tagged(tag);
            o.insert("n", int(*n));
            o.insert("unit", s(unit));
            o
        }
    })
}

/// Java's `number`: a bound's number as its text and its class.
fn number(mut o: Obj, n: &JavaNumber) -> Obj {
    let (text, class) = match n {
        JavaNumber::Long(v) => (v.to_string(), "Long"),
        JavaNumber::Integer(v) => (v.to_string(), "Integer"),
        JavaNumber::Double(d) => (d.text().to_owned(), "Double"),
        JavaNumber::BigDecimal(d) => (d.as_str().to_owned(), "BigDecimal"),
    };
    o.insert("n", Node::Str(text));
    o.insert("numberClass", s(class));
    o
}

/// Java's `type`.
pub fn write_type(t: &SqlType) -> Node {
    Node::Obj(match t {
        SqlType::Scalar(scalar) => {
            let mut o = tagged("scalar");
            o.insert("name", s(scalar.as_str()));
            o
        }
        SqlType::Decimal { precision, scale } => {
            let mut o = tagged("decimal");
            o.insert("precision", int(i64::from(*precision)));
            o.insert("scale", int(i64::from(*scale)));
            o
        }
        SqlType::Array { element } => {
            let mut o = tagged("array");
            o.insert("element", write_type(element));
            o
        }
        SqlType::Map { key, value } => {
            let mut o = tagged("map");
            o.insert("key", write_type(key));
            o.insert("value", write_type(value));
            o
        }
        SqlType::Struct { fields } => {
            let mut o = tagged("struct");
            let fields = fields
                .iter()
                .map(|f| {
                    let mut fo = Obj::new();
                    fo.insert("name", s(&f.name));
                    fo.insert("type", write_type(&f.r#type));
                    Node::Obj(fo)
                })
                .collect();
            o.insert("fields", Node::Arr(fields));
            o
        }
        SqlType::Tagged { slots } => {
            let mut o = tagged("tagged");
            let slots = slots
                .iter()
                .map(|sl| {
                    let mut so = Obj::new();
                    so.insert("kind", s(&sl.kind));
                    so.insert("type", write_type(&sl.r#type));
                    Node::Obj(so)
                })
                .collect();
            o.insert("slots", Node::Arr(slots));
            o
        }
    })
}

/// Java's `fact`.
pub fn write_fact(f: &TypeFact) -> Node {
    Node::Obj(match f {
        TypeFact::Typed { r#type, nullable } => {
            let mut o = tagged("typed");
            o.insert("sqlType", write_type(r#type));
            o.insert("nullable", Node::Bool(*nullable));
            o
        }
        TypeFact::Bottom => tagged("bottom"),
        TypeFact::Raises => tagged("raises"),
        TypeFact::Unknown => tagged("unknown"),
    })
}

fn fmt(f: &DateFmt) -> Node {
    Node::Obj(match f {
        DateFmt::Part(p) => {
            let mut o = tagged("part");
            o.insert("part", s(p.as_str()));
            o
        }
        DateFmt::Text { s: text } => {
            let mut o = tagged("text");
            o.insert("s", s(text));
            o
        }
    })
}

/// Java's `columnType`.
pub fn write_column_type(t: &ColumnType) -> Node {
    Node::Obj(match t {
        ColumnType::Plain { kind } => {
            let mut o = tagged("plain");
            o.insert("kind", s(kind.as_str()));
            o
        }
        ColumnType::Sized { kind, size } => {
            let mut o = tagged("sized");
            o.insert("kind", s(kind));
            o.insert("size", int(i64::from(*size)));
            o
        }
        ColumnType::Scaled { kind, precision, scale } => {
            let mut o = tagged("scaled");
            o.insert("kind", s(kind));
            o.insert("precision", int(i64::from(*precision)));
            o.insert("scale", int(i64::from(*scale)));
            o
        }
    })
}

/// Java's `ddl`.
pub fn write_ddl(d: &SqlDdl) -> Node {
    Node::Obj(match d {
        SqlDdl::CreateTable { schema, table, quoting: q, columns, temporary } => {
            let mut o = tagged("createTable");
            o.insert("schema", s_or_null(schema));
            o.insert("table", s(table));
            o.insert("quoting", quoting(q));
            let columns = columns
                .iter()
                .map(|c| {
                    let mut co = Obj::new();
                    co.insert("name", s(&c.name));
                    co.insert("declaredQuoted", Node::Bool(c.declared_quoted));
                    co.insert("type", write_column_type(&c.r#type));
                    co.insert("notNull", Node::Bool(c.not_null));
                    co.insert("primaryKey", Node::Bool(c.primary_key));
                    Node::Obj(co)
                })
                .collect();
            o.insert("columns", Node::Arr(columns));
            o.insert("temporary", Node::Bool(*temporary));
            o
        }
        SqlDdl::DropTable { schema, table, quoting: q } => {
            let mut o = tagged("dropTable");
            o.insert("schema", s_or_null(schema));
            o.insert("table", s(table));
            o.insert("quoting", quoting(q));
            o
        }
        SqlDdl::CreateSchema { schema, quoted } | SqlDdl::DropSchema { schema, quoted } => {
            let mut o = tagged(if matches!(d, SqlDdl::CreateSchema { .. }) { "createSchema" } else { "dropSchema" });
            o.insert("schema", s(schema));
            o.insert("quoted", Node::Bool(*quoted));
            o
        }
    })
}

/// Java's `target`: a DML statement's tag and the table it writes.
fn target(tag: &str, schema: &Option<String>, table: &str, q: &NameQuoting) -> Obj {
    let mut o = tagged(tag);
    o.insert("schema", s_or_null(schema));
    o.insert("table", s(table));
    o.insert("quoting", quoting(q));
    o
}

/// Java's `dml`.
pub fn write_dml(d: &SqlDml) -> Node {
    Node::Obj(match d {
        SqlDml::InsertValues { schema, table, quoting, columns, quoted_columns, rows } => {
            let mut o = target("insertValues", schema, table, quoting);
            o.insert("columns", strings(columns));
            o.insert("quotedColumns", strings(quoted_columns));
            o.insert("rows", Node::Arr(rows.iter().map(|r| exprs(r)).collect()));
            o
        }
        SqlDml::InsertFromTable { schema, table, quoting, columns, quoted_columns, source } => {
            let mut o = target("insertFromTable", schema, table, quoting);
            o.insert("columns", strings(columns));
            o.insert("quotedColumns", strings(quoted_columns));
            o.insert("source", s(source));
            o
        }
        SqlDml::DeleteAll { schema, table, quoting } => target("deleteAll", schema, table, quoting),
        SqlDml::CreateTempTableAs { table, query, columns, staged } => {
            let mut o = tagged("createTempTableAs");
            o.insert("table", s(table));
            o.insert("query", write_query(query));
            o.insert("columns", strings(columns));
            o.insert("staged", Node::Bool(*staged));
            o
        }
        SqlDml::InsertSelect { schema, table, quoting, columns, quoted_columns, query } => {
            let mut o = target("insertSelect", schema, table, quoting);
            o.insert("columns", strings(columns));
            o.insert("quotedColumns", strings(quoted_columns));
            o.insert("query", write_query(query));
            o
        }
        SqlDml::Update { schema, table, quoting, alias, stored_types: stored, assignments, r#where } => {
            let mut o = target("update", schema, table, quoting);
            o.insert("alias", s(alias));
            o.insert("storedTypes", stored_types(stored));
            let assignments = assignments
                .iter()
                .map(|a| {
                    let mut m = Obj::new();
                    m.insert("column", s(&a.column));
                    m.insert("quoted", Node::Bool(a.quoted));
                    m.insert("value", write_expr(&a.value));
                    Node::Obj(m)
                })
                .collect();
            o.insert("assignments", Node::Arr(assignments));
            o.insert("where", expr_or_null(r#where.as_ref()));
            o
        }
        SqlDml::DeleteWhere { schema, table, quoting, alias, stored_types: stored, r#where } => {
            let mut o = target("deleteWhere", schema, table, quoting);
            o.insert("alias", s(alias));
            o.insert("storedTypes", stored_types(stored));
            o.insert("where", expr_or_null(r#where.as_ref()));
            o
        }
        SqlDml::Merge { schema, table, quoting, columns, quoted_columns, keys, query, alias, source_alias } => {
            let mut o = target("merge", schema, table, quoting);
            o.insert("columns", strings(columns));
            o.insert("quotedColumns", strings(quoted_columns));
            o.insert("keys", strings(keys));
            o.insert("query", write_query(query));
            o.insert("alias", s(alias));
            o.insert("sourceAlias", s(source_alias));
            o
        }
    })
}
