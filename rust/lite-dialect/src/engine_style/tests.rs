// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The engine-text dialects' hooks held to their Java (`EngineStyleH2.java`, `EngineStyleDB2.java`,
//! `EngineStyleComposite.java`): each spelling and refusal on a hand-built node, the inputs the golden oracles hold few
//! or none of among them (DB2's interval and truncation shapes, Composite's spellings, the time-zone shift, the alias
//! plan read after a render). The oracles hold the rest (`tests/dialect_differential.rs`).

use super::*;
use crate::build::{call, case, cast, cast_conform, decimal_lit, float_lit, int_lit, null_lit, string_lit, when};
use lite_sql::{BigDecimal, Column, NameQuoting, Origin, OutputCol, TypeFact};

// ---- builders

fn h2() -> EngineStyle {
    EngineStyle::h2()
}

fn db2() -> EngineStyle {
    EngineStyle::db2()
}

fn composite() -> EngineStyle {
    EngineStyle::composite()
}

fn in_zone(tz: &str) -> EngineStyle {
    EngineStyle::new(Profile::H2, false, Some(tz.to_owned()))
}

/// A bare (table-less) physical column.
fn bare(name: &str) -> SqlExpr {
    SqlExpr::Column(Column {
        table: None,
        name: name.into(),
        r#type: TypeFact::Unknown,
        origin: Some(Origin::Physical),
    })
}

/// A physical column of `table`.
fn col(table: &str, name: &str) -> SqlExpr {
    SqlExpr::Column(Column {
        table: Some(table.into()),
        name: name.into(),
        r#type: TypeFact::Unknown,
        origin: Some(Origin::Physical),
    })
}

fn table(name: &str, alias: &str) -> SqlSource {
    SqlSource::Table {
        name: name.into(),
        alias: alias.into(),
        outputs: Vec::new(),
        call: false,
        stored_types: Vec::new(),
        quoting: NameQuoting::default(),
    }
}

fn proj(e: SqlExpr, alias: &str) -> Projection {
    Projection { expr: e, alias: Some(alias.into()), out: None }
}

fn select(projections: Vec<Projection>, from: SqlSource) -> SqlSelect {
    crate::build::select_of(projections, from, Vec::new()).unwrap()
}

fn query(s: SqlSelect) -> SqlQuery {
    SqlQuery::Select(Box::new(s))
}

fn eq(a: SqlExpr, b: SqlExpr) -> SqlExpr {
    call(SqlFn::Equal, vec![a, b])
}

fn param(name: &str, kind: ParamKind, optional: bool) -> SqlExpr {
    SqlExpr::PlanParam {
        name: name.into(),
        kind,
        optional,
        enum_map_fn: None,
        r#type: TypeFact::Unknown,
        by_value: None,
    }
}

fn enum_param(name: &str, optional: bool) -> SqlExpr {
    SqlExpr::PlanParam {
        name: name.into(),
        kind: ParamKind::Enum,
        optional,
        enum_map_fn: Some("f".into()),
        r#type: TypeFact::Unknown,
        by_value: None,
    }
}

fn date_part(p: DatePart) -> SqlExpr {
    SqlExpr::DatePartLit { part: p, r#type: TypeFact::Unknown }
}

fn format(parts: Vec<DateFmt>) -> SqlExpr {
    SqlExpr::FormatLit { parts, r#type: TypeFact::Unknown }
}

fn text(s: &str) -> DateFmt {
    DateFmt::Text { s: s.into() }
}

fn array(elements: Vec<SqlExpr>) -> SqlExpr {
    crate::build::array_lit(elements)
}

fn try_expr(d: &EngineStyle, e: &SqlExpr) -> Result<String, Refusal> {
    let mut w = SqlWriter::new();
    d.expr(&mut w, e, 0)?;
    w.into_text()
}

fn expr(d: &EngineStyle, e: &SqlExpr) -> String {
    try_expr(d, e).unwrap_or_else(|r| panic!("{r}"))
}

fn refused(d: &EngineStyle, e: &SqlExpr) -> Refusal {
    try_expr(d, e).unwrap_err()
}

fn render(d: &EngineStyle, q: SqlQuery) -> String {
    SqlDialect::render(d, q).unwrap_or_else(|r| panic!("{r}"))
}

fn key(e: SqlExpr, ascending: bool) -> SortKey {
    crate::build::sort_key(e, ascending, None, None)
}

fn reducer(f: AggFn, args: Vec<SqlExpr>, distinct: bool, order_by: Vec<SortKey>) -> SqlExpr {
    SqlExpr::Reducer(crate::build::reducer(f, args, distinct, order_by))
}

// ---- the alias plan

#[test]
fn a_render_plans_the_engines_aliases() {
    // a self-join: the leftmost source is "root" and spends its group's index 0; the other takes index 1
    let on = eq(col("t0", "ID"), col("t1", "ID"));
    let from = crate::build::join(table("personTable", "t0"), table("personTable", "t1"), JoinKind::Inner, Some(on));
    let q = query(select(vec![proj(col("t0", "ID"), "id"), proj(col("t1", "NAME"), "name")], from.unwrap()));
    assert_eq!(
        render(&h2(), q),
        "select \"root\".ID as \"id\", \"persontable_1\".NAME as \"name\" from personTable as \"root\" inner join \
         personTable as \"persontable_1\" on (\"root\".ID = \"persontable_1\".ID)"
    );
    // a joined subselect: grouped by its first inner table, numbered before its interior (pre-order), whose own root
    // spends the group's next index; a read of its projected alias quoted, of any other column bare
    let inner = query(select(vec![proj(col("t2", "X"), "X")], table("s.tableB", "t2")));
    let on = eq(col("t0", "ID"), col("t1", "ID"));
    let from =
        crate::build::join(table("tableA", "t0"), crate::build::subselect(inner, "t1", None), JoinKind::Left, Some(on));
    let q = query(select(vec![proj(col("t1", "X"), "x")], from.unwrap()));
    let d = h2();
    assert_eq!(
        render(&d, q.clone()),
        "select \"tableb_0\".\"X\" as \"x\" from tableA as \"root\" left outer join (select \"root\".X as \"X\" from \
         s.tableB as \"root\") as \"tableb_0\" on (\"root\".ID = \"tableb_0\".ID)"
    );
    // renderedAlias reads the plan the render left; renderAsAliased keeps the aliases the query carries
    assert_eq!(d.rendered_alias("t0"), "root");
    assert_eq!(d.rendered_alias("t1"), "tableb_0");
    assert_eq!(d.rendered_alias("t2"), "root");
    assert_eq!(d.rendered_alias("t9"), "t9");
    assert_eq!(
        d.render_as_aliased(q.clone()).unwrap(),
        "select \"t1\".X as \"x\" from tableA as \"t0\" left outer join (select \"t2\".X as \"X\" from s.tableB as \
         \"t2\") as \"t1\" on (\"t0\".ID = \"t1\".ID)"
    );
    assert_eq!(d.rendered_alias("t0"), "t0");
    // the seam's replay: a fresh dialect plans the rendered query
    let fresh = h2();
    assert_eq!(fresh.rendered_alias("t1"), "t1");
    fresh.plan_aliases(q).unwrap();
    assert_eq!(fresh.rendered_alias("t1"), "tableb_0");
}

#[test]
fn frames_are_named_and_spelled_as_the_engine_writes_them() {
    // a view frame groups by its own name; its projections are aliased unquoted
    let view = query(select(vec![proj(col("t2", "ORDER_ID"), "ORDER_ID")], table("orderPnlTable", "t2")));
    let on = eq(col("t1", "ORDER_ID"), col("t0", "ID"));
    let from = crate::build::join(
        table("orderTable", "t0"),
        crate::build::subselect(view, "t1", Some("OrderPnlView".into())),
        JoinKind::Left,
        Some(on),
    );
    assert_eq!(
        render(&h2(), query(select(vec![proj(col("t1", "ORDER_ID"), "o")], from.unwrap()))),
        "select \"orderpnlview_0\".ORDER_ID as \"o\" from orderTable as \"root\" left outer join (select \
         \"root\".ORDER_ID as ORDER_ID from orderPnlTable as \"root\") as \"orderpnlview_0\" on \
         (\"orderpnlview_0\".ORDER_ID = \"root\".ID)"
    );
    // the leftmost DISTINCT materialization keeps the root name, its interior the group's next index
    let mut distinct = select(vec![proj(col("t1", "ID"), "ID")], table("personTable", "t1"));
    distinct.distinct = true;
    let from = crate::build::subselect(query(distinct), "t0", None);
    assert_eq!(
        render(&h2(), query(select(vec![proj(col("t0", "ID"), "id")], from))),
        "select \"root\".ID as \"id\" from (select distinct \"root\".ID as \"ID\" from personTable as \"root\") as \
         \"root\""
    );
    // an exists-key frame's DISTINCT keys are bare where self-aliased; its interior continues the numbering
    let mut keys = select(vec![proj(col("t1", "FIRMID"), "FIRMID")], table("personTable", "t1"));
    keys.distinct = true;
    let on = eq(col("t0", "ID"), col("t2", "FIRMID"));
    let from = crate::build::join(
        table("firmTable", "t0"),
        crate::build::subselect(query(keys), "t2", Some(EXISTS_KEYS_FRAME.into())),
        JoinKind::Inner,
        Some(on),
    );
    assert_eq!(
        render(&h2(), query(select(vec![proj(col("t0", "ID"), "id")], from.unwrap()))),
        "select \"root\".ID as \"id\" from firmTable as \"root\" inner join (select distinct \"persontable_1\".FIRMID \
         from personTable as \"persontable_1\") as \"persontable_0\" on (\"root\".ID = \"persontable_0\".FIRMID)"
    );
    // a cross-store plan variable: its variable's group, its columns bare
    let from = crate::build::join(
        table("firmTable", "t0"),
        SqlSource::VarSetPlaceholder { var_name: "tdsVar".into(), alias: "t1".into(), outputs: Vec::new() },
        JoinKind::Inner,
        Some(eq(col("t0", "ID"), col("t1", "fid"))),
    );
    assert_eq!(
        render(&h2(), query(select(vec![proj(col("t1", "fid"), "f")], from.unwrap()))),
        "select \"tdsvar_0\".fid as \"f\" from firmTable as \"root\" inner join (${tdsVar}) as \"tdsvar_0\" on \
         (\"root\".ID = \"tdsvar_0\".fid)"
    );
    // a correlated EXISTS joins the outer numbering, its projections the literal 1
    let sub = SqlSelect {
        r#where: Some(eq(col("t1", "ID"), col("t0", "ID"))),
        ..select(vec![proj(col("t1", "X"), "x")], table("certTable", "t1"))
    };
    let outer =
        SqlSelect { r#where: Some(crate::build::exists(query(sub))), ..select(Vec::new(), table("personTable", "t0")) };
    assert_eq!(
        render(&h2(), query(outer)),
        "select * from personTable as \"root\" where exists (select 1 from certTable as \"certtable_0\" where \
         \"certtable_0\".ID = \"root\".ID)"
    );
}

#[test]
fn a_tds_joins_bare_star_is_wrapped_and_its_select_list_read() {
    let out = |n: &str| OutputCol {
        name: n.into(),
        r#type: SqlType::Scalar(Scalar::Bigint),
        nullable: true,
        origin: Origin::Physical,
    };
    let on = eq(col("t0", "ID"), col("t1", "ID"));
    let join = crate::build::join(table("tableA", "t0"), table("tableB", "t1"), JoinKind::Inner, Some(on)).unwrap();
    let star = SqlSelect { outputs: vec![out("a"), out("b")], ..select(Vec::new(), join) };
    let d = h2();
    assert_eq!(
        render(&d, query(star.clone())),
        "select \"tablea_0\".\"a\" as \"a\", \"tablea_0\".\"b\" as \"b\" from (select * from tableA as \"root\" inner \
         join tableB as \"tableb_0\" on (\"root\".ID = \"tableb_0\".ID)) as \"tablea_0\""
    );
    assert_eq!(d.rendered_alias("tdswrap__"), "tablea_0");
    // (Java's: an output name's quote is not doubled in the wrap's column)
    let odd = SqlSelect { outputs: vec![out("x\"y")], ..star.clone() };
    assert!(render(&d, query(odd)).starts_with("select \"tablea_0\".\"x\"y\" as \"x\"y\" from"));
    assert_eq!(
        d.select_list(query(star.clone())).unwrap().as_deref(),
        Some("\"tablea_0\".\"a\" as \"a\", \"tablea_0\".\"b\" as \"b\"")
    );
    assert_eq!(
        d.render_as_aliased(query(star.clone())).unwrap(),
        "select \"tdswrap__\".\"a\" as \"a\", \"tdswrap__\".\"b\" as \"b\" from (select * from tableA as \"t0\" inner \
         join tableB as \"t1\" on (\"t0\".ID = \"t1\".ID)) as \"tdswrap__\""
    );
    // a union's select list is its first branch's; a WITH has none
    let branch = |a: &str| query(select(vec![proj(col(a, "A"), "a")], table("T", a)));
    let union = SqlQuery::Union(crate::build::union(vec![branch("t0"), branch("t1")], true, Vec::new()).unwrap());
    assert_eq!(d.select_list(union.clone()).unwrap().as_deref(), Some("\"root\".A as \"a\""));
    assert_eq!(
        render(&d, union),
        "select \"root\".A as \"a\" from T as \"root\" union all select \"root\".A as \"a\" from T as \"root\""
    );
    let cte = lite_sql::Cte { name: "c".into(), query: branch("t0"), materialized: false };
    let with = SqlQuery::With(crate::build::with(vec![cte], branch("t1"), false).unwrap());
    assert_eq!(d.select_list(with.clone()).unwrap(), None);
    assert_eq!(
        render(&d, with),
        "with c as (select \"root\".A as \"a\" from T as \"root\") select \"root\".A as \"a\" from T as \"root\""
    );
}

// ---- expressions

#[test]
fn literals_and_references_are_the_engines() {
    let d = h2();
    assert_eq!(expr(&d, &float_lit(0.9)), "cast(0.9 as float)");
    assert_eq!(expr(&d, &decimal_lit(BigDecimal::parse("1.50").unwrap())), "cast(1.50 as Decimal(32,16))");
    assert_eq!(expr(&d, &bare("ID")), "ID");
    assert_eq!(expr(&d, &col("t0", "ID")), "\"t0\".ID");
    let quoted = SqlExpr::Column(Column {
        table: Some("t0".into()),
        name: "calendar name".into(),
        r#type: TypeFact::Unknown,
        origin: Some(Origin::PhysicalQuoted),
    });
    assert_eq!(expr(&d, &quoted), "\"t0\".\"calendar name\"");
    // quoteIdentifiers: every physical identifier quoted, a table's each part
    let q = EngineStyle::new(Profile::H2, true, None);
    assert_eq!(expr(&q, &col("t0", "ID")), "\"t0\".\"ID\"");
    assert_eq!(
        render(&q, query(select(vec![proj(bare("ID"), "id")], table("s.T", "t0")))),
        "select \"ID\" as \"id\" from \"s\".\"T\" as \"root\""
    );
    assert_eq!(expr(&d, &crate::build::star(Some("t0"))), "\"t0\".*");
    assert_eq!(expr(&d, &crate::build::star(None)), "*");
    assert_eq!(expr(&d, &crate::build::row_order(None)), "rowid");
    assert_eq!(expr(&d, &crate::build::row_order(Some("t0".into()))), "\"t0\".rowid");
    assert_eq!(d.date_lit("2005-10-10"), "DATE'2005-10-10'");
    assert_eq!(d.time_lit("10:00:00").unwrap_err().class, Refusal::DIALECT_CAPABILITY);
    assert_eq!(d.timestamp_lit("2019-04-08T18:00:00").unwrap(), "TIMESTAMP'2019-04-08 18:00:00'");
    // a connection's zone shifts a date-time constant from GMT (adjustDate); GMT itself shifts none
    assert_eq!(in_zone("EST").timestamp_lit("2019-04-08T18:00:00").unwrap(), "TIMESTAMP'2019-04-08 13:00:00'");
    assert_eq!(in_zone("GMT").timestamp_lit("2019-04-08T18:00:00").unwrap(), "TIMESTAMP'2019-04-08 18:00:00'");
    assert_eq!(in_zone("UTC").timestamp_lit("2019-04-08T18:00:00.000").unwrap(), "TIMESTAMP'2019-04-08 18:00.000'");
    assert_eq!(in_zone("Mars/Base").timestamp_lit("2019-04-08T18:00:00").unwrap(), "TIMESTAMP'2019-04-08 18:00:00'");
    assert_eq!(in_zone("EST").timestamp_lit("2019-04-08").unwrap(), "TIMESTAMP'2019-04-08'");
    // (Java's: only position 10's T is a space, and a text that is no date-time is left unshifted)
    assert_eq!(in_zone("EST").timestamp_lit("+10000-01-01T00:00:00").unwrap(), "TIMESTAMP'+10000-01-01T00:00:00'");
    assert_eq!(
        in_zone("America/New_York").timestamp_lit("2019-04-08T18:00:00").unwrap_err().class,
        Refusal::ILLEGAL_STATE
    );
    // a projected date constant is a plain string; a scalar-map column unaliased; a pre-quoted alias quoted once
    let q = query(select(
        vec![
            proj(SqlExpr::DateLit { iso: "2015-10-16".into(), r#type: TypeFact::Unknown }, "k_processingDate"),
            proj(bare("A"), "u_map__0"),
            proj(bare("B"), "\"firstName\""),
        ],
        SqlSource::Dual,
    ));
    assert_eq!(render(&d, q), "select '2015-10-16' as \"k_processingDate\", A, B as \"firstName\"");
}

#[test]
fn plan_parameters_are_the_engines_templates() {
    let d = h2();
    let kinds = [
        (ParamKind::Raw, "${p}"),
        (ParamKind::String, "'${p?replace(\"'\", \"''\")}'"),
        (ParamKind::Date, "TIMESTAMP'${p}'"),
        (ParamKind::StrictDate, "DATE'${p}'"),
        (ParamKind::Datetime, "TIMESTAMP'${p}'"),
        (ParamKind::Enum, "'${p}'"),
        (ParamKind::Float, "${p}"),
        (ParamKind::Boolean, "${p}"),
        (ParamKind::Other, "${p}"),
    ];
    for (kind, spelled) in kinds {
        assert_eq!(expr(&d, &param("p", kind, false)), spelled, "{kind}");
    }
    assert_eq!(expr(&in_zone("EST"), &param("p", ParamKind::Datetime, false)), "TIMESTAMP'${GMTtoTZ( \"[EST]\" p)}'");
    // an optional one: varPlaceHolderToString in every position
    assert_eq!(
        expr(&d, &param("p", ParamKind::String, true)),
        "${varPlaceHolderToString(p![] \"'\" \"'\" {\"'\" : \"''\"} \"null\")}"
    );
    assert_eq!(
        expr(&in_zone("US/Arizona"), &param("p", ParamKind::Datetime, true)),
        "${varPlaceHolderToString(GMTtoTZ( \"[US/Arizona]\" p![]) \"TIMESTAMP'\" \"'\" {} \"null\")}"
    );
    assert_eq!(
        expr(&d, &param("p", ParamKind::Boolean, true)),
        "${varPlaceHolderToString(p![] \"\" \"\" {} \"null\")}"
    );
    // DB2 quotes a boolean
    assert_eq!(
        expr(&db2(), &param("p", ParamKind::Boolean, true)),
        "${varPlaceHolderToString(p![] \"'\" \"'\" {} \"null\")}"
    );
    assert_eq!(
        refused(&d, &param("p", ParamKind::Raw, true)),
        Refusal::illegal_state("RAW plan params are bare splices, never collections")
    );
    // the collection template: each escape pair with a trailing space
    let splice = |d: &EngineStyle, kind| d.collection_splice(&PlanParam::of(&param("p", kind, false)).unwrap());
    assert_eq!(
        splice(&d, ParamKind::String).unwrap(),
        "${renderCollection(p![] \",\" \"'\" \"'\" {\"'\" : \"''\" } \"null\")}"
    );
    assert_eq!(
        splice(&d, ParamKind::Float).unwrap(),
        "${renderCollection(p![] \",\" \"CAST(\" \" AS FLOAT)\" {} \"null\")}"
    );
    assert_eq!(splice(&db2(), ParamKind::Boolean).unwrap(), "${renderCollection(p![] \",\" \"'\" \"'\" {} \"null\")}");
    assert_eq!(splice(&d, ParamKind::Raw).unwrap_err().class, Refusal::ILLEGAL_STATE);
    // a property read through a parameter: its dotted placeholder; through anything else refused
    let get = crate::build::struct_get(crate::build::struct_get(param("p", ParamKind::Other, false), "a"), "b");
    assert_eq!(expr(&d, &get), "'${p.a.b}'");
    let r = refused(&d, &crate::build::struct_get(bare("A"), "a"));
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::UNSUPPORTED_OPERATION, "plan: struct extraction has no engine-H2 spelling")
    );
}

#[test]
fn enum_and_optional_parameter_comparisons_are_the_engines_selectors() {
    let d = h2();
    let selector = "(${optionalVarPlaceHolderOperationSelector(e, equalEnumOperationSelector(f(e), '\"t0\".TYPE in \
                    (${f(e)})', '\"t0\".TYPE = ${f(e)}'), '0 = 1')})";
    assert_eq!(expr(&d, &eq(col("t0", "TYPE"), enum_param("e", false))), selector);
    // the parameter on either side; its negation a lowercase not
    assert_eq!(expr(&d, &eq(enum_param("e", false), col("t0", "TYPE"))), selector);
    assert_eq!(
        expr(&d, &call(SqlFn::Not, vec![eq(col("t0", "TYPE"), enum_param("e", false))])),
        format!("not {selector}")
    );
    // optional: the parameter's holder name
    assert!(
        expr(&d, &eq(col("t0", "TYPE"), enum_param("e", true)))
            .starts_with("(${optionalVarPlaceHolderOperationSelector(e![], equalEnumOperationSelector(f(e![]), ")
    );
    // a decode chain's raw source column is compared; a quote in the column's text escaped into the template
    let decode = case(vec![when(eq(col("t0", "C"), string_lit("A")), string_lit("Alpha"))], Some(null_lit()));
    assert!(expr(&d, &eq(decode, enum_param("e", false))).contains("'\"t0\".C in (${f(e)})'"));
    let quoted = call(SqlFn::Upper, vec![string_lit("it's")]);
    assert!(expr(&d, &eq(quoted, enum_param("e", false))).contains("'upper(\\'it\\'\\'s\\') = ${f(e)}'"));
    // a required parameter against an enum literal: plain equality
    assert_eq!(expr(&d, &eq(string_lit("NO"), enum_param("e", false))), "'NO' = '${e}'");
    // an optional parameter's equality: null-safe, H2's spelling and DB2's OR form
    let holder = "${varPlaceHolderToString(p![] \"TIMESTAMP'\" \"'\" {} \"null\")}";
    let e = eq(col("t0", "D"), param("p", ParamKind::Date, true));
    assert_eq!(expr(&d, &e), format!("\"t0\".D is not distinct from {holder}"));
    assert_eq!(expr(&db2(), &e), format!("(\"t0\".D = {holder} or (\"t0\".D is null and {holder} is null))"));
    let both = eq(param("p", ParamKind::Date, true), param("p", ParamKind::Date, true));
    assert_eq!(expr(&d, &both), format!("{holder} is not distinct from {holder}"));
    let left = eq(param("p", ParamKind::Date, true), col("t0", "D"));
    assert_eq!(expr(&d, &left), format!("{holder} is not distinct from \"t0\".D"));
}

#[test]
fn boolean_text_and_in_lists_are_the_engines() {
    let d = h2();
    let (a, b, c) = (bare("a"), bare("b"), bare("c"));
    let one = |x: &SqlExpr, v| eq(x.clone(), int_lit(v));
    // AND flat, an OR under it parenthesized; an AND under OR parenthesized
    let and = call(
        SqlFn::And,
        vec![one(&a, 1), call(SqlFn::Or, vec![one(&b, 2), one(&c, 3)]), call(SqlFn::And, vec![one(&a, 4), one(&b, 5)])],
    );
    assert_eq!(expr(&d, &and), "a = 1 and (b = 2 or c = 3) and a = 4 and b = 5");
    assert_eq!(
        expr(&d, &call(SqlFn::Or, vec![one(&a, 1), call(SqlFn::And, vec![one(&b, 2), one(&c, 3)])])),
        "a = 1 or (b = 2 and c = 3)"
    );
    assert_eq!(expr(&d, &call(SqlFn::NullSafeEqual, vec![a.clone(), b.clone()])), "a is not distinct from b");
    assert_eq!(expr(&d, &call(SqlFn::NullSafeNotEqual, vec![a.clone(), b.clone()])), "a is distinct from b");
    assert_eq!(
        expr(&db2(), &call(SqlFn::NullSafeEqual, vec![a.clone(), b.clone()])),
        "(a = b or (a is null and b is null))"
    );
    assert_eq!(
        expr(&composite(), &call(SqlFn::NullSafeNotEqual, vec![a.clone(), b.clone()])),
        "(not (a = b) or (a is null and b is not null) or (a is not null and b is null))"
    );
    assert_eq!(expr(&d, &call(SqlFn::IsNull, vec![a.clone()])), "a is null");
    assert_eq!(expr(&d, &call(SqlFn::IsNotNull, vec![a.clone()])), "a is not null");
    // the null-guarded in: every stacked COALESCE(x, false) unwrapped
    let guarded = call(SqlFn::In, vec![a.clone(), int_lit(1), int_lit(2)]);
    let guarded = call(
        SqlFn::Coalesce,
        vec![call(SqlFn::Coalesce, vec![guarded, crate::build::bool_lit(false)]), crate::build::bool_lit(false)],
    );
    assert_eq!(expr(&d, &guarded), "a in (1, 2)");
    // a singleton in-list is equality (Java's: any second argument, not only a literal)
    assert_eq!(expr(&d, &call(SqlFn::In, vec![a.clone(), int_lit(1)])), "a = 1");
    assert_eq!(expr(&d, &call(SqlFn::In, vec![a.clone(), b.clone()])), "a = b");
    let splice = SqlExpr::TempTableInSplice { temp_table_name: "tempTableForIn_x".into(), r#type: TypeFact::Unknown };
    assert_eq!(
        expr(&d, &call(SqlFn::In, vec![a.clone(), splice])),
        "a in (select \"temptableforin_x_0\".ColumnForStoringInCollection as ColumnForStoringInCollection from \
         tempTableForIn_x as \"temptableforin_x_0\")"
    );
    assert_eq!(expr(&d, &call(SqlFn::In, vec![a.clone(), param("p", ParamKind::Raw, false)])), "a in (${p})");
    assert_eq!(
        expr(&in_zone("EST"), &call(SqlFn::In, vec![a.clone(), param("p", ParamKind::Datetime, false)])),
        "a in (${renderCollectionWithTz(p![] \"[EST]\" \",\" \"TIMESTAMP'\" \"'\" \"null\")})"
    );
    assert_eq!(
        expr(&d, &call(SqlFn::In, vec![a.clone(), param("p", ParamKind::String, false)])),
        "a in (${renderCollection(p![] \",\" \"'\" \"'\" {\"'\" : \"''\" } \"null\")})"
    );
    // an arithmetic operation over a mixed-operator operand wraps itself; a same-operator chain stays flat
    let mixed = call(SqlFn::Plus, vec![call(SqlFn::Minus, vec![a.clone(), b.clone()]), c.clone()]);
    assert_eq!(expr(&d, &mixed), "(a - b + c)");
    assert_eq!(expr(&db2(), &mixed), "(a-b+c)");
    assert_eq!(
        expr(&d, &call(SqlFn::Plus, vec![call(SqlFn::Plus, vec![a.clone(), b.clone()]), c.clone()])),
        "a + b + c"
    );
    // membership: a literal collection's elements, any other the collection itself
    let m = crate::build::membership(
        a.clone(),
        cast(
            array(vec![string_lit("x"), string_lit("y")]),
            SqlType::Array { element: Box::new(SqlType::Scalar(Scalar::Varchar)) },
        ),
    );
    assert_eq!(expr(&d, &m), "a in ('x', 'y')");
    assert_eq!(expr(&d, &crate::build::membership(a.clone(), b.clone())), "a in (b)");
}

#[test]
fn date_differences_decodes_and_cases_fold_to_the_engines_text() {
    let d = h2();
    let (s, e) = (bare("s"), bare("e"));
    let elapsed = |unit: i64| {
        let minus =
            call(SqlFn::Minus, vec![call(SqlFn::EpochMs, vec![e.clone()]), call(SqlFn::EpochMs, vec![s.clone()])]);
        call(SqlFn::IntDivide, vec![minus, int_lit(unit)])
    };
    assert_eq!(expr(&d, &elapsed(3_600_000)), "datediff(hour, s, e)");
    assert_eq!(expr(&d, &elapsed(1_000)), "datediff(second, s, e)");
    // Java's (int) cast of the unit: its low 32 bits
    assert_eq!(expr(&d, &elapsed((1 << 32) + 60_000)), "datediff(minute, s, e)");
    assert!(!expr(&d, &elapsed(7)).starts_with("datediff"));
    // the Sunday-boundary week shape
    let sunday = |x: &SqlExpr| {
        let epoch = SqlExpr::DateLit { iso: "0001-01-07".into(), r#type: TypeFact::Unknown };
        call(
            SqlFn::IntDivide,
            vec![call(SqlFn::DateDiff, vec![date_part(DatePart::Day), epoch, x.clone()]), int_lit(7)],
        )
    };
    let diff = call(SqlFn::Minus, vec![sunday(&e), sunday(&s)]);
    let guard = call(
        SqlFn::LessEqual,
        vec![call(SqlFn::DateDiff, vec![date_part(DatePart::Day), e.clone(), s.clone()]), int_lit(0)],
    );
    let weeks = case(vec![when(guard.clone(), diff.clone())], Some(diff));
    assert_eq!(expr(&d, &weeks), "datediff(week, s, e)");
    // the same guard over any other branches is a CASE
    let other = case(vec![when(guard, int_lit(1))], Some(int_lit(2)));
    assert_eq!(expr(&d, &other), "case when datediff(day, e, s) <= 0 then 1 else 2 end");
    // the mapping's enum decode chain: flat, a multi-value branch an in-list
    let decode = case(
        vec![
            when(eq(col("t0", "C"), string_lit("A")), string_lit("Alpha")),
            when(
                call(SqlFn::Or, vec![eq(col("t0", "C"), string_lit("B")), eq(col("t0", "C"), string_lit("C"))]),
                string_lit("Beta"),
            ),
        ],
        Some(null_lit()),
    );
    assert_eq!(
        expr(&d, &decode),
        "case when \"t0\".C = 'A' then 'Alpha' when \"t0\".C in ('B', 'C') then 'Beta' else null end"
    );
}

#[test]
fn h2s_functions_are_the_engines_spellings() {
    let d = h2();
    let (a, b) = (bare("a"), bare("b"));
    let f = |func, args: Vec<SqlExpr>| expr(&d, &call(func, args));
    assert_eq!(f(SqlFn::Md5, vec![a.clone()]), "rawtohex(hash('MD5', a))");
    assert_eq!(f(SqlFn::Cbrt, vec![a.clone()]), "power(a, 1.0/3)");
    assert_eq!(f(SqlFn::Strpos, vec![a.clone(), b.clone()]), "locate(b, a)");
    assert_eq!(f(SqlFn::Ltrim, vec![a.clone()]), "regexp_replace(a, '^[ ]+', '')");
    assert_eq!(f(SqlFn::Rtrim, vec![a.clone()]), "regexp_replace(a, '[ ]+$', '')");
    assert_eq!(f(SqlFn::Lpad, vec![a.clone(), int_lit(5)]), "legend_h2_extension_lpad(a, 5, ' ')");
    assert_eq!(f(SqlFn::Rpad, vec![a.clone(), int_lit(5), string_lit("x")]), "legend_h2_extension_rpad(a, 5, 'x')");
    assert_eq!(f(SqlFn::Substring, vec![a.clone(), int_lit(1), int_lit(2)]), "substring(a, 1, 2)");
    assert_eq!(
        f(SqlFn::Concat, vec![a.clone(), call(SqlFn::Concat, vec![string_lit("_"), b.clone()])]),
        "concat(a, '_', b)"
    );
    assert_eq!(f(SqlFn::DateDiff, vec![date_part(DatePart::Day), a.clone(), b.clone()]), "datediff(day, a, b)");
    assert_eq!(f(SqlFn::AddInterval, vec![date_part(DatePart::Month), int_lit(1), a.clone()]), "dateadd(month, 1, a)");
    assert_eq!(
        try_expr(&d, &call(SqlFn::AddInterval, vec![date_part(DatePart::Quarter), int_lit(1), a.clone()])).unwrap_err(),
        Refusal::illegal_state("no H2 dateadd unit for an interval of QUARTER")
    );
    assert_eq!(
        try_expr(&d, &call(SqlFn::AddInterval, vec![date_part(DatePart::Nanosecond), int_lit(1), a.clone()]))
            .unwrap_err()
            .class,
        Refusal::DIALECT_CAPABILITY
    );
    assert_eq!(f(SqlFn::ParseInt, vec![a.clone()]), "cast(a as integer)");
    assert_eq!(f(SqlFn::BoolToText, vec![a.clone()]), "cast(a as varchar)");
    assert_eq!(f(SqlFn::Length, vec![a.clone()]), "char_length(a)");
    assert_eq!(f(SqlFn::Today, vec![]), "cast(now() as date)");
    assert_eq!(f(SqlFn::Trim, vec![a.clone()]), "trim(both from a)");
    assert_eq!(f(SqlFn::Trim, vec![a.clone(), b.clone()]), "trim(a, b)");
    assert_eq!(f(SqlFn::DateTruncDay, vec![a.clone()]), "cast(truncate(a) as date)");
    assert_eq!(
        f(SqlFn::Greater, vec![call(SqlFn::Strpos, vec![a.clone(), string_lit("it's")]), int_lit(0)]),
        "a like '%it''s%'"
    );
    assert_eq!(f(SqlFn::Greater, vec![a.clone(), b.clone()]), "a > b");
    assert_eq!(f(SqlFn::Extract, vec![date_part(DatePart::DayOfYear), a.clone()]), "extract(doy from a)");
    assert_eq!(f(SqlFn::Extract, vec![date_part(DatePart::Month), a.clone()]), "date_part('month', a)");
    assert_eq!(
        f(SqlFn::DateTrunc, vec![date_part(DatePart::Month), call(SqlFn::Today, vec![])]),
        "cast(cast(date_trunc('month', now()) as timestamp) as date)"
    );
    assert_eq!(f(SqlFn::DateTrunc, vec![date_part(DatePart::Hour), a.clone()]), "date_trunc('hour', a)");
    assert_eq!(
        f(SqlFn::Strftime, vec![a.clone(), format(vec![DateFmt::Part(DateFmtPart::MonthName)])]),
        "formatdatetime(a, 'MMMM')"
    );
    let r = try_expr(&d, &call(SqlFn::Strftime, vec![a.clone(), format(vec![DateFmt::Part(DateFmtPart::Year4)])]))
        .unwrap_err();
    assert!(r.message.starts_with("strftime format has no engine-H2 formatdatetime spelling yet: ["), "{}", r.message);
    let ymd = vec![
        DateFmt::Part(DateFmtPart::Year4),
        text("-"),
        DateFmt::Part(DateFmtPart::Month2),
        text("-"),
        DateFmt::Part(DateFmtPart::Day2),
    ];
    assert_eq!(
        f(SqlFn::Strptime, vec![a.clone(), format(ymd.clone())]),
        "parsedatetime(substring(a, 1, 10), 'yyyy-MM-dd')"
    );
    let mut ymdh = ymd;
    ymdh.extend([text(" "), DateFmt::Part(DateFmtPart::Hour2)]);
    assert_eq!(f(SqlFn::Strptime, vec![a.clone(), format(ymdh)]), "parsedatetime(a, 'yyyy-MM-dd HH')");
    let my = vec![DateFmt::Part(DateFmtPart::MonthAbbrev), DateFmt::Part(DateFmtPart::Year4)];
    assert_eq!(f(SqlFn::Strptime, vec![a.clone(), format(my)]), "parsedatetime(concat('01', a), 'ddMMMyyyy')");
    assert_eq!(f(SqlFn::Guid, vec![]), "CAST(uuid() AS VARCHAR)");
    assert_eq!(f(SqlFn::Round, vec![a.clone(), int_lit(2)]), "round(a, 2)");
    assert_eq!(
        f(SqlFn::PureSplitPart, vec![a.clone(), string_lit(","), int_lit(1)]),
        "legend_h2_extension_split_part(a, ',', 1)"
    );
    for refused_fn in [SqlFn::Translate, SqlFn::DateAtTime, SqlFn::EpochNs] {
        assert_eq!(
            try_expr(&d, &call(refused_fn, vec![a.clone(), b.clone()])).unwrap_err().class,
            Refusal::DIALECT_CAPABILITY
        );
    }
    assert_eq!(
        f(SqlFn::Coalesce, vec![call(SqlFn::ListLength, vec![param("p", ParamKind::String, false)]), int_lit(0)]),
        "(${collectionSize(p![])})"
    );
}

#[test]
fn db2s_and_composites_functions_are_their_goldens() {
    let (a, b) = (bare("a"), bare("b"));
    let f = |d: &EngineStyle, func, args: Vec<SqlExpr>| try_expr(d, &call(func, args));
    let db2 = db2();
    let ok = |func, args: Vec<SqlExpr>| f(&db2, func, args).unwrap_or_else(|r| panic!("{r}"));
    assert_eq!(
        ok(SqlFn::Concat, vec![a.clone(), call(SqlFn::Concat, vec![b.clone(), string_lit("x")])]),
        "(a concat b concat 'x')"
    );
    assert_eq!(ok(SqlFn::ConcatJoin, vec![a.clone(), b.clone()]), "(a concat b)");
    assert_eq!(ok(SqlFn::Trim, vec![a.clone()]), "trim(a)");
    assert_eq!(ok(SqlFn::Trim, vec![a.clone(), b.clone()]), "trim(a, b)");
    assert_eq!(ok(SqlFn::Cbrt, vec![a.clone()]), "cbrt(a)");
    assert_eq!(ok(SqlFn::Strpos, vec![a.clone(), b.clone()]), "position(b, a)");
    assert_eq!(ok(SqlFn::Substring, vec![a.clone(), int_lit(1), int_lit(2)]), "substr(a, 1, 2)");
    assert_eq!(ok(SqlFn::Ltrim, vec![a.clone()]), "ltrim(a)");
    assert_eq!(ok(SqlFn::Lpad, vec![a.clone(), int_lit(5)]), "lpad(a, 5)");
    assert_eq!(ok(SqlFn::ReverseString, vec![a.clone()]), "reverse(a)");
    assert_eq!(ok(SqlFn::Left, vec![a.clone(), int_lit(2)]), "trim(left(a, 2))");
    assert_eq!(ok(SqlFn::Right, vec![a.clone(), int_lit(2)]), "trim(right(a, 2))");
    assert_eq!(ok(SqlFn::Today, vec![]), "date(current date)");
    assert_eq!(ok(SqlFn::Length, vec![a.clone()]), "CHARACTER_LENGTH(a,CODEUNITS32)");
    assert_eq!(ok(SqlFn::Extract, vec![date_part(DatePart::DayOfYear), a.clone()]), "dayofyear(a)");
    assert_eq!(ok(SqlFn::Extract, vec![date_part(DatePart::Month), a.clone()]), "date_part('month', a)");
    assert_eq!(ok(SqlFn::DateTrunc, vec![date_part(DatePart::Year), a.clone()]), "date(1) + (year(a)-1) YEARS");
    assert_eq!(
        ok(SqlFn::DateTrunc, vec![date_part(DatePart::Month), call(SqlFn::Today, vec![])]),
        "date(1) + (year(current date)-1) YEARS + (month(current date)-1) MONTHS"
    );
    assert_eq!(
        ok(SqlFn::DateTrunc, vec![date_part(DatePart::Quarter), a.clone()]),
        "date(1) + ((year(a)-1) YEARS) + (3 * QUARTER(a) - 3) MONTHS"
    );
    let r = f(&db2, SqlFn::DateTrunc, vec![date_part(DatePart::Week), a.clone()]).unwrap_err();
    assert!(r.message.starts_with("date_trunc unit has no engine-DB2 spelling yet: ["), "{}", r.message);
    assert_eq!(ok(SqlFn::AddInterval, vec![date_part(DatePart::Month), int_lit(-1), a.clone()]), "a - 1 MONTHS");
    // a week is seven days; a long's negation wraps as Java's
    assert_eq!(ok(SqlFn::AddInterval, vec![date_part(DatePart::Week), int_lit(2), a.clone()]), "a + 14 DAYS");
    assert_eq!(
        ok(SqlFn::AddInterval, vec![date_part(DatePart::Day), int_lit(i64::MIN), a.clone()]),
        "a - -9223372036854775808 DAYS"
    );
    let weeks = 1i64 << 61;
    assert_eq!(
        ok(SqlFn::AddInterval, vec![date_part(DatePart::Week), int_lit(weeks), a.clone()]),
        format!("a - {} DAYS", weeks.wrapping_mul(7).wrapping_neg())
    );
    for shape in [
        vec![date_part(DatePart::Millisecond), int_lit(1), a.clone()],
        vec![date_part(DatePart::Month), b.clone(), a.clone()],
    ] {
        let r = f(&db2, SqlFn::AddInterval, shape).unwrap_err();
        assert!(r.message.starts_with("interval shape has no engine-DB2 spelling yet: ["), "{}", r.message);
    }
    let ym = vec![DateFmt::Part(DateFmtPart::Year4), text("-"), DateFmt::Part(DateFmtPart::Month2)];
    assert_eq!(ok(SqlFn::Strptime, vec![a.clone(), format(ym.clone())]), "to_date(a,'yyyy-MM')");
    let mut ymh = ym;
    ymh.extend([text(" "), DateFmt::Part(DateFmtPart::Hour2), DateFmt::Part(DateFmtPart::SubsecMin)]);
    assert_eq!(ok(SqlFn::Strptime, vec![a.clone(), format(ymh)]), "timestamp_format(a,'yyyy-MM hhmmm')");
    // a date-typed parse or interval renders bare under DB2's normalizing cast; a string cast carries its width
    let parse = call(SqlFn::Strptime, vec![a.clone(), format(vec![DateFmt::Part(DateFmtPart::Year4)])]);
    assert_eq!(expr(&db2, &cast(parse, SqlType::Scalar(Scalar::Date))), "to_date(a,'yyyy')");
    assert_eq!(expr(&db2, &cast(a.clone(), SqlType::Scalar(Scalar::Varchar))), "cast(a as varchar(16000))");
    assert_eq!(expr(&db2, &cast(a.clone(), SqlType::Scalar(Scalar::Date))), "cast(a as date)");
    // Composite: plain char_length and the full substring; DB2's every other form
    let c = composite();
    assert_eq!(f(&c, SqlFn::Length, vec![a.clone()]).unwrap(), "char_length(a)");
    assert_eq!(f(&c, SqlFn::Substring, vec![a.clone(), int_lit(1)]).unwrap(), "substring(a, 1)");
    assert_eq!(f(&c, SqlFn::Concat, vec![a.clone(), b.clone()]).unwrap(), "(a concat b)");
    assert_eq!(f(&c, SqlFn::Left, vec![a.clone(), int_lit(2)]).unwrap(), "trim(left(a, 2))");
    assert_eq!(f(&c, SqlFn::Today, vec![]).unwrap(), "date(current date)");
}

#[test]
fn list_calls_are_the_engines_variadic_and_infix_forms() {
    let d = h2();
    let (a, b) = (bare("a"), bare("b"));
    let typed_one = cast(int_lit(1), SqlType::Scalar(Scalar::Double));
    let f = |func, args: Vec<SqlExpr>| try_expr(&d, &call(func, args));
    assert_eq!(
        f(SqlFn::ListMax, vec![array(vec![typed_one.clone(), a.clone(), int_lit(3)])]).unwrap(),
        "greatest(1, a, 3)"
    );
    assert_eq!(f(SqlFn::ListMin, vec![null_lit()]).unwrap(), "least(null)");
    let empty = cast(null_lit(), SqlType::Array { element: Box::new(SqlType::Scalar(Scalar::Varchar)) });
    assert_eq!(f(SqlFn::ListMax, vec![empty]).unwrap(), "greatest(null)");
    // the mixed-identity selection: the variadic form over the comparables
    let vals = array(vec![a.clone(), b.clone()]);
    let pos = call(SqlFn::ListPosition, vec![vals.clone(), call(SqlFn::ListMin, vec![vals])]);
    assert_eq!(f(SqlFn::ListGet, vec![array(vec![string_lit("x"), string_lit("y")]), pos]).unwrap(), "least(a, b)");
    // a reduction over a literal list: its infix chain, an arithmetic one parenthesized
    assert_eq!(f(SqlFn::ListSum, vec![array(vec![typed_one, int_lit(2), int_lit(3)])]).unwrap(), "(1 + 2 + 3)");
    assert_eq!(f(SqlFn::ListBoolAnd, vec![array(vec![a.clone(), b.clone()])]).unwrap(), "a and b");
    assert_eq!(f(SqlFn::ListProduct, vec![array(vec![a.clone()])]).unwrap(), "a");
    // firstNotNull: coalesce over the list
    let pred = crate::build::lambda(&["x"], call(SqlFn::IsNotNull, vec![crate::build::derived(None, "x")]));
    let filtered = call(SqlFn::ListFilter, vec![array(vec![a.clone(), b.clone()]), pred]);
    assert_eq!(f(SqlFn::ListGet, vec![filtered, int_lit(1)]).unwrap(), "coalesce(a, b)");
    assert_eq!(
        f(SqlFn::ListSort, vec![a.clone()]).unwrap_err(),
        Refusal::capability("LIST_SORT reached a dialect without a list encoding")
    );
    // joinStrings over a literal list: one flat concat, the separators last, each element's text coercion dropped
    let element = call(SqlFn::Coalesce, vec![cast(a.clone(), SqlType::Scalar(Scalar::Varchar)), string_lit("TDSNull")]);
    let rc = |sep: &str| {
        crate::build::reduce_collection(
            AggFn::StringAgg,
            array(vec![element.clone(), b.clone()]),
            vec![string_lit(sep)],
        )
    };
    let joined = |sep: &str| call(SqlFn::Coalesce, vec![rc(sep), string_lit("")]);
    assert_eq!(expr(&d, &joined(",")), "concat(a, b, ',')");
    assert_eq!(expr(&d, &joined("")), "concat(a, b, '', '', '')");
    let wrapped = call(SqlFn::Concat, vec![call(SqlFn::Concat, vec![string_lit("<"), joined(",")]), string_lit(">")]);
    assert_eq!(expr(&d, &wrapped), "concat(a, b, '<', ',', '>')");
    // (Java's: the COALESCE's default is not checked to be '', so another default is dropped)
    assert_eq!(expr(&d, &call(SqlFn::Coalesce, vec![rc(","), string_lit("x")])), "concat(a, b, ',')");
}

#[test]
fn aggregates_windows_and_sort_keys_are_lowercase() {
    let d = h2();
    let (a, b) = (bare("a"), bare("b"));
    assert_eq!(expr(&d, &reducer(AggFn::Sum, vec![a.clone()], false, vec![])), "sum(a)");
    assert_eq!(expr(&d, &reducer(AggFn::Count, vec![], false, vec![])), "count(*)");
    assert_eq!(expr(&d, &reducer(AggFn::Count, vec![a.clone()], true, vec![])), "count(distinct a)");
    assert_eq!(expr(&d, &reducer(AggFn::AnyValue, vec![a.clone()], false, vec![])), "a");
    assert_eq!(
        expr(&d, &reducer(AggFn::StringAgg, vec![a.clone(), string_lit(",")], false, vec![])),
        "listagg(a, ',')"
    );
    let ordered = reducer(AggFn::StringAgg, vec![a.clone(), string_lit(",")], true, vec![key(b.clone(), false)]);
    assert_eq!(expr(&d, &ordered), "listagg(distinct a, ',') within group (order by b desc)");
    assert_eq!(
        expr(&d, &reducer(AggFn::QuantileCont, vec![a.clone(), b.clone()], false, vec![key(a.clone(), false)])),
        "percentile_cont(b) within group (order by a desc)"
    );
    assert_eq!(
        expr(&d, &reducer(AggFn::QuantileDisc, vec![a.clone(), b.clone()], true, vec![])),
        "quantile_disc(distinct a, b)"
    );
    assert_eq!(
        refused(&d, &reducer(AggFn::Wavg, vec![a.clone()], false, vec![])),
        Refusal::illegal_state("lowering marker WAVG reached the renderer")
    );
    assert_eq!(
        refused(&d, &reducer(AggFn::RowNumber, vec![], false, vec![])),
        Refusal::illegal_state("ROW_NUMBER is a window function: windowCall writes it")
    );
    // an ordered string aggregate over a window: its WITHIN GROUP after the window
    let agg =
        crate::build::reducer(AggFn::StringAgg, vec![a.clone(), string_lit(",")], false, vec![key(b.clone(), true)]);
    let window = crate::build::window_call(SqlAgg::Reducer(agg), vec![a.clone()], vec![key(b.clone(), true)], None);
    // (the window's key is a sort key, a table-less column quoted; the aggregate's own ordering is not)
    assert_eq!(
        expr(&d, &window),
        "listagg(a, ',') over (partition by a order by \"b\" asc) within group (order by b asc)"
    );
    let rank = crate::build::window_call(
        SqlAgg::RankingFn { r#fn: AggFn::RowNumber, args: vec![] },
        vec![],
        vec![key(col("t0", "b"), false)],
        None,
    );
    assert_eq!(expr(&d, &rank), "row_number() over (order by \"t0\".b desc)");
    // sort keys: an output name quoted, a table-less column quoted, the direction explicit and no null placement
    let keys = [
        crate::build::sort_key(col("t0", "A"), true, Some(lite_sql::NullOrder::NullsFirst), Some("na\"me".into())),
        key(bare("A"), false),
        key(col("t0", "A"), true),
    ];
    let mut w = SqlWriter::new();
    w.join(&keys, ", ", |w, k| d.sort_key(w, k)).unwrap();
    assert_eq!(w.into_text().unwrap(), "\"name\" asc, \"A\" desc, \"t0\".A asc");
}

#[test]
fn a_select_writes_its_clauses_as_the_engine_does() {
    let d = h2();
    let base = select(vec![proj(col("t0", "A"), "a")], table("T", "t0"));
    let capped = SqlSelect { limit: Some(5), ..base.clone() };
    assert_eq!(render(&d, query(capped)), "select top 5 \"root\".A as \"a\" from T as \"root\"");
    let sliced = SqlSelect { limit: Some(5), offset: Some(10), ..base.clone() };
    assert_eq!(
        render(&d, query(sliced)),
        "select \"root\".A as \"a\" from T as \"root\" offset 10 rows fetch next 5 rows only"
    );
    // group keys: a renaming alias's quoted output name, a self-alias's physical expression, a table-less key quoted
    let grouped = SqlSelect {
        projections: vec![proj(col("t0", "A"), "a"), proj(col("t0", "B"), "B")],
        group_by: vec![col("t0", "A"), col("t0", "B"), bare("prodName")],
        ..base.clone()
    };
    let grouped = crate::build::select(grouped).unwrap();
    assert_eq!(
        render(&d, query(grouped.clone())),
        "select \"root\".A as \"a\", \"root\".B as \"B\" from T as \"root\" group by \"a\", \"root\".B, \"prodName\""
    );
    // DB2: every key its expression, a bare comma between
    assert_eq!(
        render(&db2(), query(grouped)),
        "select \"root\".A as \"a\", \"root\".B as \"B\" from T as \"root\" group by \"root\".A,\"root\".B,prodName"
    );
    // DB2 wraps a conjunction one of whose conjuncts is OR-rooted
    let wh = call(
        SqlFn::And,
        vec![call(SqlFn::Or, vec![eq(bare("x"), int_lit(1)), eq(bare("y"), int_lit(2))]), eq(bare("z"), int_lit(3))],
    );
    let filtered = SqlSelect { r#where: Some(wh), ..base.clone() };
    assert_eq!(
        render(&db2(), query(filtered.clone())),
        "select \"root\".A as \"a\" from T as \"root\" where ((x = 1 or y = 2) and z = 3)"
    );
    assert_eq!(
        render(&d, query(filtered)),
        "select \"root\".A as \"a\" from T as \"root\" where (x = 1 or y = 2) and z = 3"
    );
    let qualified = SqlSelect { qualify: Some(crate::build::bool_lit(true)), ..base.clone() };
    assert_eq!(
        SqlDialect::render(&d, query(qualified)).unwrap_err(),
        Refusal::illegal_state("QUALIFY has no engine-H2 golden spelling")
    );
    // the legacy plan binds no parameter: a statement is refused
    assert_eq!(SqlDialect::render_statement(&d, query(base)).unwrap_err().class, Refusal::DIALECT_CAPABILITY);
    // the passes: H2's carrier strategies (no literal reduction folded) and the lateral explode
    let names: Vec<&str> = d.passes().iter().map(|p| p.name()).collect();
    assert_eq!(names, ["CarrierStrategies", "LateralExplodeToUnion"]);
}

#[test]
fn casts_are_spelled_per_dynafunction() {
    let d = h2();
    let a = bare("a");
    let c = |t: SqlType| expr(&d, &cast(a.clone(), t));
    assert_eq!(c(SqlType::Decimal { precision: 38, scale: 18 }), "cast(a as decimal)");
    assert_eq!(c(SqlType::Decimal { precision: 10, scale: 2 }), "cast(a as decimal(10, 2))");
    assert_eq!(c(SqlType::Scalar(Scalar::Double)), "cast(a as float)");
    assert_eq!(c(SqlType::Scalar(Scalar::Varchar)), "cast(a as varchar)");
    assert_eq!(c(SqlType::Scalar(Scalar::Hugeint)), "cast(a as hugeint)");
    assert_eq!(c(SqlType::Scalar(Scalar::TemporalText)), "a");
    assert_eq!(expr(&d, &cast_conform(a.clone(), SqlType::Scalar(Scalar::Bigint), true)), "a");
    assert_eq!(refused(&d, &cast(a.clone(), SqlType::Scalar(Scalar::Time))).class, Refusal::DIALECT_CAPABILITY);
    assert_eq!(
        refused(&d, &cast(a.clone(), SqlType::Scalar(Scalar::Json))),
        Refusal::illegal_state("JSON cast reached a dialect without JSON support")
    );
    assert_eq!(
        refused(&d, &cast(a.clone(), SqlType::Scalar(Scalar::DecimalText))),
        Refusal::illegal_state("DECIMAL_TEXT cast reached a dialect without DECIMAL_TEXT support")
    );
    assert_eq!(
        refused(&d, &cast(a, SqlType::Struct { fields: vec![] })),
        Refusal::capability("a STRUCT type reached a dialect without struct support")
    );
    // checked narrowing and compaction: the inner value verbatim
    assert_eq!(expr(&d, &crate::build::checked_one(bare("l"), false, false)), "l");
    assert_eq!(expr(&d, &crate::build::compact_list(bare("l"))), "l");
}

// ---- DDL and DML

#[test]
fn ddl_and_seed_rows_are_the_engines_text() {
    let d = h2();
    let column = |name: &str, quoted: bool, t: ColumnType, pk: bool| lite_sql::DdlColumn {
        name: name.into(),
        declared_quoted: quoted,
        r#type: t,
        not_null: false,
        primary_key: pk,
    };
    let create = SqlDdl::CreateTable {
        schema: Some("S".into()),
        table: "T".into(),
        quoting: NameQuoting { schema: true, table: true },
        columns: vec![
            column("ID", false, ColumnType::Plain { kind: ColumnKind::Integer }, true),
            column("order", false, ColumnType::Sized { kind: "VARCHAR".into(), size: 20 }, false),
            column("calendar name", true, ColumnType::Plain { kind: ColumnKind::Date }, true),
            column("my col", false, ColumnType::Scaled { kind: "DECIMAL".into(), precision: 10, scale: 2 }, false),
            column("\"x\"", false, ColumnType::Plain { kind: ColumnKind::Other }, false),
        ],
        temporary: false,
    };
    assert_eq!(
        SqlDialect::render_ddl(&d, &create).unwrap(),
        "Create Table S.T(ID INT NOT NULL,\"order\" VARCHAR(20) NULL,\"calendar name\" DATE NOT NULL,\"my col\" \
         DECIMAL(10, 2) NULL,\"x\" OTHER NULL, PRIMARY KEY(ID,\"calendar name\"));"
    );
    assert_eq!(
        ddl_engine_text(&ColumnType::Plain { kind: ColumnKind::Array }).unwrap_err(),
        Refusal::illegal_state("no DDL spelling for declared column type ARRAY")
    );
    // a stored name as the store wrote it, its declared quoting not the engine text's
    assert_eq!(SqlDialect::stored_name(&d, "Orders", true), "Orders");
    assert_eq!(SqlDialect::physical_name(&d, "my table"), "\"my table\"");
    assert_eq!(
        SqlDialect::render_ddl(&d, &SqlDdl::CreateSchema { schema: "Sales".into(), quoted: true }).unwrap(),
        "Create Schema if not exists Sales;"
    );
    // an insert's column: a declared-quoted one quoted, a name that is no plain identifier quoted (its quotes dropped)
    let names: Vec<String> = [("x", true), ("a b", false), ("a\"b", false), ("order", false), ("ID", false)]
        .iter()
        .map(|(n, q)| SqlDialect::insert_column_name(&d, n, *q))
        .collect();
    assert_eq!(names, ["\"x\"", "\"a b\"", "\"ab\"", "\"order\"", "ID"]);
    // a seed row: one row a statement, each cell as convertValuesToCsvH2 writes it
    let insert = |rows: Vec<Vec<SqlExpr>>| SqlDml::InsertValues {
        schema: Some("S".into()),
        table: "T".into(),
        quoting: NameQuoting::default(),
        columns: vec!["ID".into(), "name".into(), "order".into()],
        quoted_columns: vec!["name".into()],
        rows,
    };
    let row = vec![decimal_lit(BigDecimal::parse("1E+3").unwrap()), string_lit("it's"), null_lit()];
    assert_eq!(
        SqlDialect::render_dml(&d, &insert(vec![row.clone()])).unwrap(),
        "insert into S.T (ID,\"name\",\"order\") values (1000,'it''s',null);"
    );
    assert_eq!(
        SqlDialect::render_dml_statement(&d, &insert(vec![row.clone()])).unwrap().sql,
        "insert into S.T (ID,\"name\",\"order\") values (1000,'it''s',null);"
    );
    assert_eq!(
        SqlDialect::render_dml(&d, &insert(vec![row.clone(), row])).unwrap_err(),
        Refusal::capability(
            "the engine's seed text writes one row a statement, a cell a column: 2 rows of 3 columns reached it"
        )
    );
    assert_eq!(
        SqlDialect::render_dml(&d, &insert(vec![vec![int_lit(1), string_lit("a"), crate::build::bool_lit(true)]]))
            .unwrap_err(),
        Refusal::capability("a IntLit cell reached the engine's seed text, which writes values only")
    );
    let all = SqlDml::DeleteAll { schema: None, table: "T".into(), quoting: NameQuoting::default() };
    assert_eq!(SqlDialect::render_dml(&d, &all).unwrap(), "DELETE FROM T;");
    // the interface's defaults name the dialect, each its own simple name
    let r = SqlDialect::keys_snapshot(&composite()).unwrap_err();
    assert_eq!(r.message, "a dynamic pivot's probe plan reached EngineStyleComposite, which says no snapshot for it");
    assert_eq!(SqlDialect::from_engine_text(&db2(), "select 1").unwrap(), "select 1");
    assert_eq!(
        d.in_temp_table_select("SESSION.tempTableForIn_x"),
        "select \"session.temptableforin_x_0\".ColumnForStoringInCollection as ColumnForStoringInCollection from SESSION.tempTableForIn_x as \"session.temptableforin_x_0\""
    );
}
