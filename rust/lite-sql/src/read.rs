// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The reader: MirJson's `read*`, from the JSON Java's `MirJson` writes. A nullable field may be missing or null; an
//! unknown tag or constant is refused by name, worded as Java's (`unknown expression tag 'x'`).

use crate::mir::{
    BigDecimal, Bound, Column, Cte, DateFmt, Double, Frame, Hoist, JavaNumber, JsonArrayAggKey, Lambda, NameQuoting,
    OutputCol, PivotUsing, Projection, Reducer, SortKey, SqlAgg, SqlExpr, SqlQuery, SqlSelect, SqlSource, SqlType,
    SqlUnion, SqlWith, StructField, StructLitField, TaggedSlot, TypeFact, ValueTyping, When,
};
use crate::names::{
    AggFn, CatalogGrid, ColumnKind, DateFmtPart, DatePart, FrameKind, JoinKind, JsonKind, NullOrder, Origin, ParamKind,
    Quantifier, Scalar, SqlFn, TdsForm, ValueKind,
};
use crate::stmt::{Assignment, ColumnType, DdlColumn, SqlDdl, SqlDml};
use lite_json::{Error, Node, Obj};

/// Every expression tag MirJson writes, in its `ExprTag` order.
pub const EXPR_TAGS: [&str; 47] = [
    "rowOrder",
    "storedRead",
    "reduceCollection",
    "membership",
    "column",
    "starExcept",
    "star",
    "stringLit",
    "intLit",
    "floatLit",
    "decimalLit",
    "boolLit",
    "nullLit",
    "dateLit",
    "group",
    "planParam",
    "formatLit",
    "datePartLit",
    "jsonKindLit",
    "typeNameLit",
    "timestampLit",
    "timeLit",
    "orderedListAgg",
    "arrayLit",
    "structLit",
    "structGet",
    "call",
    "caseExpr",
    "exists",
    "inSubquery",
    "checkedDefects",
    "checkedChildValue",
    "quantified",
    "scalarSubquery",
    "checkedOne",
    "compactList",
    "deferredTdsString",
    "tempTableInSplice",
    "jsonObject",
    "jsonArray",
    "jsonArrayAgg",
    "windowCall",
    "lambda",
    "cast",
    "foldCall",
    "reducer",
    "keyCell",
];

/// Java's `named`: the constant `name` names, or `unknown <what> '<name>'`.
fn named<T>(from_name: fn(&str) -> Option<T>, name: &str, what: &str) -> Result<T, Error> {
    from_name(name).ok_or_else(|| unknown(what, name))
}

fn unknown(what: &str, name: &str) -> Error {
    Error(format!("unknown {what} '{name}'"))
}

/// Java's `tagOf`: the object's `_type`.
fn tag(o: &Obj) -> Result<&str, Error> {
    o.str("_type")
}

fn unknown_tag(what: &str, name: &str) -> Error {
    unknown(&format!("{what} tag"), name)
}

fn string(o: &Obj, key: &str) -> Result<String, Error> {
    o.str(key).map(str::to_owned)
}

fn string_or_null(o: &Obj, key: &str) -> Result<Option<String>, Error> {
    Ok(o.str_or_null(key)?.map(str::to_owned))
}

/// Java's `getInt`: an integer field, as an int.
fn int(o: &Obj, key: &str) -> Result<i32, Error> {
    let v = o.i64(key)?;
    i32::try_from(v).map_err(|_| Error(format!("field '{key}' is not an int: {v}")))
}

/// A long field that may be missing or null.
fn long_or_null(o: &Obj, key: &str) -> Result<Option<i64>, Error> {
    match o.get_or_null(key) {
        None => Ok(None),
        Some(_) => o.i64(key).map(Some),
    }
}

fn objs(a: &[Node]) -> impl Iterator<Item = Result<&Obj, Error>> {
    a.iter().map(Node::as_obj)
}

fn strings(a: &[Node]) -> Result<Vec<String>, Error> {
    a.iter().map(|n| n.as_str().map(str::to_owned)).collect()
}

fn exprs(a: &[Node]) -> Result<Vec<SqlExpr>, Error> {
    objs(a).map(|o| read_expr(o?)).collect()
}

fn boxed(o: &Obj, key: &str) -> Result<Box<SqlExpr>, Error> {
    read_expr(o.obj(key)?).map(Box::new)
}

fn expr_or_null(o: &Obj, key: &str) -> Result<Option<SqlExpr>, Error> {
    o.obj_or_null(key)?.map(read_expr).transpose()
}

fn subquery(o: &Obj, key: &str) -> Result<Box<SqlQuery>, Error> {
    read_query(o.obj(key)?).map(Box::new)
}

/// Java's `readQuery`.
pub fn read_query(o: &Obj) -> Result<SqlQuery, Error> {
    match tag(o)? {
        "select" => read_select(o).map(|s| SqlQuery::Select(Box::new(s))),
        "union" => Ok(SqlQuery::Union(SqlUnion {
            branches: objs(o.arr("branches")?).map(|b| read_query(b?)).collect::<Result<_, _>>()?,
            all: o.bool("all")?,
            outputs: output_cols(o.arr("outputs")?)?,
        })),
        "with" => {
            let ctes = objs(o.arr("ctes")?)
                .map(|c| {
                    let c = c?;
                    Ok(Cte {
                        name: string(c, "name")?,
                        query: read_query(c.obj("query")?)?,
                        materialized: c.bool("materialized")?,
                    })
                })
                .collect::<Result<_, Error>>()?;
            Ok(SqlQuery::With(SqlWith { ctes, body: subquery(o, "body")?, recursive: o.bool("recursive")? }))
        }
        other => Err(unknown_tag("query", other)),
    }
}

fn read_select(o: &Obj) -> Result<SqlSelect, Error> {
    Ok(SqlSelect {
        projections: objs(o.arr("projections")?).map(|p| projection(p?)).collect::<Result<_, _>>()?,
        distinct: o.bool("distinct")?,
        from: read_source(o.obj("from")?)?,
        r#where: expr_or_null(o, "where")?,
        group_by: exprs(o.arr("groupBy")?)?,
        having: expr_or_null(o, "having")?,
        qualify: expr_or_null(o, "qualify")?,
        order_by: sort_keys(o.arr("orderBy")?)?,
        limit: long_or_null(o, "limit")?,
        offset: long_or_null(o, "offset")?,
        outputs: output_cols(o.arr("outputs")?)?,
    })
}

fn projection(o: &Obj) -> Result<Projection, Error> {
    Ok(Projection {
        expr: read_expr(o.obj("expr")?)?,
        alias: string_or_null(o, "alias")?,
        out: o.obj_or_null("out")?.map(output_col).transpose()?,
    })
}

fn sort_keys(a: &[Node]) -> Result<Vec<SortKey>, Error> {
    objs(a)
        .map(|k| {
            let k = k?;
            Ok(SortKey {
                expr: read_expr(k.obj("expr")?)?,
                ascending: k.bool("ascending")?,
                null_order: k
                    .str_or_null("nullOrder")?
                    .map(|n| named(NullOrder::from_name, n, "null order"))
                    .transpose()?,
                output_name: string_or_null(k, "outputName")?,
            })
        })
        .collect()
}

fn output_cols(a: &[Node]) -> Result<Vec<OutputCol>, Error> {
    objs(a).map(|c| output_col(c?)).collect()
}

fn output_col(o: &Obj) -> Result<OutputCol, Error> {
    Ok(OutputCol {
        name: string(o, "name")?,
        r#type: read_type(o.obj("type")?)?,
        nullable: o.bool("nullable")?,
        origin: named(Origin::from_name, o.str("origin")?, "output origin")?,
    })
}

fn origin_or_null(o: &Obj) -> Result<Option<Origin>, Error> {
    o.str_or_null("origin")?.map(|n| named(Origin::from_name, n, "output origin")).transpose()
}

fn quoting(q: &Obj) -> Result<NameQuoting, Error> {
    Ok(NameQuoting { schema: q.bool("schema")?, table: q.bool("table")? })
}

fn stored_types(o: &Obj) -> Result<Vec<(String, ColumnType)>, Error> {
    o.iter().map(|(k, v)| Ok((k.to_owned(), read_column_type(v.as_obj()?)?))).collect()
}

/// Java's `readSource`.
pub fn read_source(o: &Obj) -> Result<SqlSource, Error> {
    let alias = || string(o, "alias");
    let outputs = || output_cols(o.arr("outputs")?);
    Ok(match tag(o)? {
        "table" => SqlSource::Table {
            name: string(o, "name")?,
            alias: alias()?,
            outputs: outputs()?,
            call: o.bool("call")?,
            stored_types: stored_types(o.obj("storedTypes")?)?,
            quoting: quoting(o.obj("quoting")?)?,
        },
        "subselect" => SqlSource::Subselect {
            inner: subquery(o, "inner")?,
            alias: alias()?,
            frame_name: string_or_null(o, "frameName")?,
        },
        "cte" => SqlSource::Cte { name: string(o, "name")?, alias: alias()?, outputs: outputs()? },
        "join" => SqlSource::Join {
            left: Box::new(read_source(o.obj("left")?)?),
            right: Box::new(read_source(o.obj("right")?)?),
            kind: named(JoinKind::from_name, o.str("kind")?, "join kind")?,
            on: expr_or_null(o, "on")?.map(Box::new),
            outputs: outputs()?,
        },
        "pivot" => SqlSource::Pivot {
            source: Box::new(read_source(o.obj("source")?)?),
            on: exprs(o.arr("on")?)?,
            r#in: exprs(o.arr("in")?)?,
            named: exprs(o.arr("named")?)?,
            usings: objs(o.arr("usings")?).map(|u| pivot_using(u?)).collect::<Result<_, _>>()?,
            alias: alias()?,
            outputs: outputs()?,
        },
        "values" => SqlSource::Values {
            rows: o.arr("rows")?.iter().map(|r| exprs(r.as_arr()?)).collect::<Result<_, _>>()?,
            columns: strings(o.arr("columns")?)?,
            alias: alias()?,
            outputs: outputs()?,
        },
        "dual" => SqlSource::Dual,
        "sourceUrl" => SqlSource::SourceUrl { url: string(o, "url")?, alias: alias()?, outputs: outputs()? },
        "rawSql" => SqlSource::RawSql {
            sql: string(o, "sql")?,
            alias: alias()?,
            outputs: outputs()?,
            written_for: string(o, "writtenFor")?,
        },
        "catalog" => SqlSource::Catalog {
            grid: named(CatalogGrid::from_name, o.str("grid")?, "grid")?,
            schema_pattern: string_or_null(o, "schemaPattern")?,
            table_pattern: string_or_null(o, "tablePattern")?,
            column_pattern: string_or_null(o, "columnPattern")?,
            alias: alias()?,
            outputs: outputs()?,
        },
        "varSet" => {
            SqlSource::VarSetPlaceholder { var_name: string(o, "varName")?, alias: alias()?, outputs: outputs()? }
        }
        other => return Err(unknown_tag("source", other)),
    })
}

fn pivot_using(u: &Obj) -> Result<PivotUsing, Error> {
    let agg = match read_agg(u.obj("agg")?)? {
        SqlAgg::Reducer(r) => r,
        _ => return Err(Error("a pivot's aggregate is not a reducer".to_owned())),
    };
    Ok(PivotUsing { agg, alias: string(u, "alias")?, r#type: u.obj_or_null("type")?.map(read_type).transpose()? })
}

/// Java's `readExpr`.
pub fn read_expr(o: &Obj) -> Result<SqlExpr, Error> {
    let t = tag(o)?;
    if !EXPR_TAGS.contains(&t) {
        return Err(unknown_tag("expression", t));
    }
    let r#type = read_fact(o.obj("type")?)?;
    Ok(match t {
        "rowOrder" => SqlExpr::RowOrder { table: string_or_null(o, "table")?, r#type },
        "storedRead" => SqlExpr::StoredRead {
            column: match read_expr(o.obj("column")?)? {
                SqlExpr::Column(c) => c,
                _ => return Err(Error("a stored read's column is not a column".to_owned())),
            },
            stored: read_column_type(o.obj("stored")?)?,
            r#type,
        },
        "reduceCollection" => SqlExpr::ReduceCollection {
            reducer: named(AggFn::from_name, o.str("reducer")?, "aggregate")?,
            collection: boxed(o, "collection")?,
            extras: exprs(o.arr("extras")?)?,
            r#type,
        },
        "membership" => {
            SqlExpr::Membership { needle: boxed(o, "needle")?, collection: boxed(o, "collection")?, r#type }
        }
        "column" => SqlExpr::Column(Column {
            table: string_or_null(o, "table")?,
            name: string(o, "name")?,
            r#type,
            origin: origin_or_null(o)?,
        }),
        "starExcept" => {
            SqlExpr::StarExcept { table: string_or_null(o, "table")?, except: strings(o.arr("except")?)?, r#type }
        }
        "star" => SqlExpr::Star { table: string_or_null(o, "table")?, r#type },
        "stringLit" => SqlExpr::StringLit { value: string(o, "value")?, r#type },
        "intLit" => SqlExpr::IntLit { value: o.i64("value")?, r#type },
        "floatLit" => SqlExpr::FloatLit { value: Double::parse(o.str("value")?)?, r#type },
        "decimalLit" => SqlExpr::DecimalLit { value: BigDecimal::parse(o.str("value")?)?, r#type },
        "boolLit" => SqlExpr::BoolLit { value: o.bool("value")?, r#type },
        "nullLit" => SqlExpr::NullLit { r#type },
        "dateLit" => SqlExpr::DateLit { iso: string(o, "iso")?, r#type },
        "group" => SqlExpr::Group { inner: boxed(o, "inner")?, r#type },
        "keyCell" => SqlExpr::KeyCell { cell: int(o, "cell")?, r#type },
        "planParam" => plan_param(o, r#type)?,
        "formatLit" => {
            SqlExpr::FormatLit { parts: objs(o.arr("parts")?).map(|p| fmt(p?)).collect::<Result<_, _>>()?, r#type }
        }
        "datePartLit" => {
            SqlExpr::DatePartLit { part: named(DatePart::from_name, o.str("part")?, "date part")?, r#type }
        }
        "jsonKindLit" => {
            SqlExpr::JsonKindLit { kind: named(JsonKind::from_name, o.str("kind")?, "JSON kind")?, r#type }
        }
        "typeNameLit" => {
            SqlExpr::TypeNameLit { scalar: named(Scalar::from_name, o.str("scalar")?, "scalar type")?, r#type }
        }
        "timestampLit" => SqlExpr::TimestampLit { iso: string(o, "iso")?, r#type },
        "timeLit" => SqlExpr::TimeLit { iso: string(o, "iso")?, r#type },
        "orderedListAgg" => {
            SqlExpr::OrderedListAgg { value: boxed(o, "value")?, order_by: boxed(o, "orderBy")?, r#type }
        }
        "arrayLit" => SqlExpr::ArrayLit { elements: exprs(o.arr("elements")?)?, r#type },
        "structLit" => SqlExpr::StructLit { fields: struct_lit_fields(o.arr("fields")?)?, r#type },
        "structGet" => SqlExpr::StructGet { source: boxed(o, "source")?, field: string(o, "field")?, r#type },
        "call" => SqlExpr::Call {
            r#fn: named(SqlFn::from_name, o.str("fn")?, "function")?,
            args: exprs(o.arr("args")?)?,
            r#type,
        },
        "caseExpr" => SqlExpr::Case {
            whens: objs(o.arr("whens")?)
                .map(|w| {
                    let w = w?;
                    Ok(When { condition: read_expr(w.obj("condition")?)?, then: read_expr(w.obj("then")?)? })
                })
                .collect::<Result<_, Error>>()?,
            otherwise: expr_or_null(o, "otherwise")?.map(Box::new),
            r#type,
        },
        "exists" => SqlExpr::Exists { subquery: subquery(o, "subquery")?, r#type },
        "inSubquery" => SqlExpr::InSubquery { value: boxed(o, "value")?, subquery: subquery(o, "subquery")?, r#type },
        "checkedDefects" => SqlExpr::CheckedDefects {
            own: boxed(o, "own")?,
            hoists: objs(o.arr("hoists")?)
                .map(|h| {
                    let h = h?;
                    Ok(Hoist {
                        path: strings(h.arr("path")?)?,
                        envelope: read_expr(h.obj("envelope")?)?,
                        to_many: h.bool("toMany")?,
                    })
                })
                .collect::<Result<_, Error>>()?,
            r#type,
        },
        "checkedChildValue" => {
            SqlExpr::CheckedChildValue { envelope: boxed(o, "envelope")?, to_many: o.bool("toMany")?, r#type }
        }
        "quantified" => SqlExpr::Quantified {
            value: boxed(o, "value")?,
            comparison: named(SqlFn::from_name, o.str("comparison")?, "function")?,
            quantifier: named(Quantifier::from_name, o.str("quantifier")?, "quantifier")?,
            subquery: subquery(o, "subquery")?,
            r#type,
        },
        "scalarSubquery" => SqlExpr::ScalarSubquery { subquery: subquery(o, "subquery")?, r#type },
        "checkedOne" => SqlExpr::CheckedOne {
            list: boxed(o, "list")?,
            scalar_carrier: o.bool("scalarCarrier")?,
            at_least_only: o.bool("atLeastOnly")?,
            r#type,
        },
        "compactList" => SqlExpr::CompactList { list: boxed(o, "list")?, r#type },
        "deferredTdsString" => SqlExpr::DeferredTdsString {
            inner: match read_query(o.obj("inner")?)? {
                SqlQuery::Select(s) => s,
                _ => return Err(Error("a deferred TDS string's query is not a select".to_owned())),
            },
            alias: string(o, "alias")?,
            id: int(o, "id")?,
            form: named(TdsForm::from_name, o.str("form")?, "TDS form")?,
            render_tds_null: o.bool("renderTdsNull")?,
            r#type,
        },
        "tempTableInSplice" => SqlExpr::TempTableInSplice { temp_table_name: string(o, "tempTableName")?, r#type },
        "jsonObject" => SqlExpr::JsonObject { kv: exprs(o.arr("kv")?)?, r#type },
        "jsonArray" => SqlExpr::JsonArray { elements: exprs(o.arr("elements")?)?, r#type },
        "jsonArrayAgg" => SqlExpr::JsonArrayAgg {
            value: boxed(o, "value")?,
            order_keys: objs(o.arr("orderKeys")?)
                .map(|k| {
                    let k = k?;
                    Ok(JsonArrayAggKey { expr: read_expr(k.obj("expr")?)?, desc: k.bool("desc")? })
                })
                .collect::<Result<_, Error>>()?,
            r#type,
        },
        "windowCall" => SqlExpr::WindowCall {
            r#fn: Box::new(read_agg(o.obj("fn")?)?),
            partition_by: exprs(o.arr("partitionBy")?)?,
            order_by: sort_keys(o.arr("orderBy")?)?,
            frame: o.obj_or_null("frame")?.map(frame).transpose()?,
            r#type,
        },
        "lambda" => SqlExpr::Lambda(Lambda { params: strings(o.arr("params")?)?, body: boxed(o, "body")?, r#type }),
        "cast" => SqlExpr::Cast {
            value: boxed(o, "value")?,
            target: read_type(o.obj("target")?)?,
            conform: o.bool("conform")?,
            r#type,
        },
        "foldCall" => SqlExpr::FoldCall {
            source: boxed(o, "source")?,
            lambda: match read_expr(o.obj("lambda")?)? {
                SqlExpr::Lambda(l) => l,
                _ => return Err(Error("a fold's lambda is not a lambda".to_owned())),
            },
            init: boxed(o, "init")?,
            acc_is_list: o.bool("accIsList")?,
            homogeneous: o.bool("homogeneous")?,
            r#type,
        },
        "reducer" => SqlExpr::Reducer(reducer(o, r#type)?),
        other => return Err(unknown_tag("expression", other)),
    })
}

fn plan_param(o: &Obj, r#type: TypeFact) -> Result<SqlExpr, Error> {
    let by_value = match o.obj_or_null("byValue")? {
        None => None,
        Some(v) => Some(ValueTyping {
            kinds: v
                .arr("kinds")?
                .iter()
                .map(|k| named(ValueKind::from_name, k.as_str()?, "value kind"))
                .collect::<Result<_, _>>()?,
            absent: named(ValueKind::from_name, v.str("absent")?, "value kind")?,
        }),
    };
    Ok(SqlExpr::PlanParam {
        name: string(o, "name")?,
        kind: named(ParamKind::from_name, o.str("kind")?, "parameter kind")?,
        optional: o.bool("optional")?,
        enum_map_fn: string_or_null(o, "enumMapFn")?,
        r#type,
        by_value,
    })
}

fn struct_lit_fields(a: &[Node]) -> Result<Vec<StructLitField>, Error> {
    objs(a)
        .map(|f| {
            let f = f?;
            Ok(StructLitField {
                name: string(f, "name")?,
                value: read_expr(f.obj("value")?)?,
                declared: f.obj_or_null("declared")?.map(read_type).transpose()?,
            })
        })
        .collect()
}

fn frame(f: &Obj) -> Result<Frame, Error> {
    Ok(Frame {
        kind: named(FrameKind::from_name, f.str("kind")?, "frame kind")?,
        from: bound(f.obj("from")?)?,
        to: bound(f.obj("to")?)?,
    })
}

fn reducer(o: &Obj, r#type: TypeFact) -> Result<Reducer, Error> {
    Ok(Reducer {
        r#fn: named(AggFn::from_name, o.str("fn")?, "aggregate")?,
        args: exprs(o.arr("args")?)?,
        distinct: o.bool("distinct")?,
        order_by: sort_keys(o.arr("orderBy")?)?,
        r#type,
    })
}

/// Java's `readAgg`.
pub fn read_agg(o: &Obj) -> Result<SqlAgg, Error> {
    let r#fn = || named(AggFn::from_name, o.str("fn")?, "aggregate");
    Ok(match tag(o)? {
        "reducer" => SqlAgg::Reducer(reducer(o, read_fact(o.obj("type")?)?)?),
        "rankingFn" => SqlAgg::RankingFn { r#fn: r#fn()?, args: exprs(o.arr("args")?)? },
        "valueFn" => SqlAgg::ValueFn { r#fn: r#fn()?, args: exprs(o.arr("args")?)? },
        other => return Err(unknown_tag("aggregate", other)),
    })
}

fn bound(o: &Obj) -> Result<Bound, Error> {
    Ok(match tag(o)? {
        "unboundedPreceding" => Bound::UnboundedPreceding,
        "preceding" => Bound::Preceding { n: number(o)? },
        "currentRow" => Bound::CurrentRow,
        "following" => Bound::Following { n: number(o)? },
        "unboundedFollowing" => Bound::UnboundedFollowing,
        "intervalPreceding" => Bound::IntervalPreceding { n: o.i64("n")?, unit: string(o, "unit")? },
        "intervalFollowing" => Bound::IntervalFollowing { n: o.i64("n")?, unit: string(o, "unit")? },
        other => return Err(unknown_tag("window bound", other)),
    })
}

/// Java's `readNumber`: a bound's number, its text `n` read as its class `numberClass`.
fn number(o: &Obj) -> Result<JavaNumber, Error> {
    let n = o.str("n")?;
    let bad = || Error(format!("not a window bound number: '{n}'"));
    Ok(match o.str("numberClass")? {
        "Long" => JavaNumber::Long(n.parse().map_err(|_| bad())?),
        "Integer" => JavaNumber::Integer(n.parse().map_err(|_| bad())?),
        "Double" => JavaNumber::Double(Double::parse(n)?),
        "BigDecimal" => JavaNumber::BigDecimal(BigDecimal::parse(n)?),
        other => return Err(unknown("window bound number class", other)),
    })
}

/// Java's `readType`.
pub fn read_type(o: &Obj) -> Result<SqlType, Error> {
    Ok(match tag(o)? {
        "scalar" => SqlType::Scalar(named(Scalar::from_name, o.str("name")?, "scalar type")?),
        "decimal" => SqlType::Decimal { precision: int(o, "precision")?, scale: int(o, "scale")? },
        "array" => SqlType::Array { element: Box::new(read_type(o.obj("element")?)?) },
        "map" => {
            SqlType::Map { key: Box::new(read_type(o.obj("key")?)?), value: Box::new(read_type(o.obj("value")?)?) }
        }
        "struct" => SqlType::Struct {
            fields: objs(o.arr("fields")?)
                .map(|f| {
                    let f = f?;
                    Ok(StructField { name: string(f, "name")?, r#type: read_type(f.obj("type")?)? })
                })
                .collect::<Result<_, Error>>()?,
        },
        "tagged" => SqlType::Tagged {
            slots: objs(o.arr("slots")?)
                .map(|s| {
                    let s = s?;
                    Ok(TaggedSlot { kind: string(s, "kind")?, r#type: read_type(s.obj("type")?)? })
                })
                .collect::<Result<_, Error>>()?,
        },
        other => return Err(unknown_tag("SQL type", other)),
    })
}

/// Java's `readFact`.
pub fn read_fact(o: &Obj) -> Result<TypeFact, Error> {
    Ok(match tag(o)? {
        "typed" => TypeFact::Typed { r#type: read_type(o.obj("sqlType")?)?, nullable: o.bool("nullable")? },
        "bottom" => TypeFact::Bottom,
        "raises" => TypeFact::Raises,
        "unknown" => TypeFact::Unknown,
        other => return Err(unknown_tag("type fact", other)),
    })
}

fn fmt(o: &Obj) -> Result<DateFmt, Error> {
    Ok(match tag(o)? {
        "part" => DateFmt::Part(named(DateFmtPart::from_name, o.str("part")?, "date format part")?),
        "text" => DateFmt::Text { s: string(o, "s")? },
        other => return Err(unknown_tag("date format", other)),
    })
}

/// Java's `readColumnType`.
pub fn read_column_type(o: &Obj) -> Result<ColumnType, Error> {
    Ok(match tag(o)? {
        "plain" => ColumnType::Plain { kind: named(ColumnKind::from_name, o.str("kind")?, "column kind")? },
        "sized" => ColumnType::Sized { kind: string(o, "kind")?, size: int(o, "size")? },
        "scaled" => {
            ColumnType::Scaled { kind: string(o, "kind")?, precision: int(o, "precision")?, scale: int(o, "scale")? }
        }
        other => return Err(unknown_tag("column type", other)),
    })
}

/// Java's `readDdl`.
pub fn read_ddl(o: &Obj) -> Result<SqlDdl, Error> {
    Ok(match tag(o)? {
        "createTable" => SqlDdl::CreateTable {
            schema: string_or_null(o, "schema")?,
            table: string(o, "table")?,
            quoting: quoting(o.obj("quoting")?)?,
            columns: objs(o.arr("columns")?)
                .map(|c| {
                    let c = c?;
                    Ok(DdlColumn {
                        name: string(c, "name")?,
                        declared_quoted: c.bool("declaredQuoted")?,
                        r#type: read_column_type(c.obj("type")?)?,
                        not_null: c.bool("notNull")?,
                        primary_key: c.bool("primaryKey")?,
                    })
                })
                .collect::<Result<_, Error>>()?,
            temporary: o.bool("temporary")?,
        },
        "dropTable" => SqlDdl::DropTable {
            schema: string_or_null(o, "schema")?,
            table: string(o, "table")?,
            quoting: quoting(o.obj("quoting")?)?,
        },
        "createSchema" => SqlDdl::CreateSchema { schema: string(o, "schema")?, quoted: o.bool("quoted")? },
        "dropSchema" => SqlDdl::DropSchema { schema: string(o, "schema")?, quoted: o.bool("quoted")? },
        other => return Err(unknown_tag("DDL statement", other)),
    })
}

/// Java's `readDml`.
pub fn read_dml(o: &Obj) -> Result<SqlDml, Error> {
    let schema = || string_or_null(o, "schema");
    let table = || string(o, "table");
    let name_quoting = || quoting(o.obj("quoting")?);
    let columns = || strings(o.arr("columns")?);
    let quoted_columns = || strings(o.arr("quotedColumns")?);
    Ok(match tag(o)? {
        "insertValues" => SqlDml::InsertValues {
            schema: schema()?,
            table: table()?,
            quoting: name_quoting()?,
            columns: columns()?,
            quoted_columns: quoted_columns()?,
            rows: o.arr("rows")?.iter().map(|r| exprs(r.as_arr()?)).collect::<Result<_, _>>()?,
        },
        "insertFromTable" => SqlDml::InsertFromTable {
            schema: schema()?,
            table: table()?,
            quoting: name_quoting()?,
            columns: columns()?,
            quoted_columns: quoted_columns()?,
            source: string(o, "source")?,
        },
        "deleteAll" => SqlDml::DeleteAll { schema: schema()?, table: table()?, quoting: name_quoting()? },
        "createTempTableAs" => SqlDml::CreateTempTableAs {
            table: table()?,
            query: read_query(o.obj("query")?)?,
            columns: columns()?,
            staged: o.bool("staged")?,
        },
        "insertSelect" => SqlDml::InsertSelect {
            schema: schema()?,
            table: table()?,
            quoting: name_quoting()?,
            columns: columns()?,
            quoted_columns: quoted_columns()?,
            query: read_query(o.obj("query")?)?,
        },
        "update" => SqlDml::Update {
            schema: schema()?,
            table: table()?,
            quoting: name_quoting()?,
            alias: string(o, "alias")?,
            stored_types: stored_types(o.obj("storedTypes")?)?,
            assignments: objs(o.arr("assignments")?)
                .map(|a| {
                    let a = a?;
                    Ok(Assignment {
                        column: string(a, "column")?,
                        quoted: a.bool("quoted")?,
                        value: read_expr(a.obj("value")?)?,
                    })
                })
                .collect::<Result<_, Error>>()?,
            r#where: expr_or_null(o, "where")?,
        },
        "deleteWhere" => SqlDml::DeleteWhere {
            schema: schema()?,
            table: table()?,
            quoting: name_quoting()?,
            alias: string(o, "alias")?,
            stored_types: stored_types(o.obj("storedTypes")?)?,
            r#where: expr_or_null(o, "where")?,
        },
        "merge" => SqlDml::Merge {
            schema: schema()?,
            table: table()?,
            quoting: name_quoting()?,
            columns: columns()?,
            quoted_columns: quoted_columns()?,
            keys: strings(o.arr("keys")?)?,
            query: read_query(o.obj("query")?)?,
            alias: string(o, "alias")?,
            source_alias: string(o, "sourceAlias")?,
        },
        other => return Err(unknown_tag("DML statement", other)),
    })
}
