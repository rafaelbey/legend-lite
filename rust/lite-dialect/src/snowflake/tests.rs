// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Snowflake's hooks held to their Java (`Snowflake.java`) where the golden oracles cannot reach them: the helpers'
//! defaults their one caller never passes (`list`'s, `bitOp`'s), the answer the arms oracle could not record (a list
//! lambda that is no lambda: Java's `ClassCastException`, its message less the JVM's class loader), `dateTrunc`'s parts
//! refused by `unit` before its own wall, Snowflake's own `PivotAsGroupBy`, the fresh names' counter (started over by
//! the outermost render, continued by a nested one and by a DML render), printf's directives and refusals, the regular
//! expressions' readers, the literals and identifiers, the raise in a tagged slot, the wire names and the pass list.
//! The oracles hold the rest (`tests/dialect_differential.rs`, `mir/dialect-arms.jsonl`).

use super::functions::format_text;
use super::*;
use crate::SqlDialect;
use crate::build::{call, int_lit, string_lit};
use lite_sql::{BigDecimal, DateFmtPart, DatePart, NameQuoting, SqlDml};

fn typed(s: Scalar) -> TypeFact {
    TypeFact::Typed { r#type: SqlType::Scalar(s), nullable: true }
}

fn col(name: &str, t: TypeFact) -> SqlExpr {
    SqlExpr::Column(lite_sql::Column {
        table: Some("t0".into()),
        name: name.into(),
        r#type: t,
        origin: Some(Origin::Physical),
    })
}

fn list(name: &str, s: Scalar) -> SqlExpr {
    col(name, TypeFact::Typed { r#type: SqlType::Array { element: Box::new(SqlType::Scalar(s)) }, nullable: true })
}

fn render_result(e: &SqlExpr) -> Result<String, Refusal> {
    let mut w = SqlWriter::new();
    Snowflake::new().expr(&mut w, e, 0)?;
    w.into_text()
}

fn render(e: &SqlExpr) -> String {
    render_result(e).unwrap_or_else(|r| panic!("{r}"))
}

fn part(p: DatePart) -> SqlExpr {
    SqlExpr::DatePartLit { part: p, r#type: TypeFact::Unknown }
}

/// `SELECT e AS "v"` over no source.
fn select_one(e: SqlExpr) -> SqlQuery {
    build::query(
        build::select_of(vec![build::projection(e, Some("v".into()), None).unwrap()], SqlSource::Dual, vec![]).unwrap(),
    )
}

#[test]
fn the_helpers_defaults_no_caller_reaches_answer_as_java() {
    let d = Snowflake::new();
    let mut w = SqlWriter::new();
    let r = d.list_call(&mut w, SqlFn::Abs, &[list("xs", Scalar::Bigint)]).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_STATE, "not a list call: ABS"));
    let mut w = SqlWriter::new();
    let r = d.bit_op(&mut w, SqlFn::Plus, &[int_lit(1), int_lit(2)]).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::ILLEGAL_STATE, "not a bit op: PLUS"));
    // the bit operators over NUMBER(38,0)
    let mut w = SqlWriter::new();
    d.bit_op(&mut w, SqlFn::BitShiftLeft, &[int_lit(1), int_lit(63)]).unwrap();
    assert_eq!(w.into_text().unwrap(), "BITSHIFTLEFT(1, 63)");
}

#[test]
fn a_list_lambda_that_is_no_lambda_is_javas_class_cast() {
    // Java's `(SqlExpr.Lambda) a.get(1)`: a ClassCastException, its message less the JVM's class loader
    let xs = list("xs", Scalar::Bigint);
    for f in [SqlFn::ListTransform, SqlFn::ListFilter, SqlFn::ListExists, SqlFn::ListForAll] {
        let r = render_result(&call(f, vec![xs.clone(), int_lit(1)])).unwrap_err();
        assert_eq!(
            (r.class, r.message.as_str(), r.kind()),
            (
                Refusal::CLASS_CAST,
                "class com.legend.sql.SqlExpr$IntLit cannot be cast to class com.legend.sql.SqlExpr$Lambda",
                "CLASS_CAST"
            ),
            "{f}"
        );
    }
    // a lambda of the element and its index ranges over the positions, the element and index read off each
    let pair =
        build::lambda(&["x", "i"], call(SqlFn::Plus, vec![build::derived(None, "x"), build::derived(None, "i")]));
    assert_eq!(
        render(&call(SqlFn::ListTransform, vec![xs.clone(), pair])),
        "TRANSFORM(ARRAY_GENERATE_RANGE(0, ARRAY_SIZE(\"t0\".\"xs\")), \"__i0\" INTEGER -> CAST(GET(\"t0\".\"xs\", \
         \"__i0\") AS BIGINT) + (\"__i0\" + 1))"
    );
    let three = build::lambda(&["a", "b", "c"], int_lit(1));
    let r = render_result(&call(SqlFn::ListTransform, vec![xs, three])).unwrap_err();
    assert_eq!(r.message, "a lambda of 3 parameters over a list reached Snowflake");
}

#[test]
fn a_lambda_reading_an_enclosing_lambdas_name_is_carried_or_refused() {
    // removeDuplicates' comparator: a FILTER whose body reads the enclosing lambda's parameter is a REDUCE carrying it
    let xs = list("xs", Scalar::Bigint);
    let y = SqlExpr::Column(lite_sql::Column {
        table: None,
        name: "y".into(),
        r#type: typed(Scalar::Bigint),
        origin: Some(Origin::Derived),
    });
    let inner = build::lambda(&["x"], call(SqlFn::Equal, vec![build::derived(None, "x"), y]));
    let outer = build::lambda(&["y"], call(SqlFn::ListFilter, vec![xs.clone(), inner]));
    assert_eq!(
        render(&call(SqlFn::ListTransform, vec![xs.clone(), outer])),
        "TRANSFORM(\"t0\".\"xs\", \"y\" BIGINT -> CAST(GET(REDUCE(\"t0\".\"xs\", \
         OBJECT_CONSTRUCT_KEEP_NULL('r', ARRAY_CONSTRUCT(), 'c0', \"y\"), (\"__s0\" OBJECT, \"x\" BIGINT) -> \
         OBJECT_CONSTRUCT_KEEP_NULL('r', IFF(\"x\" = CAST(GET(\"__s0\", 'c0') AS BIGINT), \
         ARRAY_APPEND(CAST(GET(\"__s0\", 'r') AS ARRAY), \"x\"), CAST(GET(\"__s0\", 'r') AS ARRAY)), 'c0', \
         GET(\"__s0\", 'c0'))), 'r') AS ARRAY))"
    );
    // any other read of an enclosing lambda's parameter is refused by name
    let reads = build::lambda(&["a", "b"], build::derived(None, "y"));
    let outer = build::lambda(&["y"], call(SqlFn::ListReduce, vec![xs.clone(), reads]));
    let r = render_result(&call(SqlFn::ListTransform, vec![xs, outer])).unwrap_err();
    assert_eq!(
        r.message,
        "a lambda reading the parameter 'y' of an enclosing lambda reached Snowflake, whose lambdas cannot (measured: \
         invalid identifier)"
    );
}

#[test]
fn date_trunc_refuses_by_unit_before_its_own_wall() {
    // `unit(part)` is read before dateTrunc's switch: its own wall for these parts is never reached
    for (p, name) in [
        (DatePart::Nanosecond, "NANOSECOND"),
        (DatePart::DayOfYear, "DAY_OF_YEAR"),
        (DatePart::IsoDayOfWeek, "ISO_DAY_OF_WEEK"),
    ] {
        let ts = col("ts", typed(Scalar::Timestamp));
        let r = render_result(&call(SqlFn::DateTrunc, vec![part(p), ts])).unwrap_err();
        assert_eq!(r.message, format!("a {name} unit has no Snowflake spelling"));
    }
    let d = col("d", typed(Scalar::Date));
    assert_eq!(
        render(&call(SqlFn::DateTrunc, vec![part(DatePart::Week), d.clone()])),
        "CAST(DATE_TRUNC('week', \"t0\".\"d\") AS DATE)"
    );
    assert_eq!(
        render(&call(SqlFn::DateTrunc, vec![part(DatePart::Hour), d])),
        "DATE_TRUNC('hour', CAST(\"t0\".\"d\" AS TIMESTAMP_NTZ))"
    );
}

#[test]
fn snowflakes_own_pivot_pass_leaves_an_argumentless_using_to_the_writer() {
    let source = SqlSource::Table {
        name: "trades".into(),
        alias: "t0".into(),
        outputs: vec![OutputCol {
            name: "id".into(),
            r#type: SqlType::Scalar(Scalar::Bigint),
            nullable: true,
            origin: Origin::Physical,
        }],
        call: false,
        stored_types: Vec::new(),
        quoting: NameQuoting::default(),
    };
    let pivot = |values: Vec<SqlExpr>, args: Vec<SqlExpr>| SqlSource::Pivot {
        source: Box::new(source.clone()),
        on: vec![col("sym", typed(Scalar::Varchar))],
        r#in: values,
        named: Vec::new(),
        usings: vec![lite_sql::PivotUsing {
            agg: build::reducer(AggFn::Count, args, false, Vec::new()),
            alias: "n".into(),
            r#type: Some(SqlType::Scalar(Scalar::Bigint)),
        }],
        alias: "p".into(),
        outputs: vec![OutputCol {
            name: "id".into(),
            r#type: SqlType::Scalar(Scalar::Bigint),
            nullable: true,
            origin: Origin::Physical,
        }],
    };
    let mut pass = PivotAsGroupBy;
    assert_eq!(pass.name(), "Snowflake.PivotAsGroupBy");
    // COUNT(*): left as it came (the package's pass refuses it), then refused by the writer
    let counted = pivot(vec![string_lit("A")], Vec::new());
    assert!(matches!(pass.source(counted.clone()).unwrap(), Rw::Same(_)));
    let q = build::query(build::star_of(counted).unwrap());
    let r = AnsiSqlRenderer::render(&Snowflake::new(), q).unwrap_err();
    assert_eq!(
        r.message,
        "a pivot reached the Snowflake writer: PivotAsGroupBy emulates a static one; this one has 1 pivot columns"
    );
    // a value of no literal text, in Snowflake's words
    let date = SqlExpr::DateLit { iso: "2024-01-01".into(), r#type: TypeFact::Unknown };
    let r = pass.source(pivot(vec![date], vec![col("x", typed(Scalar::Bigint))])).unwrap_err();
    assert_eq!(r, Refusal::capability("a pivot value DateLit reached Snowflake without a column name"));
}

#[test]
fn the_outermost_render_starts_the_fresh_names_over() {
    let jw = || call(SqlFn::JaroWinkler, vec![string_lit("a"), string_lit("b")]);
    let d = Snowflake::new();
    // Java's `fresh = 0` at each outermost render: the same names twice on one dialect
    for _ in 0..2 {
        let sql = AnsiSqlRenderer::render(&d, select_one(jw())).unwrap();
        assert!(sql.contains("\"__o0\" OBJECT -> ") && sql.contains("\"__i6\" INTEGER"), "{sql}");
    }
    let statement = AnsiSqlRenderer::render_statement(&d, select_one(jw())).unwrap();
    assert!(statement.sql.contains("\"__o0\" OBJECT -> "), "{}", statement.sql);
    // renderInline goes through render: the outermost call starts over
    let inline = ansi::render_inline(&d, select_one(jw())).unwrap();
    assert!(inline.contains("\"__o0\" OBJECT -> "), "{inline}");
    // a DML render does not start over: its row's names continue the count of the last query render
    let insert = SqlDml::InsertValues {
        schema: None,
        table: "t".into(),
        quoting: NameQuoting::default(),
        columns: Vec::new(),
        quoted_columns: Vec::new(),
        rows: vec![vec![jw()]],
    };
    let dml = AnsiSqlRenderer::render_dml(&d, &insert).unwrap();
    assert!(dml.contains("\"__o7\" OBJECT -> "), "{dml}");
    // a fresh dialect (the seam's, each call) starts at 0
    let fresh = AnsiSqlRenderer::render_dml(&Snowflake::new(), &insert).unwrap();
    assert!(fresh.contains("\"__o0\" OBJECT -> "), "{fresh}");
}

#[test]
fn printf_is_unrolled_as_java_reads_its_directives() {
    let x = col("x", typed(Scalar::Bigint));
    let y = col("y", typed(Scalar::Double));
    let s = col("s", typed(Scalar::Varchar));
    let format = |f: &str, args: Vec<SqlExpr>| {
        let mut a = vec![string_lit(f)];
        a.extend(args);
        call(SqlFn::Format, a)
    };
    assert_eq!(
        render(&format("%05d|%.2f|%%|%s", vec![x.clone(), y.clone(), s.clone()])),
        "((CASE WHEN \"t0\".\"x\" < 0 THEN '-' || LPAD(TO_VARCHAR(ABS(\"t0\".\"x\")), 4, '0') ELSE \
         LPAD(TO_VARCHAR(ABS(\"t0\".\"x\")), 5, '0') END) || '|' || (CASE WHEN \"t0\".\"y\" < 0 AND \
         CAST(\"t0\".\"y\" AS NUMBER(38, 2)) = 0 THEN '-' ELSE '' END || TO_VARCHAR(CAST(\"t0\".\"y\" AS \
         NUMBER(38, 2)))) || '|%|' || \"t0\".\"s\")"
    );
    // a width pads with blanks; a value's own text through TO_VARCHAR; a literal format with no directive is its text
    assert_eq!(render(&format("%3s", vec![x.clone()])), "(LPAD(TO_VARCHAR(\"t0\".\"x\"), 3, ' '))");
    assert_eq!(render(&format("", Vec::new())), "('')");
    // a directive other than s, d and f is text: refused where the '%' count says so
    let r = render_result(&format("a%e", vec![])).unwrap_err();
    assert_eq!(r.message, "a format directive other than %s, %d and %f reached Snowflake: a%e");
    let r = render_result(&format("%s %s", vec![x.clone()])).unwrap_err();
    assert_eq!(r.message, "a format with more directives than arguments reached Snowflake");
    // ... but taken as text where the count balances, an argument dropped (Java's check counts every '%')
    assert_eq!(render(&format("%e %s", vec![s.clone(), x.clone()])), "('%e ' || \"t0\".\"s\")");
    // Integer.parseInt of a width past an int
    let r = render_result(&format("%99999999999d", vec![x])).unwrap_err();
    assert_eq!((r.class, r.message.as_str()), (Refusal::NUMBER_FORMAT, "For input string: \"99999999999\""));
    let r = render_result(&call(SqlFn::Format, vec![s])).unwrap_err();
    assert_eq!(r.message, "a computed format reached Snowflake, which has no printf: only a literal one is unrolled");
}

#[test]
fn the_regular_expressions_read_as_java_reads_them() {
    let s = col("s", typed(Scalar::Varchar));
    // inline flags as parameters; a non-capturing group a plain one
    assert_eq!(
        render(&call(SqlFn::RegexpFullMatch, vec![s.clone(), string_lit("(?is)a(?:b|c)\\d")])),
        "REGEXP_LIKE(\"t0\".\"s\", 'a(b|c)\\\\d', 'cis')"
    );
    // a non-capturing group before a read group renumbers it: refused
    let r =
        render_result(&call(SqlFn::RegexpExtract, vec![s.clone(), string_lit("(?:x)(y)"), int_lit(1)])).unwrap_err();
    assert_eq!(
        r.message,
        "a non-capturing group in a pattern whose groups are numbered reached Snowflake (no (?: here, measured): \
         (?:x)(y)"
    );
    // a lazy quantifier and a lookaround are no POSIX
    let r = render_result(&call(SqlFn::Matches, vec![s.clone(), string_lit("a+?")])).unwrap_err();
    assert!(r.message.starts_with("a lazy quantifier reached Snowflake"), "{r}");
    let r = render_result(&call(SqlFn::Matches, vec![s.clone(), string_lit("a(?=b)")])).unwrap_err();
    assert_eq!(
        r.message,
        "a lookaround, flag group or named group reached Snowflake's POSIX regular expressions: a(?=b)"
    );
    // the text before a match (the lowering's whole match after its lazy prefix), or before a group opening the rest
    // (^((?:.*?)B)F): the lazy prefix read as REGEXP_INSTR's position; any other prefix is the pattern as it is
    assert_eq!(
        render(&call(SqlFn::RegexpExtract, vec![s.clone(), string_lit("^((?:.*?))(?:b+)"), int_lit(1)])),
        "COALESCE(LEFT(\"t0\".\"s\", NULLIF(REGEXP_INSTR(\"t0\".\"s\", 'b+', 1, 1, 0, 'c'), 0) - 1), '')"
    );
    assert_eq!(
        render(&call(SqlFn::RegexpExtract, vec![s.clone(), string_lit("(?i)^((?:.*?)a)(b)"), int_lit(1)])),
        "COALESCE(LEFT(\"t0\".\"s\", NULLIF(REGEXP_INSTR(\"t0\".\"s\", 'a(b)', 1, 1, 0, 'ci', 1), 0) - 1), '')"
    );
    // a replacement reading no group reads none of the pattern's numbering; 'g' replaces every match
    assert_eq!(
        render(&call(SqlFn::RegexpReplace, vec![s.clone(), string_lit("(?:a)(b)"), string_lit("x"), string_lit("gi")])),
        "REGEXP_REPLACE(\"t0\".\"s\", '(a)(b)', 'x', 1, 0, 'ci')"
    );
    let r =
        render_result(&call(SqlFn::RegexpReplace, vec![s.clone(), string_lit("a"), string_lit("x"), s])).unwrap_err();
    assert_eq!(r.message, "REGEXP_REPLACE reached Snowflake: computed regexp options");
}

#[test]
fn literals_identifiers_and_types_answer_as_java() {
    let d = Snowflake::new();
    // a decimal whose trailing zeros carry its scale spells its type (Snowflake drops it: 17.0 is a NUMBER(2,0))
    assert_eq!(d.float_literal(17.0).unwrap(), "CAST(17.0 AS NUMBER(3,1))");
    assert_eq!(d.float_literal(0.0).unwrap(), "CAST(0.0 AS NUMBER(1,1))");
    assert_eq!(d.float_literal(1.0e-5).unwrap(), "CAST(0.000010 AS NUMBER(6,6))");
    assert_eq!(d.float_literal(1.5).unwrap(), "1.5");
    assert_eq!(d.float_literal(1.0e20).unwrap(), "1.0E20");
    assert_eq!(d.float_literal(f64::NAN).unwrap(), "CAST('NaN' AS DOUBLE)");
    assert_eq!(d.float_literal(f64::NEG_INFINITY).unwrap(), "CAST('-inf' AS DOUBLE)");
    let typed_decimal = build::decimal_lit(BigDecimal::parse("1.50").unwrap());
    assert_eq!(render(&typed_decimal), "CAST(1.50 AS DECIMAL(3, 2))");
    let untyped = SqlExpr::DecimalLit { value: BigDecimal::parse("1.50").unwrap(), r#type: TypeFact::Unknown };
    assert_eq!(render(&untyped), "CAST(1.50 AS NUMBER(3,2))");
    assert_eq!(render(&build::decimal_lit(BigDecimal::parse("1.25").unwrap())), "1.25");
    // a backslash escaped, a quote doubled, NUL spliced
    assert_eq!(d.string_lit("a'b\\c").unwrap(), "'a''b\\\\c'");
    assert_eq!(d.string_lit("a\0b'").unwrap(), "('a' || chr(0) || 'b''')");
    assert_eq!(d.timestamp_lit("2024-01-02 03:04:05.123456789").unwrap(), "TIMESTAMP '2024-01-02 03:04:05.123456'");
    assert_eq!(
        d.timestamp_lit("2024-01-02 03:04:05.1234567+01:00").unwrap(),
        "TIMESTAMP '2024-01-02 03:04:05.123456+01:00'"
    );
    assert_eq!(d.ident("plain"), "\"plain\"");
    assert_eq!(d.ident("\"da\"\"te\""), "\"da\"\"te\"");
    assert_eq!(d.ident("a\"b"), "\"a\"\"b\"");
    assert_eq!(d.cte_name("__se"), "\"__se\"");
    assert_eq!(d.hole_type(ValueKind::Date).unwrap(), TypeSpelling::new("DATE", Digits::None));
    assert_eq!(
        d.hole_type(ValueKind::DateTimeNanos).unwrap(),
        TypeSpelling { name: "TIMESTAMP_NTZ".into(), digits: Digits::None, fraction: Some(6) }
    );
    assert!(d.hole_type(ValueKind::Integer).is_err());
    let r = d.ddl_type(&ColumnType::Plain { kind: ColumnKind::Distinct }).unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::ILLEGAL_STATE, "no DDL spelling for declared column type DISTINCT")
    );
    assert_eq!(d.ddl_type(&ColumnType::Plain { kind: ColumnKind::TimestampNs }).unwrap(), "TIMESTAMP_NTZ(9)");
    // a struct's fields are spelled before its OBJECT: a field Snowflake cannot spell refuses the struct
    let field = |t: Scalar| lite_sql::StructField { name: "f".into(), r#type: SqlType::Scalar(t) };
    assert_eq!(d.struct_type_name(&[field(Scalar::Bigint)]).unwrap(), "OBJECT");
    let r = d.struct_type_name(&[field(Scalar::DecimalText)]).unwrap_err();
    assert_eq!(r.message, "DECIMAL_TEXT cast reached a dialect without DECIMAL_TEXT support");
    // a date format's text: double quotes refused, a weekday name refused
    assert_eq!(
        format_text(&[
            DateFmt::Part(DateFmtPart::Year4),
            DateFmt::Text { s: "-".into() },
            DateFmt::Part(DateFmtPart::SubsecMin)
        ])
        .unwrap(),
        "YYYY\"-\"FF3"
    );
    assert!(format_text(&[DateFmt::Text { s: "\"".into() }]).is_err());
    assert_eq!(
        format_text(&[DateFmt::Part(DateFmtPart::WeekdayName)]).unwrap_err().message,
        "a full weekday name in a Snowflake date format (DY is three letters)"
    );
}

#[test]
fn a_raise_takes_its_slots_type() {
    let d = Snowflake::new();
    let msg = [string_lit("boom")];
    let raised = |slot: Option<&SqlType>| -> Result<String, Refusal> {
        let p = d.raise(&msg, slot)?;
        let mut w = SqlWriter::new();
        w.piece(&*p)?;
        w.into_text()
    };
    assert_eq!(raised(None).unwrap(), "CAST(CAST(CHR(31) || ('boom') || CHR(31) AS INTEGER) AS VARCHAR)");
    assert_eq!(raised(Some(&SqlType::Scalar(Scalar::Date))).unwrap(), "CAST(CHR(31) || ('boom') || CHR(31) AS DATE)");
    assert_eq!(
        raised(Some(&SqlType::Array { element: Box::new(SqlType::Scalar(Scalar::Bigint)) })).unwrap(),
        "TO_ARRAY(CAST(CHR(31) || ('boom') || CHR(31) AS INTEGER))"
    );
    let r = raised(Some(&SqlType::Tagged { slots: Vec::new() })).unwrap_err();
    assert_eq!(
        (r.class, r.message.as_str()),
        (Refusal::ILLEGAL_STATE, "a tagged value is never raised as: its tag and slots are its query's columns")
    );
}

#[test]
fn the_wire_and_the_facts_answer_as_java() {
    let d = Snowflake::new();
    assert_eq!(d.wire_type_name(Some("NUMBER")), "DECIMAL");
    assert_eq!(d.wire_type_name(Some(" number ")), "DECIMAL");
    assert_eq!(d.typing_wire_name(Some("NUMBER")), None);
    assert_eq!(d.wire_type_name(Some("VARCHAR")), crate::wire_names::fold(Some("VARCHAR")));
    assert_eq!(d.typing_wire_name(Some("TIMESTAMP_NTZ")), Some(crate::wire_names::fold(Some("TIMESTAMP_NTZ"))));
    assert_eq!(d.wire_type_name(None), crate::wire_names::fold(None));
    let st = SqlType::Struct { fields: Vec::new() };
    assert!(d.delivers_own(&SqlType::Scalar(Scalar::Bigint), "DECIMAL(38,0)"));
    assert!(d.delivers_own(&SqlType::Scalar(Scalar::Integer), "DECIMAL(2,0)"));
    assert!(!d.delivers_own(&SqlType::Scalar(Scalar::Integer), "DECIMAL(10,2)"));
    assert!(!d.delivers_own(&SqlType::Scalar(Scalar::Hugeint), "DECIMAL(38,0)"));
    assert!(d.delivers_own(&st, "OBJECT"));
    assert!(!d.delivers_own(&st, "VARIANT"));
    assert_eq!(d.session_setup(), vec![SESSION_SETUP.to_owned()]);
    assert_eq!(d.keys_snapshot().unwrap(), KeysSnapshot::None);
    assert_eq!(d.result_column_limit().unwrap(), None);
    assert!(d.needs_static_pivot());
    assert_eq!(d.typeof_name(Scalar::Date).unwrap(), "DATE");
    assert_eq!(
        d.typeof_name(Scalar::Time).unwrap_err().message,
        "a TYPEOF compared with TIME's name reached Snowflake"
    );
    assert_eq!(d.json_kind_name(JsonKind::UnsignedInteger).unwrap(), "UBIGINT");
}

#[test]
fn the_passes_are_javas_in_javas_order() {
    let names: Vec<&str> = Snowflake::new().passes().iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        [
            "CarrierStrategies",
            "Snowflake.PivotAsGroupBy",
            "CheckedDefectsToLists",
            "SubstringClamp",
            "ExplosionMarkers.LateralUnnestAsMarker",
            "ExplosionMarkers.SelectListUnnestAsLateral",
            "ExplosionMarkers.StarOverExplosionUnwrapped",
            "ConstantKeysAsExpressions",
            "RootNumericTypes",
        ]
    );
    let d = Snowflake::new();
    assert!(d.spells_float_canon() && d.spells_float_print());
    assert!(!d.binds_float_text() && !d.binds_float_print());
    // the expansions run nowhere: the dialect spells a Float's texts itself
    let names: Vec<&str> = ansi::render_passes(&d).iter().map(|p| p.name()).collect();
    assert_eq!(names.first(), Some(&"CarrierStrategies"));
    assert_eq!(names.last(), Some(&"StoredReads"));
}
