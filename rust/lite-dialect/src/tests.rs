// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The framework's pieces held to Java's behaviour: the writer and its regions, the records' `toString`, the
//! refusals' wording, the Java number and text spellings, and a handful of hand-built statements rendered by DuckDB.
//! The golden dump holds the rest (`tests/dialect_differential.rs`).

use crate::build::{call, int_lit, string_lit};
use crate::java::{big_decimal_plain, plain_float, split, strip};
use crate::raw_sql::translated;
use crate::statement::{Bind, Binding, Digits, Region, RenderedStatement, TypeSpelling, list_text};
use crate::{AnsiSqlRenderer, DuckDb, Refusal, SqlDialect, SqlWriter, ansi};
use lite_sql::{
    Column, ColumnKind, ColumnType, DdlColumn, JoinKind, NameQuoting, Origin, OutputCol, ParamKind, Projection, Scalar,
    SortKey, SqlDdl, SqlDml, SqlExpr, SqlFn, SqlQuery, SqlSelect, SqlSource, SqlType, TypeFact, ValueKind, ValueTyping,
};

// ---- builders

fn typed(s: Scalar) -> TypeFact {
    TypeFact::Typed { r#type: SqlType::Scalar(s), nullable: true }
}

fn col(table: &str, name: &str, origin: Origin) -> SqlExpr {
    SqlExpr::Column(Column {
        table: Some(table.to_owned()),
        name: name.to_owned(),
        r#type: typed(Scalar::Bigint),
        origin: Some(origin),
    })
}

fn table(name: &str, alias: &str) -> SqlSource {
    SqlSource::Table {
        name: name.to_owned(),
        alias: alias.to_owned(),
        outputs: Vec::new(),
        call: false,
        stored_types: Vec::new(),
        quoting: NameQuoting::default(),
    }
}

fn select(projections: Vec<Projection>, from: SqlSource) -> SqlSelect {
    SqlSelect {
        projections,
        distinct: false,
        from,
        r#where: None,
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by: Vec::new(),
        limit: None,
        offset: None,
        outputs: Vec::new(),
    }
}

fn proj(expr: SqlExpr, alias: Option<&str>) -> Projection {
    Projection { expr, alias: alias.map(str::to_owned), out: None }
}

fn query(s: SqlSelect) -> SqlQuery {
    SqlQuery::Select(Box::new(s))
}

fn param(name: &str, r#type: TypeFact, by_value: Option<ValueTyping>) -> SqlExpr {
    SqlExpr::PlanParam {
        name: name.to_owned(),
        kind: ParamKind::Other,
        optional: false,
        enum_map_fn: None,
        r#type,
        by_value,
    }
}

fn render(e: SqlExpr) -> String {
    let d = DuckDb::new();
    let mut w = SqlWriter::new();
    d.expr(&mut w, &e, 0).unwrap();
    w.into_text().unwrap()
}

// ---- the writer, its binds and regions

#[test]
fn a_bind_is_recorded_where_its_placeholder_is_written() {
    let mut w = SqlWriter::new();
    w.append("a = ")
        .bind(Bind { parameter: "x".into(), binding: Binding::One { null_type: "BIGINT".into(), hole: None } });
    w.append(" AND b = ")
        .bind(Bind { parameter: "y".into(), binding: Binding::Array { element_sql_type: "VARCHAR".into() } });
    assert_eq!(w.bound(), 2);
    let s = w.into_statement().unwrap();
    assert_eq!(s.sql, "a = ? AND b = ?");
    assert_eq!(
        list_text(&s.binds),
        "[Bind[parameter=x, binding=One[nullType=BIGINT, hole=null]], Bind[parameter=y, binding=Array[elementSqlType=VARCHAR]]]"
    );
}

#[test]
fn a_statement_that_binds_is_refused_as_text_in_javas_words() {
    let mut w = SqlWriter::new();
    w.append("x > ")
        .bind(Bind { parameter: "age".into(), binding: Binding::One { null_type: "BIGINT".into(), hole: None } });
    let refusal = w.into_text().unwrap_err();
    assert_eq!(refusal.class, "java.lang.IllegalStateException");
    assert_eq!(
        refusal.message,
        "a statement with bound parameters [Bind[parameter=age, binding=One[nullType=BIGINT, hole=null]]] was asked for \
         as text: render it as a statement"
    );
}

#[test]
fn a_region_records_utf16_positions_and_its_key_cells() {
    let mut w = SqlWriter::new();
    // 'é' is one UTF-16 unit and two bytes; '𝄞' two units and four bytes
    w.append("SELECT 'é𝄞' AS a");
    w.region(", ", ", ", Some(""), |w| {
        w.append("x").counter()?.append("y = ").bind_key(0)?;
        Ok(())
    })
    .unwrap();
    let s = w.into_statement().unwrap();
    // "SELECT 'é𝄞' AS a" is 17 UTF-16 units
    assert_eq!(s.regions[0].start, 17);
    assert_eq!(s.regions[0].counters, vec![18]);
    assert_eq!(s.regions[0].end, 23);
    assert_eq!(
        list_text(&s.regions),
        "[Region[start=17, end=23, lead=, , separator=, , empty=, firstBind=0, bindCount=1, counters=[18]]]"
    );
    assert_eq!(list_text(&s.binds), "[Bind[parameter=, binding=Key[cell=0]]]");
}

#[test]
fn a_region_is_refused_where_java_refuses_it() {
    let mut w = SqlWriter::new();
    assert_eq!(w.counter().unwrap_err().message, "a per-key name outside a per-key region: no key to name it by");
    assert_eq!(w.bind_key(0).unwrap_err().message, "a key's cell outside a per-key region: no key to bind");
    let nested = w.region("", ", ", None, |w| w.region("", ", ", None, |_| Ok(())).map(|_| ())).unwrap_err();
    assert_eq!(nested.message, "a per-key region inside another per-key region");
    let mut w = SqlWriter::new();
    w.region("", ", ", None, |w| w.append("x").counter().map(|_| ())).unwrap();
    assert_eq!(
        w.into_text().unwrap_err().message,
        "a statement with per-key regions was asked for as text: render it as a statement, which the runner writes \
         once per key"
    );
}

#[test]
fn a_key_cell_outside_every_region_is_refused_by_the_statement() {
    let binds = vec![Bind { parameter: String::new(), binding: Binding::Key { cell: 0 } }];
    let refusal = RenderedStatement::new("?".into(), binds, Vec::new()).unwrap_err();
    assert_eq!(refusal.class, "java.lang.IllegalArgumentException");
    assert_eq!(refusal.message, "placeholder 1 binds a key's cell outside a per-key region: no key to bind");
    assert_eq!(Binding::key(-1).unwrap_err().message, "a key's cell -1");
    let r = Region::new(5, 3, String::new(), String::new(), None, 0, 0, Vec::new()).unwrap_err();
    assert_eq!(r.message, "a per-key region [5, 3)");
}

#[test]
fn a_type_hole_follows_its_placeholder() {
    let mut w = SqlWriter::new();
    let refused = w.type_hole(vec![(ValueKind::Date, TypeSpelling::new("DATE", Digits::None))], ValueKind::Date);
    assert_eq!(refused.unwrap_err().message, "a type hole follows a placeholder of one value with none: []");
    w.append("CAST(")
        .bind(Bind { parameter: "d".into(), binding: Binding::One { null_type: "DATE".into(), hole: None } });
    w.append(" AS ")
        .type_hole(
            vec![
                (ValueKind::DateTime, TypeSpelling::new("TIMESTAMP", Digits::None)),
                (ValueKind::Date, TypeSpelling::new("DATE", Digits::None)),
            ],
            ValueKind::Date,
        )
        .unwrap()
        .append(")");
    let s = w.into_statement().unwrap();
    // the kinds in their declared order, as Java's EnumMap holds them
    assert_eq!(
        list_text(&s.binds),
        "[Bind[parameter=d, binding=One[nullType=DATE, hole=TypeHole[at=10, types={DATE=TypeSpelling[name=DATE, \
         digits=NONE, fraction=null], DATE_TIME=TypeSpelling[name=TIMESTAMP, digits=NONE, fraction=null]}, \
         absent=DATE]]]]"
    );
}

// ---- refusals

#[test]
fn a_refusal_carries_the_java_class_and_message() {
    let r = Refusal::capability("pivot reached a dialect without a PIVOT strategy");
    assert_eq!(r.class, "com.legend.sql.dialect.DialectCapability");
    assert!(r.is_capability());
    assert_eq!(
        r.to_string(),
        "com.legend.sql.dialect.DialectCapability: pivot reached a dialect without a PIVOT strategy"
    );
    assert!(!Refusal::illegal_state("x").is_capability());
    assert_eq!(crate::refusal::arg(&[], 1).unwrap_err().message, "Index 1 out of bounds for length 0");
}

#[test]
fn the_base_refuses_what_duckdb_does_not_spell() {
    let fold = SqlExpr::FoldCall {
        source: Box::new(int_lit(1)),
        lambda: lite_sql::Lambda { params: vec!["x".into()], body: Box::new(int_lit(1)), r#type: TypeFact::Unknown },
        init: Box::new(int_lit(0)),
        acc_is_list: false,
        homogeneous: true,
        r#type: TypeFact::Unknown,
    };
    let d = DuckDb::new();
    let mut w = SqlWriter::new();
    let r = d.expr(&mut w, &fold, 0).unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::DIALECT_CAPABILITY, "fold reached a dialect without a fold encoding")
    );
    let tagged = SqlType::Tagged { slots: Vec::new() };
    assert!(ansi::cast_type_name(&d, &tagged).unwrap_err().message.starts_with("a tagged value is never cast"));
    let r = d.typeof_name(Scalar::Varchar).unwrap_err();
    assert_eq!(r.message, "a TYPEOF compared with VARCHAR's name reached DuckDB");
    let r = d.scalar_type_name(Scalar::DecimalText).unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::ILLEGAL_STATE, "DECIMAL_TEXT cast reached a dialect without DECIMAL_TEXT support")
    );
}

// ---- Java's spellings

#[test]
fn a_big_decimal_is_written_plain() {
    assert_eq!(big_decimal_plain("1E+3"), "1000");
    assert_eq!(big_decimal_plain("1.5E-7"), "0.00000015");
    assert_eq!(big_decimal_plain("-1.23E+2"), "-123");
    assert_eq!(big_decimal_plain("123.4500"), "123.4500");
    assert_eq!(big_decimal_plain("-0.0"), "0.0");
    assert_eq!(big_decimal_plain("0E+3"), "0");
    assert_eq!(big_decimal_plain("0E-3"), "0.000");
    assert_eq!(big_decimal_plain("12345678901234567890123456789"), "12345678901234567890123456789");
}

#[test]
fn a_float_literal_is_the_numeric_charters_plain_spelling() {
    assert_eq!(plain_float(1.0).unwrap(), "1.0");
    assert_eq!(plain_float(-0.0).unwrap(), "0.0");
    assert_eq!(plain_float(123.456).unwrap(), "123.456");
    assert_eq!(plain_float(1e-6).unwrap(), "0.0000010");
    assert_eq!(plain_float(1e7).unwrap(), "10000000.0");
    assert_eq!(plain_float(1e14).unwrap(), "100000000000000.0");
    // the extreme magnitudes keep Double.toString's exponent form
    assert_eq!(plain_float(1e15).unwrap(), "1.0E15");
    assert_eq!(plain_float(1e38).unwrap(), "1.0E38");
    assert_eq!(plain_float(1e-17).unwrap(), "1.0E-17");
    assert_eq!(plain_float(9.007199254740992e15).unwrap(), "9.007199254740992E15");
    assert_eq!(plain_float(f64::INFINITY).unwrap(), "Infinity");
    assert_eq!(plain_float(f64::NAN).unwrap_err().class, Refusal::NUMBER_FORMAT);
}

#[test]
fn a_decimal_unit_is_bigdecimals_text() {
    assert_eq!(ansi::decimal_unit(0), ("1".to_owned(), "0.5".to_owned(), "2".to_owned()));
    assert_eq!(ansi::decimal_unit(3), ("0.001".to_owned(), "0.0005".to_owned(), "0.002".to_owned()));
}

#[test]
fn javas_split_and_strip() {
    assert_eq!(split("a,b,,", ','), vec!["a", "b"]);
    assert_eq!(split("", ','), vec![""]);
    assert_eq!(split(",", ','), Vec::<&str>::new());
    assert_eq!(split(",a", ','), vec!["", "a"]);
    // Java's isWhitespace takes the separators U+001C..U+001F but not the no-break space
    assert_eq!(strip("\u{1F} a\u{A0}"), "a\u{A0}");
}

#[test]
fn identifiers_quote_only_when_necessary() {
    let d = DuckDb::new();
    assert_eq!(d.ident("NAME"), "NAME");
    assert_eq!(d.ident("select"), "\"select\"");
    assert_eq!(d.ident("Order"), "\"Order\"");
    assert_eq!(d.ident("first name"), "\"first name\"");
    assert_eq!(d.ident("a\"b"), "\"a\"\"b\"");
    // a quote-bearing identity is its own spelling when its interior is a valid quoted body
    assert_eq!(d.ident("\"date\""), "\"date\"");
    assert_eq!(d.ident("\"a\"b\""), "\"\"\"a\"\"b\"\"\"");
    assert_eq!(d.table_name("hr.EMPLOYEES", NameQuoting::default()), "\"hr\".\"EMPLOYEES\"");
    assert_eq!(d.table_name("Orders", NameQuoting { schema: false, table: true }), "\"Orders\"");
    assert_eq!(d.ddl_identifier("name", false), "NAME");
    assert_eq!(d.ddl_identifier("default", false), "\"DEFAULT\"");
    assert_eq!(d.ddl_identifier("Amt", true), "\"Amt\"");
}

#[test]
fn literals_are_spelled_as_java_spells_them() {
    assert_eq!(render(string_lit("it's")), "'it''s'");
    assert_eq!(render(string_lit("a\0b'\0")), "('a' || chr(0) || 'b''' || chr(0) || '')");
    let d = DuckDb::new();
    let ts = |iso: &str| d.timestamp_lit(iso).unwrap();
    assert_eq!(ts("2024-01-01T00:00:00.123456789"), "TIMESTAMP_NS '2024-01-01T00:00:00.123456789'");
    assert_eq!(ts("9999-12-31T23:59:59.999999000"), "TIMESTAMP '9999-12-31T23:59:59.999999000'");
    assert_eq!(ts("2024-01-01T00:00:00.123456789+0000"), "TIMESTAMP_NS '2024-01-01T00:00:00.123456789+0000'");
    let decimal = |text: &str, r#type: TypeFact| SqlExpr::DecimalLit {
        value: lite_sql::BigDecimal::parse(text).unwrap(),
        r#type,
    };
    let dec0 = TypeFact::Typed { r#type: SqlType::Decimal { precision: 5, scale: 0 }, nullable: false };
    assert_eq!(render(decimal("17774", dec0)), "CAST(17774 AS DECIMAL(5,0))");
    assert_eq!(render(decimal("1.5E-7", TypeFact::Unknown)), "0.00000015");
}

// ---- hand-built statements

#[test]
fn a_select_writes_its_clauses_in_order() {
    let d = DuckDb::new();
    let mut s = select(vec![proj(col("t0", "ID", Origin::Physical), None)], table("PERSON", "t0"));
    s.r#where = Some(call(SqlFn::Greater, vec![col("t0", "AGE", Origin::Physical), int_lit(30)]));
    s.group_by = vec![col("t0", "ID", Origin::Physical)];
    s.order_by = vec![SortKey {
        expr: col("t0", "ID", Origin::Physical),
        ascending: false,
        null_order: None,
        output_name: None,
    }];
    s.limit = Some(10);
    assert_eq!(
        SqlDialect::render(&d, query(s)).unwrap(),
        "SELECT t0.ID\nFROM PERSON AS t0\nWHERE t0.AGE > 30\nGROUP BY t0.ID\nORDER BY t0.ID DESC NULLS FIRST\nLIMIT 10"
    );
}

#[test]
fn infix_operators_parenthesize_by_precedence() {
    // 6 - (4 - 5): a trailing operand of the same precedence is its own value
    let e = call(SqlFn::Minus, vec![int_lit(6), call(SqlFn::Minus, vec![int_lit(4), int_lit(5)])]);
    assert_eq!(render(e), "6 - (4 - 5)");
    let e = call(SqlFn::Times, vec![call(SqlFn::Plus, vec![int_lit(1), int_lit(2)]), int_lit(3)]);
    assert_eq!(render(e), "(1 + 2) * 3");
    // comparisons are non-associative
    let e = call(
        SqlFn::Equal,
        vec![
            call(SqlFn::Equal, vec![int_lit(1), int_lit(1)]),
            SqlExpr::BoolLit { value: true, r#type: TypeFact::Unknown },
        ],
    );
    assert_eq!(render(e), "(1 = 1) = TRUE");
}

#[test]
fn an_outer_joins_condition_drops_the_not_null_tests_a_comparison_makes() {
    let d = DuckDb::new();
    let on = call(
        SqlFn::And,
        vec![
            call(SqlFn::IsNotNull, vec![col("t0", "ID", Origin::Physical)]),
            call(SqlFn::Equal, vec![col("t0", "ID", Origin::Physical), col("t1", "PID", Origin::Physical)]),
            call(SqlFn::IsNotNull, vec![col("t1", "X", Origin::Physical)]),
        ],
    );
    let join = |kind| SqlSource::Join {
        left: Box::new(table("A", "t0")),
        right: Box::new(table("B", "t1")),
        kind,
        on: Some(Box::new(on.clone())),
        outputs: Vec::new(),
    };
    let left = SqlDialect::render(&d, query(select(Vec::new(), join(JoinKind::Left)))).unwrap();
    assert_eq!(left, "SELECT *\nFROM A AS t0\nLEFT OUTER JOIN B AS t1 ON t0.ID = t1.PID AND t1.X IS NOT NULL");
    let inner = SqlDialect::render(&d, query(select(Vec::new(), join(JoinKind::Inner)))).unwrap();
    assert_eq!(
        inner,
        "SELECT *\nFROM A AS t0\nJOIN B AS t1 ON t0.ID IS NOT NULL AND t0.ID = t1.PID AND t1.X IS NOT NULL"
    );
}

#[test]
fn a_plan_parameter_is_bound_and_a_date_time_typed_by_its_value() {
    let d = DuckDb::new();
    let by_value = ValueTyping { kinds: vec![ValueKind::DateTime, ValueKind::Date], absent: ValueKind::DateTime };
    let s = select(
        vec![
            proj(param("n", typed(Scalar::Varchar), None), Some("n")),
            proj(param("ts", TypeFact::Unknown, Some(by_value)), Some("ts")),
        ],
        SqlSource::Dual,
    );
    let st = SqlDialect::render_statement(&d, query(s.clone())).unwrap();
    assert_eq!(st.sql, "SELECT ? AS n, CAST(? AS ) AS ts");
    assert_eq!(
        list_text(&st.binds),
        "[Bind[parameter=n, binding=One[nullType=VARCHAR, hole=null]], Bind[parameter=ts, binding=One[nullType=TIMESTAMP, \
         hole=TypeHole[at=25, types={DATE=TypeSpelling[name=DATE, digits=NONE, fraction=null], \
         DATE_TIME=TypeSpelling[name=TIMESTAMP, digits=NONE, fraction=null]}, absent=DATE_TIME]]]]"
    );
    let r = SqlDialect::render(&d, query(s)).unwrap_err();
    assert!(r.message.starts_with("a statement with bound parameters [Bind[parameter=n"), "{}", r.message);
}

#[test]
fn a_list_parameter_is_bound_whole_and_refused_as_one_value() {
    let d = DuckDb::new();
    let list = TypeFact::Typed {
        r#type: SqlType::Array { element: Box::new(SqlType::Scalar(Scalar::Bigint)) },
        nullable: false,
    };
    let mut s = select(Vec::new(), table("T", "t0"));
    s.r#where = Some(call(SqlFn::In, vec![col("t0", "ID", Origin::Physical), param("ids", list.clone(), None)]));
    let st = SqlDialect::render_statement(&d, query(s)).unwrap();
    assert_eq!(st.sql, "SELECT *\nFROM T AS t0\nWHERE t0.ID = ANY(?)");
    assert_eq!(list_text(&st.binds), "[Bind[parameter=ids, binding=Array[elementSqlType=BIGINT]]]");
    let one = select(vec![proj(param("ids", list, None), None)], SqlSource::Dual);
    let r = SqlDialect::render_statement(&d, query(one)).unwrap_err();
    assert_eq!(
        r.message,
        "list parameter 'ids' is bound only as a whole list (in, contains); written where one value goes, it is not bound"
    );
}

#[test]
fn a_per_key_run_is_one_region_written_last() {
    let d = DuckDb::new();
    let out = OutputCol {
        name: "_u0".into(),
        r#type: SqlType::Scalar(Scalar::Bigint),
        nullable: true,
        origin: Origin::PerKey,
    };
    let per_key =
        Projection { expr: SqlExpr::KeyCell { cell: 0, r#type: TypeFact::Unknown }, alias: None, out: Some(out) };
    let s = select(vec![proj(col("t0", "ID", Origin::Physical), Some("ID")), per_key], table("T", "t0"));
    let st = SqlDialect::render_statement(&d, query(s)).unwrap();
    assert_eq!(st.sql, "SELECT t0.ID AS ID? AS \"_pk__u0\"\nFROM T AS t0");
    assert_eq!(
        list_text(&st.regions),
        "[Region[start=18, end=32, lead=, , separator=, , empty=, firstBind=0, bindCount=1, counters=[27]]]"
    );
}

#[test]
fn a_checked_one_spells_puress_size_guard() {
    let list = col("t0", "XS", Origin::Physical);
    let one = SqlExpr::CheckedOne {
        list: Box::new(list),
        scalar_carrier: false,
        at_least_only: false,
        r#type: TypeFact::Unknown,
    };
    assert_eq!(
        render(one),
        "CASE WHEN t0.XS IS NULL OR len(t0.XS) <> 1 THEN error(chr(31) || (concat('Cannot cast a collection of size ', \
         CAST(coalesce(len(t0.XS), 0) AS VARCHAR), ' to multiplicity [1]')) || chr(31)) ELSE list_extract(t0.XS, 1) END"
    );
}

#[test]
fn ddl_and_dml_spell_the_stores_names() {
    let d = DuckDb::new();
    let ddl = SqlDdl::CreateTable {
        schema: Some("hr".into()),
        table: "Orders".into(),
        quoting: NameQuoting { schema: false, table: true },
        columns: vec![
            DdlColumn {
                name: "id".into(),
                declared_quoted: false,
                r#type: ColumnType::Plain { kind: ColumnKind::Integer },
                not_null: true,
                primary_key: true,
            },
            DdlColumn {
                name: "Amt".into(),
                declared_quoted: true,
                r#type: ColumnType::Plain { kind: ColumnKind::Float },
                not_null: false,
                primary_key: false,
            },
            DdlColumn {
                name: "name".into(),
                declared_quoted: false,
                r#type: ColumnType::Sized { kind: "VARCHAR".into(), size: 20 },
                not_null: false,
                primary_key: false,
            },
        ],
        temporary: false,
    };
    assert_eq!(
        SqlDialect::render_ddl(&d, &ddl).unwrap(),
        "Create Table hr.\"Orders\"(ID INTEGER NOT NULL, \"Amt\" DOUBLE NULL, NAME VARCHAR(20) NULL, PRIMARY KEY(ID));"
    );
    let insert = SqlDml::InsertValues {
        schema: None,
        table: "T".into(),
        quoting: NameQuoting::default(),
        columns: vec!["a".into(), "b".into()],
        quoted_columns: vec!["b".into()],
        rows: vec![vec![int_lit(1), string_lit("x")], vec![int_lit(2), param("p", typed(Scalar::Varchar), None)]],
    };
    let r = SqlDialect::render_dml(&d, &insert).unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (
            Refusal::DIALECT_CAPABILITY,
            "plan parameters [Bind[parameter=p, binding=One[nullType=VARCHAR, hole=null]]] in a DML row: a row holds \
             values, and a DML statement is text"
        )
    );
    let plain = SqlDml::InsertValues {
        schema: None,
        table: "T".into(),
        quoting: NameQuoting::default(),
        columns: vec!["a".into(), "b".into()],
        quoted_columns: vec!["b".into()],
        rows: vec![vec![int_lit(1), string_lit("x")]],
    };
    assert_eq!(SqlDialect::render_dml(&d, &plain).unwrap(), "INSERT INTO T (a, \"b\") VALUES (1, 'x');");
}

#[test]
fn the_engines_h2_text_is_translated_for_duckdb() {
    let same = |s: String| s;
    let bare = |s: &str| s.to_owned();
    assert_eq!(
        translated("select count(*) from (select 1 as x)", &same, &bare),
        "select count(*) AS \"COUNT(*)\" from (select 1 as x)"
    );
    assert_eq!(translated("select 1 as A", &same, &bare), "select 1 as A");
    assert_eq!(
        translated("create table T(id INT, float FLOAT, b BIT, PRIMARY KEY(id))", &same, &bare),
        "create table T(\"id\" INT, \"float\" DOUBLE, \"b\" BOOLEAN, PRIMARY KEY(id))"
    );
    assert_eq!(
        translated("insert into T (id, \"x\", name) values (1, 2, CURRENT_TIMESTAMP())", &same, &bare),
        "insert into T (\"id\", \"x\", \"name\") values (1, 2, CURRENT_TIMESTAMP)"
    );
    assert_eq!(translated("Drop schema S if exists cascade", &same, &bare), "Drop schema if exists S cascade");
    assert_eq!(translated("create schema S", &same, &bare), "Create schema if not exists S");
    assert_eq!(translated("create schema if not exists S", &same, &bare), "create schema if not exists S");
    assert_eq!(translated("SELECT H2VERSION( )", &same, &bare), "SELECT '2.1.214'");
}

#[test]
fn a_data_url_inlines_its_payload_and_a_file_url_reads_its_path() {
    let d = DuckDb::new();
    assert_eq!(
        d.source_url("data:application/json,[{\"a\":'x'}]").unwrap(),
        "SELECT unnest(CAST('[{\"a\":''x''}]' AS JSON[])) AS data"
    );
    assert_eq!(
        d.source_url("file:///tmp/a%20b//c.json").unwrap(),
        "SELECT json AS data FROM read_json_objects('/tmp/a b/c.json')"
    );
    assert_eq!(d.source_url("http://x").unwrap_err().message, "unsupported sourceUrl scheme: http://x");
}

#[test]
fn a_subquery_is_written_on_one_line_and_inline_mode_is_restored() {
    let d = DuckDb::new();
    let inner = query(select(vec![proj(int_lit(1), None)], table("T", "t1")));
    let mut s = select(Vec::new(), table("A", "t0"));
    s.r#where = Some(SqlExpr::Exists { subquery: Box::new(inner), r#type: TypeFact::Unknown });
    s.limit = Some(1);
    let text = SqlDialect::render(&d, query(s.clone())).unwrap();
    assert_eq!(text, "SELECT *\nFROM A AS t0\nWHERE EXISTS (SELECT 1 FROM T AS t1)\nLIMIT 1");
    assert_eq!(
        SqlDialect::render_inline(&d, query(s)).unwrap(),
        "SELECT * FROM A AS t0 WHERE EXISTS (SELECT 1 FROM T AS t1) LIMIT 1"
    );
    assert!(!d.ansi_state().inline.get());
}

#[test]
fn the_pass_pipeline_runs_in_javas_order() {
    let d = DuckDb::new();
    let names: Vec<&str> = ansi::render_passes(&d).iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        [
            "FloatPrintText.Expansion",
            "FloatCanonText.Expansion",
            "CarrierStrategies",
            "QuantileOrder",
            "UnqualifyPivotArgs",
            "FoldToListReduce",
            "CheckedDefectsToLists",
            "SubstringClamp",
            "ConstantKeysAsExpressions",
            "OuterJoinSubqueryFilters",
            "VariantEgress",
            "StoredReads",
        ]
    );
    let mut scan = DuckDb::new();
    scan.engine_scan_order = true;
    let names: Vec<&str> = ansi::render_passes(&scan).iter().map(|p| p.name()).collect();
    assert_eq!(names[9..11], ["OuterJoinSubqueryFilters", "StableScanOrder"]);
}
