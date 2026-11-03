// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every tag family round-trips, text to MIR to text, byte for byte; an unknown tag or constant is refused by name.
//! The samples are MirJson as Java's writer spells it: every field, in its order, null for an absent value.

use crate::*;
use lite_json::{Node, Obj, parse_with_depth, to_compact};

// ---------------------------------------------------------------- building MirJson text

/// `{"k":v,...}`, each `v` already JSON.
fn o(fields: &[(&str, &str)]) -> String {
    let body: Vec<String> = fields.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
    format!("{{{}}}", body.join(","))
}

/// An object whose first field is its tag.
fn t(tag: &str, fields: &[(&str, &str)]) -> String {
    let tag = s(tag);
    let mut all = vec![("_type", tag.as_str())];
    all.extend_from_slice(fields);
    o(&all)
}

/// An expression: its tag, its type fact, then its fields.
fn e(tag: &str, fact: &str, fields: &[(&str, &str)]) -> String {
    let tag = s(tag);
    let mut all = vec![("_type", tag.as_str()), ("type", fact)];
    all.extend_from_slice(fields);
    o(&all)
}

fn s(v: &str) -> String {
    format!("\"{v}\"")
}

fn a(items: &[&str]) -> String {
    format!("[{}]", items.join(","))
}

fn scalar(name: &str) -> String {
    t("scalar", &[("name", &s(name))])
}

fn typed(sql: &str, nullable: bool) -> String {
    t("typed", &[("sqlType", sql), ("nullable", &nullable.to_string())])
}

fn bigint() -> String {
    typed(&scalar("BIGINT"), false)
}

fn unknown_fact() -> String {
    t("unknown", &[])
}

fn int_lit(v: i64) -> String {
    e("intLit", &bigint(), &[("value", &v.to_string())])
}

fn str_lit(v: &str) -> String {
    e("stringLit", &typed(&scalar("VARCHAR"), false), &[("value", &s(v))])
}

fn bool_lit() -> String {
    e("boolLit", &typed(&scalar("BOOLEAN"), false), &[("value", "true")])
}

fn col(name: &str) -> String {
    e("column", &typed(&scalar("BIGINT"), true), &[("table", &s("t0")), ("name", &s(name)), ("origin", &s("PHYSICAL"))])
}

fn arr_lit() -> String {
    e("arrayLit", &typed(&t("array", &[("element", &scalar("BIGINT"))]), false), &[("elements", &a(&[&int_lit(1)]))])
}

fn out_col(name: &str) -> String {
    o(&[("name", &s(name)), ("type", &scalar("BIGINT")), ("nullable", "false"), ("origin", &s("DERIVED"))])
}

fn sort_key() -> String {
    o(&[("expr", &col("a")), ("ascending", "false"), ("nullOrder", &s("NULLS_LAST")), ("outputName", "null")])
}

fn reducer_agg() -> String {
    e(
        "reducer",
        &bigint(),
        &[("fn", &s("SUM")), ("args", &a(&[&col("a")])), ("distinct", "false"), ("orderBy", &a(&[&sort_key()]))],
    )
}

fn quoting() -> String {
    o(&[("schema", "false"), ("table", "true")])
}

fn plain(kind: &str) -> String {
    t("plain", &[("kind", &s(kind))])
}

/// The smallest select: one literal projected, FROM-less.
fn select() -> String {
    let projection = o(&[("expr", &int_lit(1)), ("alias", &s("x")), ("out", &out_col("x"))]);
    t(
        "select",
        &[
            ("projections", &a(&[&projection])),
            ("distinct", "false"),
            ("from", &t("dual", &[])),
            ("where", "null"),
            ("groupBy", "[]"),
            ("having", "null"),
            ("qualify", "null"),
            ("orderBy", "[]"),
            ("limit", "null"),
            ("offset", "null"),
            ("outputs", &a(&[&out_col("x")])),
        ],
    )
}

fn select_from(from: &str) -> String {
    t(
        "select",
        &[
            ("projections", "[]"),
            ("distinct", "true"),
            ("from", from),
            ("where", &bool_lit()),
            ("groupBy", &a(&[&col("a")])),
            ("having", &bool_lit()),
            ("qualify", &bool_lit()),
            ("orderBy", &a(&[&sort_key()])),
            ("limit", "10"),
            ("offset", "0"),
            ("outputs", &a(&[&out_col("a")])),
        ],
    )
}

fn table() -> String {
    // stored types in no sorted order: the map keeps its own
    let stored = o(&[("b", &t("sized", &[("kind", &s("VARCHAR")), ("size", "200")])), ("a", &plain("BIGINT"))]);
    t(
        "table",
        &[
            ("name", &s("db.T")),
            ("alias", &s("t0")),
            ("outputs", &a(&[&out_col("a")])),
            ("call", "false"),
            ("storedTypes", &stored),
            ("quoting", &quoting()),
        ],
    )
}

fn lambda() -> String {
    e("lambda", &unknown_fact(), &[("params", &a(&[&s("x"), &s("y")])), ("body", &col("x"))])
}

fn frame(from: &str, to: &str) -> String {
    o(&[("kind", &s("ROWS")), ("from", from), ("to", to)])
}

fn window(r#fn: &str, frame: &str) -> String {
    e(
        "windowCall",
        &bigint(),
        &[("fn", r#fn), ("partitionBy", &a(&[&col("a")])), ("orderBy", &a(&[&sort_key()])), ("frame", frame)],
    )
}

// ---------------------------------------------------------------- round trips

/// `text` read and written back is `text`, byte for byte.
fn round_trip<T>(text: &str, read: fn(&Obj) -> Result<T, Error>, write: fn(&T) -> Node) -> T {
    let node = parse_with_depth(text, MAX_DEPTH).unwrap_or_else(|e| panic!("{e}: {text}"));
    assert_eq!(to_compact(&node), text, "the sample is not compact MirJson");
    let value = read(node.as_obj().unwrap()).unwrap_or_else(|e| panic!("{e}: {text}"));
    assert_eq!(to_compact(&write(&value)), text);
    value
}

fn expr(text: &str) -> SqlExpr {
    round_trip(text, read_expr, write_expr)
}

fn query(text: &str) -> SqlQuery {
    round_trip(text, read_query, write_query)
}

fn refusal<T: std::fmt::Debug>(text: &str, read: fn(&Obj) -> Result<T, Error>) -> String {
    let node = parse_with_depth(text, MAX_DEPTH).unwrap();
    read(node.as_obj().unwrap()).expect_err("refused").0
}

#[test]
fn every_expression_tag_round_trips() {
    let struct_lit = e(
        "structLit",
        &unknown_fact(),
        &[(
            "fields",
            &a(&[
                &o(&[("name", &s("f")), ("value", &int_lit(1)), ("declared", &scalar("BIGINT"))]),
                &o(&[("name", &s("g")), ("value", &str_lit("v")), ("declared", "null")]),
            ]),
        )],
    );
    let by_value = o(&[("kinds", &a(&[&s("INTEGER"), &s("DECIMAL")])), ("absent", &s("DECIMAL"))]);
    let hoist = o(&[("path", &a(&[&s("a"), &s("b")])), ("envelope", &col("e")), ("toMany", "true")]);
    let ranking = t("rankingFn", &[("fn", &s("ROW_NUMBER")), ("args", "[]")]);
    let samples = [
        e("rowOrder", &bigint(), &[("table", &s("t0"))]),
        e("storedRead", &typed(&scalar("VARCHAR"), true), &[("column", &col("a")), ("stored", &plain("OTHER"))]),
        e(
            "reduceCollection",
            &unknown_fact(),
            &[("reducer", &s("STRING_AGG")), ("collection", &arr_lit()), ("extras", &a(&[&str_lit(",")]))],
        ),
        e("membership", &unknown_fact(), &[("needle", &int_lit(1)), ("collection", &arr_lit())]),
        col("a"),
        e("starExcept", &unknown_fact(), &[("table", "null"), ("except", &a(&[&s("a")]))]),
        e("star", &unknown_fact(), &[("table", &s("t0"))]),
        str_lit("it's \\\"quoted\\\", \\\\ \\n \\u0001 é ∑"),
        int_lit(i64::MIN),
        e("floatLit", &unknown_fact(), &[("value", &s("1.0E10"))]),
        e("decimalLit", &unknown_fact(), &[("value", &s("-1.230E+5"))]),
        bool_lit(),
        e("nullLit", &t("bottom", &[]), &[]),
        e("dateLit", &unknown_fact(), &[("iso", &s("2024-02-29"))]),
        e("group", &bigint(), &[("inner", &int_lit(1))]),
        e(
            "planParam",
            &unknown_fact(),
            &[
                ("name", &s("p")),
                ("kind", &s("STRICT_DATE")),
                ("optional", "true"),
                ("enumMapFn", &s("f")),
                ("byValue", &by_value),
            ],
        ),
        e(
            "formatLit",
            &unknown_fact(),
            &[("parts", &a(&[&t("part", &[("part", &s("YEAR4"))]), &t("text", &[("s", &s("-"))])]))],
        ),
        e("datePartLit", &unknown_fact(), &[("part", &s("ISO_DAY_OF_WEEK"))]),
        e("jsonKindLit", &unknown_fact(), &[("kind", &s("UNSIGNED_INTEGER"))]),
        e("typeNameLit", &unknown_fact(), &[("scalar", &s("TIMESTAMPTZ"))]),
        e("timestampLit", &unknown_fact(), &[("iso", &s("2024-01-02T03:04:05.123456"))]),
        e("timeLit", &unknown_fact(), &[("iso", &s("03:04:05"))]),
        e("orderedListAgg", &unknown_fact(), &[("value", &col("a")), ("orderBy", &col("b"))]),
        arr_lit(),
        struct_lit.clone(),
        e("structGet", &bigint(), &[("source", &struct_lit), ("field", &s("f"))]),
        e("call", &bigint(), &[("fn", &s("PLUS")), ("args", &a(&[&int_lit(1), &int_lit(2)]))]),
        e(
            "caseExpr",
            &bigint(),
            &[("whens", &a(&[&o(&[("condition", &bool_lit()), ("then", &int_lit(1))])])), ("otherwise", "null")],
        ),
        e("exists", &unknown_fact(), &[("subquery", &select())]),
        e("inSubquery", &unknown_fact(), &[("value", &int_lit(1)), ("subquery", &select())]),
        e("checkedDefects", &unknown_fact(), &[("own", &int_lit(1)), ("hoists", &a(&[&hoist]))]),
        e("checkedChildValue", &unknown_fact(), &[("envelope", &col("e")), ("toMany", "false")]),
        e(
            "quantified",
            &unknown_fact(),
            &[("value", &int_lit(1)), ("comparison", &s("LESS")), ("quantifier", &s("ALL")), ("subquery", &select())],
        ),
        e("scalarSubquery", &bigint(), &[("subquery", &select())]),
        e("checkedOne", &bigint(), &[("list", &arr_lit()), ("scalarCarrier", "true"), ("atLeastOnly", "false")]),
        e("compactList", &unknown_fact(), &[("list", &arr_lit())]),
        e(
            "deferredTdsString",
            &unknown_fact(),
            &[("inner", &select()), ("alias", &s("x")), ("id", "3"), ("form", &s("CSV")), ("renderTdsNull", "true")],
        ),
        e("tempTableInSplice", &unknown_fact(), &[("tempTableName", &s("tt"))]),
        e("jsonObject", &unknown_fact(), &[("kv", &a(&[&str_lit("k"), &int_lit(1)]))]),
        e("jsonArray", &unknown_fact(), &[("elements", &a(&[&int_lit(1)]))]),
        e(
            "jsonArrayAgg",
            &unknown_fact(),
            &[("value", &col("a")), ("orderKeys", &a(&[&o(&[("expr", &col("b")), ("desc", "true")])]))],
        ),
        window(&ranking, "null"),
        lambda(),
        e(
            "cast",
            &unknown_fact(),
            &[
                ("value", &int_lit(1)),
                ("target", &t("decimal", &[("precision", "38"), ("scale", "2")])),
                ("conform", "true"),
            ],
        ),
        e(
            "foldCall",
            &unknown_fact(),
            &[
                ("source", &arr_lit()),
                ("lambda", &lambda()),
                ("init", &int_lit(0)),
                ("accIsList", "false"),
                ("homogeneous", "true"),
            ],
        ),
        reducer_agg(),
        e("keyCell", &unknown_fact(), &[("cell", "0")]),
    ];
    let mut tags: Vec<&str> = samples.iter().map(|text| expr_tag(&expr(text))).collect();
    tags.sort_unstable();
    let mut all = EXPR_TAGS.to_vec();
    all.sort_unstable();
    assert_eq!(tags, all, "one sample per expression tag");
}

#[test]
fn the_values_read_are_the_records_fields() {
    let SqlExpr::Column(c) = expr(&col("a")) else { panic!() };
    assert_eq!(
        c,
        Column {
            table: Some("t0".to_owned()),
            name: "a".to_owned(),
            r#type: TypeFact::Typed { r#type: SqlType::Scalar(Scalar::Bigint), nullable: true },
            origin: Some(Origin::Physical),
        }
    );
    let SqlExpr::IntLit { value, .. } = expr(&int_lit(i64::MAX)) else { panic!() };
    assert_eq!(value, i64::MAX);
    let SqlExpr::FloatLit { value, .. } = expr(&e("floatLit", &unknown_fact(), &[("value", &s("4.9E-324"))])) else {
        panic!()
    };
    assert_eq!((value.value(), value.text()), (f64::from_bits(1), "4.9E-324"));
    let SqlExpr::Call { r#fn, args, .. } = expr(&e("call", &bigint(), &[("fn", &s("LIST_FOR_ALL")), ("args", "[]")]))
    else {
        panic!()
    };
    assert_eq!((r#fn, args.len()), (SqlFn::ListForAll, 0));
}

#[test]
fn every_query_tag_round_trips() {
    let SqlQuery::Select(sel) = query(&select()) else { panic!() };
    assert_eq!((sel.limit, sel.offset, sel.from), (None, None, SqlSource::Dual));
    let SqlQuery::Select(sel) = query(&select_from(&table())) else { panic!() };
    assert_eq!((sel.limit, sel.offset, sel.distinct), (Some(10), Some(0), true));
    query(&t(
        "union",
        &[("branches", &a(&[&select(), &select()])), ("all", "true"), ("outputs", &a(&[&out_col("x")]))],
    ));
    let cte = o(&[("name", &s("c")), ("query", &select()), ("materialized", "true")]);
    query(&t("with", &[("ctes", &a(&[&cte])), ("body", &select()), ("recursive", "false")]));
}

#[test]
fn every_source_tag_round_trips() {
    let cte = t("cte", &[("name", &s("c")), ("alias", &s("c0")), ("outputs", "[]")]);
    let using = o(&[("agg", &reducer_agg()), ("alias", &s("u")), ("type", "null")]);
    let typed_using = o(&[("agg", &reducer_agg()), ("alias", &s("v")), ("type", &scalar("DOUBLE"))]);
    let sources = [
        table(),
        t("subselect", &[("inner", &select()), ("alias", &s("s")), ("frameName", "null")]),
        t("subselect", &[("inner", &select()), ("alias", &s("s")), ("frameName", &s("existsKeys"))]),
        cte.clone(),
        t(
            "join",
            &[
                ("left", &table()),
                ("right", &cte),
                ("kind", &s("LEFT_LATERAL")),
                ("on", &bool_lit()),
                ("outputs", &a(&[&out_col("a")])),
            ],
        ),
        t("join", &[("left", &table()), ("right", &cte), ("kind", &s("CROSS")), ("on", "null"), ("outputs", "[]")]),
        t(
            "pivot",
            &[
                ("source", &table()),
                ("on", &a(&[&col("a")])),
                ("in", &a(&[&str_lit("x")])),
                ("named", "[]"),
                ("usings", &a(&[&using, &typed_using])),
                ("alias", &s("p")),
                ("outputs", "[]"),
            ],
        ),
        t(
            "values",
            &[
                ("rows", &a(&[&a(&[&int_lit(1), &str_lit("a")]), &a(&[&int_lit(2), &str_lit("b")])])),
                ("columns", &a(&[&s("n"), &s("s")])),
                ("alias", &s("v")),
                ("outputs", &a(&[&out_col("n")])),
            ],
        ),
        t("dual", &[]),
        t("sourceUrl", &[("url", &s("file:///x.csv")), ("alias", &s("u")), ("outputs", "[]")]),
        t("rawSql", &[("sql", &s("select 1")), ("alias", &s("r")), ("outputs", "[]"), ("writtenFor", &s("DuckDB"))]),
        t(
            "catalog",
            &[
                ("grid", &s("COLUMNS")),
                ("schemaPattern", &s("s%")),
                ("tablePattern", "null"),
                ("columnPattern", &s("c%")),
                ("alias", &s("c")),
                ("outputs", "[]"),
            ],
        ),
        t("varSet", &[("varName", &s("v")), ("alias", &s("vs")), ("outputs", "[]")]),
    ];
    for source in &sources {
        round_trip(source, read_source, write_source);
        query(&select_from(source));
    }
    let SqlSource::Table { stored_types, quoting, .. } = round_trip(&table(), read_source, write_source) else {
        panic!()
    };
    let names: Vec<&str> = stored_types.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!((names, quoting), (vec!["b", "a"], NameQuoting { schema: false, table: true }));
}

#[test]
fn every_aggregate_type_fact_and_format_tag_round_trips() {
    for agg in [
        reducer_agg(),
        t("rankingFn", &[("fn", &s("NTILE")), ("args", &a(&[&int_lit(4)]))]),
        t("valueFn", &[("fn", &s("LAG")), ("args", &a(&[&col("a"), &int_lit(1)]))]),
    ] {
        round_trip(&agg, read_agg, write_agg);
    }
    let struct_type = t(
        "struct",
        &[(
            "fields",
            &a(&[
                &o(&[("name", &s("f")), ("type", &scalar("JSON"))]),
                &o(&[("name", &s("g")), ("type", &scalar("DATE"))]),
            ]),
        )],
    );
    let tagged = t(
        "tagged",
        &[(
            "slots",
            &a(&[
                &o(&[("kind", &s("Integer")), ("type", &scalar("BIGINT"))]),
                &o(&[("kind", &s("Float")), ("type", &scalar("DOUBLE"))]),
            ]),
        )],
    );
    for ty in [
        scalar("DECIMAL_TEXT"),
        t("decimal", &[("precision", "18"), ("scale", "0")]),
        t("array", &[("element", &t("array", &[("element", &scalar("VARCHAR"))]))]),
        t("map", &[("key", &scalar("VARCHAR")), ("value", &struct_type)]),
        struct_type.clone(),
        tagged,
    ] {
        round_trip(&ty, read_type, write_type);
    }
    for fact in [typed(&scalar("TIME"), true), t("bottom", &[]), t("raises", &[]), unknown_fact()] {
        round_trip(&fact, read_fact, write_fact);
    }
    let parts: Vec<String> = DateFmtPart::ALL.iter().map(|p| t("part", &[("part", &s(p.as_str()))])).collect();
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    expr(&e("formatLit", &unknown_fact(), &[("parts", &a(&parts))]));
}

#[test]
fn every_window_bound_and_number_class_round_trips() {
    let bound = |tag: &str, n: &str, class: &str| t(tag, &[("n", &s(n)), ("numberClass", &s(class))]);
    let interval = |tag: &str| t(tag, &[("n", "3"), ("unit", &s("DAY"))]);
    let ranking = t("rankingFn", &[("fn", &s("RANK")), ("args", "[]")]);
    for (from, to) in [
        (t("unboundedPreceding", &[]), t("currentRow", &[])),
        (t("currentRow", &[]), t("unboundedFollowing", &[])),
        (bound("preceding", "-9223372036854775808", "Long"), bound("following", "2147483647", "Integer")),
        (bound("preceding", "2.5", "Double"), bound("following", "1.50E+3", "BigDecimal")),
        (bound("preceding", "1.0E-5", "Double"), bound("following", "-0.000", "BigDecimal")),
        (interval("intervalPreceding"), interval("intervalFollowing")),
    ] {
        expr(&window(&ranking, &frame(&from, &to)));
        expr(&window(&reducer_agg(), &frame(&from, &to)));
    }
    let SqlExpr::WindowCall { frame: Some(f), .. } = expr(&window(
        &ranking,
        &frame(&bound("preceding", "7", "Integer"), &bound("following", "123.4560", "BigDecimal")),
    )) else {
        panic!()
    };
    assert_eq!(f.from, Bound::Preceding { n: JavaNumber::Integer(7) });
    let Bound::Following { n: JavaNumber::BigDecimal(d) } = f.to else { panic!() };
    assert_eq!(d.as_str(), "123.4560");
}

#[test]
fn numbers_keep_the_text_java_wrote() {
    for text in ["1.0", "-0.0", "1.0E10", "4.9E-324", "1.7976931348623157E308", "NaN", "Infinity", "-Infinity"] {
        expr(&e("floatLit", &unknown_fact(), &[("value", &s(text))]));
    }
    for text in ["0", "-0.00", "123456789012345678901234567890.123456789", "1E+10", "1.23E-7"] {
        expr(&e("decimalLit", &unknown_fact(), &[("value", &s(text))]));
    }
    for bad in ["", "1.2.3", "e5", "1e", "1e+", "0x10", "1,5", ".", "-"] {
        assert!(BigDecimal::parse(bad).is_err(), "{bad}");
    }
    for bad in ["", "1.0x", "one"] {
        assert!(Double::parse(bad).is_err(), "{bad}");
    }
    for v in [i64::MIN, -1, 0, i64::MAX] {
        expr(&int_lit(v));
    }
    let not_int = e("keyCell", &unknown_fact(), &[("cell", "2147483648")]);
    assert_eq!(refusal(&not_int, read_expr), "field 'cell' is not an int: 2147483648");
}

#[test]
fn every_column_type_and_ddl_tag_round_trips() {
    for ty in [
        plain("TIMESTAMP_NS"),
        t("sized", &[("kind", &s("VARCHAR")), ("size", "255")]),
        t("scaled", &[("kind", &s("DECIMAL")), ("precision", "38"), ("scale", "10")]),
    ] {
        round_trip(&ty, read_column_type, write_column_type);
    }
    let column = |name: &str, ty: &str| {
        o(&[("name", &s(name)), ("declaredQuoted", "true"), ("type", ty), ("notNull", "true"), ("primaryKey", "false")])
    };
    for ddl in [
        t(
            "createTable",
            &[
                ("schema", "null"),
                ("table", &s("T")),
                ("quoting", &quoting()),
                (
                    "columns",
                    &a(&[
                        &column("a", &plain("BIGINT")),
                        &column("b", &t("sized", &[("kind", &s("CHAR")), ("size", "1")])),
                    ]),
                ),
                ("temporary", "true"),
            ],
        ),
        t("dropTable", &[("schema", &s("S")), ("table", &s("T")), ("quoting", &quoting())]),
        t("createSchema", &[("schema", &s("S")), ("quoted", "true")]),
        t("dropSchema", &[("schema", &s("S")), ("quoted", "false")]),
    ] {
        round_trip(&ddl, read_ddl, write_ddl);
    }
}

#[test]
fn every_dml_tag_round_trips() {
    let target = |tag: &str, rest: &[(&str, &str)]| {
        let mut fields = vec![("schema", "null"), ("table", "\"T\""), ("quoting", "{\"schema\":true,\"table\":false}")];
        fields.extend_from_slice(rest);
        t(tag, &fields)
    };
    let columns = a(&[&s("a"), &s("b")]);
    let quoted = a(&[&s("b")]);
    let stored = o(&[("a", &plain("JSON"))]);
    let assignment = o(&[("column", &s("a")), ("quoted", "false"), ("value", &int_lit(1))]);
    for dml in [
        target(
            "insertValues",
            &[("columns", &columns), ("quotedColumns", &quoted), ("rows", &a(&[&a(&[&int_lit(1), &str_lit("x")])]))],
        ),
        target("insertFromTable", &[("columns", &columns), ("quotedColumns", "[]"), ("source", &s("src"))]),
        target("deleteAll", &[]),
        t(
            "createTempTableAs",
            &[("table", &s("tmp")), ("query", &select()), ("columns", &columns), ("staged", "true")],
        ),
        target("insertSelect", &[("columns", &columns), ("quotedColumns", &quoted), ("query", &select())]),
        target(
            "update",
            &[
                ("alias", &s("t")),
                ("storedTypes", &stored),
                ("assignments", &a(&[&assignment])),
                ("where", &bool_lit()),
            ],
        ),
        target("deleteWhere", &[("alias", &s("t")), ("storedTypes", "{}"), ("where", "null")]),
        target(
            "merge",
            &[
                ("columns", &columns),
                ("quotedColumns", &quoted),
                ("keys", &a(&[&s("a")])),
                ("query", &select()),
                ("alias", &s("t")),
                ("sourceAlias", &s("s")),
            ],
        ),
    ] {
        round_trip(&dml, read_dml, write_dml);
    }
}

#[test]
fn an_unknown_tag_or_constant_is_refused_by_name() {
    assert_eq!(refusal(&e("nope", &bigint(), &[]), read_expr), "unknown expression tag 'nope'");
    assert_eq!(refusal(&t("nope", &[]), read_query), "unknown query tag 'nope'");
    assert_eq!(refusal(&t("nope", &[]), read_source), "unknown source tag 'nope'");
    assert_eq!(refusal(&t("nope", &[]), read_agg), "unknown aggregate tag 'nope'");
    assert_eq!(refusal(&t("nope", &[]), read_type), "unknown SQL type tag 'nope'");
    assert_eq!(refusal(&t("nope", &[]), read_fact), "unknown type fact tag 'nope'");
    assert_eq!(refusal(&t("nope", &[]), read_column_type), "unknown column type tag 'nope'");
    assert_eq!(refusal(&t("nope", &[]), read_ddl), "unknown DDL statement tag 'nope'");
    assert_eq!(refusal(&t("nope", &[]), read_dml), "unknown DML statement tag 'nope'");
    // Java's own `Tagged`s are resolved before a plan is written: no tag
    assert_eq!(refusal(&e("tagged", &bigint(), &[]), read_expr), "unknown expression tag 'tagged'");
    let call = e("call", &bigint(), &[("fn", &s("NOPE")), ("args", "[]")]);
    assert_eq!(refusal(&call, read_expr), "unknown function 'NOPE'");
    assert_eq!(refusal(&scalar("NOPE"), read_type), "unknown scalar type 'NOPE'");
    let bound = t("preceding", &[("n", &s("1")), ("numberClass", &s("Short"))]);
    let ranking = t("rankingFn", &[("fn", &s("RANK")), ("args", "[]")]);
    let windowed = window(&ranking, &frame(&bound, &t("currentRow", &[])));
    assert_eq!(refusal(&windowed, read_expr), "unknown window bound number class 'Short'");
    let windowed = window(&ranking, &frame(&t("sideways", &[]), &t("currentRow", &[])));
    assert_eq!(refusal(&windowed, read_expr), "unknown window bound tag 'sideways'");
}

#[test]
fn a_narrowly_typed_field_takes_only_its_type() {
    let stored = e("storedRead", &bigint(), &[("column", &int_lit(1)), ("stored", &plain("OTHER"))]);
    assert_eq!(refusal(&stored, read_expr), "a stored read's column is not a column");
    let fold = e(
        "foldCall",
        &bigint(),
        &[
            ("source", &arr_lit()),
            ("lambda", &col("f")),
            ("init", &int_lit(0)),
            ("accIsList", "false"),
            ("homogeneous", "true"),
        ],
    );
    assert_eq!(refusal(&fold, read_expr), "a fold's lambda is not a lambda");
    let union = t("union", &[("branches", &a(&[&select(), &select()])), ("all", "false"), ("outputs", "[]")]);
    let deferred = e(
        "deferredTdsString",
        &bigint(),
        &[("inner", &union), ("alias", &s("x")), ("id", "1"), ("form", &s("TDS_STRING")), ("renderTdsNull", "false")],
    );
    assert_eq!(refusal(&deferred, read_expr), "a deferred TDS string's query is not a select");
    let using =
        o(&[("agg", &t("valueFn", &[("fn", &s("LAG")), ("args", "[]")])), ("alias", &s("u")), ("type", "null")]);
    let pivot = t(
        "pivot",
        &[
            ("source", &table()),
            ("on", "[]"),
            ("in", "[]"),
            ("named", "[]"),
            ("usings", &a(&[&using])),
            ("alias", &s("p")),
            ("outputs", "[]"),
        ],
    );
    assert_eq!(refusal(&pivot, read_source), "a pivot's aggregate is not a reducer");
}

#[test]
fn an_absent_nullable_field_reads_as_null_and_is_written_as_null() {
    // Java's `getStringOr`/`objOrNull` take a missing field as null; its writer always writes the field
    let missing = o(&[("expr", &col("a")), ("ascending", "true")]);
    let text = t(
        "select",
        &[
            ("projections", "[]"),
            ("distinct", "false"),
            ("from", &t("dual", &[])),
            ("groupBy", "[]"),
            ("orderBy", &a(&[&missing])),
            ("outputs", "[]"),
        ],
    );
    let node = parse_with_depth(&text, MAX_DEPTH).unwrap();
    let q = read_query(node.as_obj().unwrap()).unwrap();
    let written = to_compact(&write_query(&q));
    assert!(written.contains("\"where\":null,\"groupBy\":[],\"having\":null,\"qualify\":null"), "{written}");
    assert!(written.contains("\"nullOrder\":null,\"outputName\":null"), "{written}");
    assert!(written.ends_with("\"limit\":null,\"offset\":null,\"outputs\":[]}"), "{written}");
    let missing_required = t("cte", &[("name", &s("c")), ("outputs", "[]")]);
    assert_eq!(refusal(&missing_required, read_source), "missing field 'alias'");
    // an output column's origin is no nullable field: Java's reader takes a name only
    let null_origin = o(&[("name", &s("x")), ("type", &scalar("BIGINT")), ("nullable", "false"), ("origin", "null")]);
    let cte = t("cte", &[("name", &s("c")), ("alias", &s("c0")), ("outputs", &a(&[&null_origin]))]);
    assert_eq!(refusal(&cte, read_source), "field 'origin' is null, not a string");
}

#[test]
fn every_name_table_is_javas() {
    fn names<T: Copy + PartialEq + std::fmt::Debug>(
        all: &[T],
        as_str: fn(T) -> &'static str,
        from_name: fn(&str) -> Option<T>,
    ) -> usize {
        for &v in all {
            assert_eq!(from_name(as_str(v)), Some(v));
            assert!(as_str(v).bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'));
        }
        assert_eq!(from_name("NOPE"), None);
        assert_eq!(from_name(&as_str(all[0]).to_lowercase()), None, "names are case-sensitive");
        all.len()
    }
    assert_eq!(names(SqlFn::ALL, SqlFn::as_str, SqlFn::from_name), 181);
    assert_eq!(names(AggFn::ALL, AggFn::as_str, AggFn::from_name), 40);
    assert_eq!(names(Scalar::ALL, Scalar::as_str, Scalar::from_name), 13);
    assert_eq!(names(DateFmtPart::ALL, DateFmtPart::as_str, DateFmtPart::from_name), 15);
    assert_eq!(names(DatePart::ALL, DatePart::as_str, DatePart::from_name), 13);
    assert_eq!(names(JsonKind::ALL, JsonKind::as_str, JsonKind::from_name), 8);
    assert_eq!(names(ValueKind::ALL, ValueKind::as_str, ValueKind::from_name), 6);
    assert_eq!(names(Origin::ALL, Origin::as_str, Origin::from_name), 4);
    assert_eq!(names(NullOrder::ALL, NullOrder::as_str, NullOrder::from_name), 2);
    assert_eq!(names(JoinKind::ALL, JoinKind::as_str, JoinKind::from_name), 8);
    assert_eq!(names(CatalogGrid::ALL, CatalogGrid::as_str, CatalogGrid::from_name), 4);
    assert_eq!(names(ParamKind::ALL, ParamKind::as_str, ParamKind::from_name), 9);
    assert_eq!(names(Quantifier::ALL, Quantifier::as_str, Quantifier::from_name), 2);
    assert_eq!(names(TdsForm::ALL, TdsForm::as_str, TdsForm::from_name), 2);
    assert_eq!(names(FrameKind::ALL, FrameKind::as_str, FrameKind::from_name), 2);
    assert_eq!(names(ColumnKind::ALL, ColumnKind::as_str, ColumnKind::from_name), 18);
    assert_eq!(
        (SqlFn::Log10.as_str(), SqlFn::Sha256.as_str(), SqlFn::Error.to_string()),
        ("LOG10", "SHA256", "ERROR".to_owned())
    );
}

#[test]
fn a_deep_tree_round_trips() {
    // a left-deep chain of 1000 calls, as a long boolean conjunction lowers
    let mut text = int_lit(0);
    for i in 1..1000 {
        text = e("call", &bigint(), &[("fn", &s("PLUS")), ("args", &a(&[&text, &int_lit(i)]))]);
    }
    let run = move || {
        expr(&text);
    };
    std::thread::Builder::new().stack_size(256 << 20).spawn(run).unwrap().join().unwrap();
}
