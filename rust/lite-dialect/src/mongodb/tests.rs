// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MongoDB's hooks held to their Java (`MongoDb.java`, `MongoDbPipeline.java`, `MongoJson.java`) where the golden
//! oracles cannot reach them: the helpers' defaults their one caller never passes (`bitOp`'s, `listCall`'s,
//! `jsonText`'s, `accumulator`'s and `emptyInput`'s), the cast type names `variantAwareCast` takes first, `ddlType`
//! (its `render(SqlDdl)` writes no column type), `structTypeName`, the nested-query and flat-join walls,
//! `reduceArray`'s COUNT, the Float forms expanded before every pass, the window over a grouped aggregate the arms
//! oracle dropped (Java's `ClassCastException`), the JSON writer's names, keys and dates, the insert's values, the
//! records' text the lowering-bug refusals embed, and the per-render state. The oracles hold the rest
//! (`tests/dialect_differential.rs`).

use super::*;
use crate::SqlDialect;
use crate::build::{self, call, int_lit, string_lit};
use lite_sql::{NameQuoting, OutputCol, Projection, SqlSelect, SqlSource};

// ---- builders

fn t(s: Scalar) -> SqlType {
    SqlType::Scalar(s)
}

fn out(name: &str, ty: SqlType) -> OutputCol {
    OutputCol { name: name.into(), r#type: ty, nullable: true, origin: Origin::Physical }
}

fn table(alias: &str, outputs: Vec<OutputCol>) -> SqlSource {
    SqlSource::Table {
        name: "t".into(),
        alias: alias.into(),
        outputs,
        call: false,
        stored_types: Vec::new(),
        quoting: NameQuoting::default(),
    }
}

/// `t AS t0 (s VARCHAR, n BIGINT)`.
fn t0() -> SqlSource {
    table("t0", vec![out("s", t(Scalar::Varchar)), out("n", t(Scalar::Bigint))])
}

fn col(name: &str, ty: SqlType) -> SqlExpr {
    SqlExpr::Column(Column {
        table: Some("t0".into()),
        name: name.into(),
        r#type: TypeFact::Typed { r#type: ty, nullable: true },
        origin: Some(Origin::Physical),
    })
}

fn n() -> SqlExpr {
    col("n", t(Scalar::Bigint))
}

fn s() -> SqlExpr {
    col("s", t(Scalar::Varchar))
}

fn proj(e: SqlExpr, alias: &str) -> Projection {
    let ty = match e.r#type() {
        TypeFact::Typed { r#type, .. } => r#type.clone(),
        _ => t(Scalar::Bigint),
    };
    build::projection(e, Some(alias.into()), Some(build::output_col(alias, ty, true))).unwrap()
}

fn select(projections: Vec<Projection>, from: SqlSource) -> SqlSelect {
    build::select_of(projections, from, Vec::new()).unwrap()
}

/// `e` written by a dialect whose pipeline scopes are set, as a query's render sets them.
fn expr_result(d: &MongoDb, e: &SqlExpr) -> Result<String, Refusal> {
    d.reset();
    let mut w = SqlWriter::new();
    d.expr(&mut w, e, 0)?;
    w.into_text()
}

fn expr_text(d: &MongoDb, e: &SqlExpr) -> String {
    expr_result(d, e).unwrap_or_else(|r| panic!("{r}"))
}

fn refused(r: Result<impl std::fmt::Debug, Refusal>) -> (&'static str, String) {
    let r = r.expect_err("a refusal");
    (r.class, r.message)
}

const N_TEXT: &str = "Column[table=t0, name=n, type=Typed[type=BIGINT, nullable=true], origin=PHYSICAL]";

// ---- the helpers' defaults

#[test]
fn the_helpers_defaults_no_caller_reaches_answer_as_java() {
    let d = MongoDb::new();
    d.reset();
    let mut w = SqlWriter::new();
    assert_eq!(
        refused(d.bit_op(&mut w, SqlFn::Plus, &[int_lit(1), int_lit(2)])),
        (Refusal::ILLEGAL_STATE, "not a bit operation: PLUS".to_owned())
    );
    let mut w = SqlWriter::new();
    assert_eq!(
        refused(d.list_call(&mut w, SqlFn::Abs, &[n()])),
        (Refusal::ILLEGAL_STATE, "not a list call: ABS".to_owned())
    );
    let mut w = SqlWriter::new();
    assert_eq!(
        refused(texts::json_text(&d, &mut w, &n())),
        (Refusal::ILLEGAL_STATE, format!("not a JSON constructor: {N_TEXT}"))
    );
    let field = GroupField::new("__agg0", false).unwrap();
    let mut w = SqlWriter::new();
    assert_eq!(
        refused(aggregates::accumulator(&d, &mut w, &n(), &field).map(|_| ())),
        (Refusal::ILLEGAL_STATE, format!("not an aggregate: {N_TEXT}"))
    );
    assert_eq!(refused(aggregates::empty_input(&n())), (Refusal::ILLEGAL_STATE, format!("not an aggregate: {N_TEXT}")));
    // a group field's stem is a field name
    assert_eq!(
        refused(GroupField::new("a.b", false)).1,
        "'a.b' is no MongoDB field name (a name that is empty, begins with $, or holds a dot or a NUL): a column or \
         alias so named is refused, never respelled"
    );
}

#[test]
fn the_type_names_a_cast_takes_first_answer_as_java() {
    let d = MongoDb::new();
    let names: Vec<(Scalar, &str)> = vec![
        (Scalar::Boolean, "bool"),
        (Scalar::Integer, "long"),
        (Scalar::Bigint, "long"),
        (Scalar::Double, "double"),
        (Scalar::Varchar, "string"),
        (Scalar::TemporalText, "string"),
        (Scalar::Date, "date"),
        (Scalar::Timestamp, "date"),
        (Scalar::Hugeint, "decimal"),
    ];
    for (scalar, name) in names {
        assert_eq!(d.scalar_type_name(scalar).unwrap(), name, "{scalar}");
    }
    assert_eq!(
        refused(d.scalar_type_name(Scalar::Timestamptz)),
        (
            Refusal::DIALECT_CAPABILITY,
            "a cast to a zoned timestamp reached MongoDb, whose dates are UTC instants".into()
        )
    );
    assert_eq!(
        refused(d.scalar_type_name(Scalar::Time)),
        (Refusal::DIALECT_CAPABILITY, "a cast to a time of day reached MongoDb, which has no time type".into())
    );
    assert_eq!(
        refused(d.scalar_type_name(Scalar::Json)),
        (Refusal::ILLEGAL_STATE, "a cast to JSON is the value itself on MongoDb".into())
    );
    assert_eq!(
        refused(d.scalar_type_name(Scalar::DecimalText)),
        (Refusal::DIALECT_CAPABILITY, "a cast to a decimal's text reached MongoDb".into())
    );
    assert_eq!(
        refused(d.struct_type_name(&[])),
        (Refusal::DIALECT_CAPABILITY, "a cast to a struct type reached MongoDb, whose documents are untyped".into())
    );
    // a cast to BOOLEAN reads a text by its word (variantAwareCast's own arm), never by the type name
    let cast = build::cast(s(), t(Scalar::Boolean));
    assert!(expr_text(&d, &cast).contains("{\"$toLower\": \"$$g0\"}, \"true\"]}"));
}

#[test]
fn ddl_type_spells_every_declared_column_though_no_ddl_writes_one() {
    let d = MongoDb::new();
    let plain = |kind| d.ddl_type(&ColumnType::Plain { kind });
    let spelled = [
        (ColumnKind::Bigint, "long"),
        (ColumnKind::Integer, "long"),
        (ColumnKind::Smallint, "long"),
        (ColumnKind::Tinyint, "long"),
        (ColumnKind::Float, "double"),
        (ColumnKind::Double, "double"),
        (ColumnKind::Real, "double"),
        (ColumnKind::Bit, "bool"),
        (ColumnKind::Timestamp, "date"),
        (ColumnKind::Date, "date"),
        (ColumnKind::Varchar, "string"),
        (ColumnKind::Json, "object"),
        (ColumnKind::Object, "object"),
        (ColumnKind::Array, "array"),
        (ColumnKind::Other, "objectId"),
    ];
    for (kind, name) in spelled {
        assert_eq!(plain(kind).unwrap(), name, "{kind}");
    }
    assert_eq!(
        refused(plain(ColumnKind::Time)),
        (Refusal::DIALECT_CAPABILITY, "a TIME column: MongoDB has no time-of-day type".into())
    );
    assert_eq!(
        refused(plain(ColumnKind::TimestampNs)),
        (Refusal::DIALECT_CAPABILITY, "a TIMESTAMP_NS column: MongoDB's dates keep milliseconds".into())
    );
    assert_eq!(
        refused(plain(ColumnKind::Distinct)),
        (Refusal::ILLEGAL_STATE, "no DDL spelling for declared column type DISTINCT".into())
    );
    assert_eq!(d.ddl_type(&ColumnType::Sized { kind: "VARCHAR".into(), size: 20 }).unwrap(), "string");
    assert_eq!(d.ddl_type(&ColumnType::Scaled { kind: "DECIMAL".into(), precision: 10, scale: 2 }).unwrap(), "decimal");
    // a collection declares no columns: a TIME column's table is created whole
    let create = SqlDdl::CreateTable {
        schema: Some("default".into()),
        table: "T".into(),
        quoting: NameQuoting::default(),
        columns: vec![lite_sql::DdlColumn {
            name: "c".into(),
            declared_quoted: false,
            r#type: ColumnType::Plain { kind: ColumnKind::Time },
            not_null: false,
            primary_key: false,
        }],
        temporary: false,
    };
    assert_eq!(AnsiSqlRenderer::render_ddl(&d, &create).unwrap(), "{\"create\": \"T\"}");
}

#[test]
fn a_nested_query_and_a_flat_join_are_walls_no_query_reaches() {
    let d = MongoDb::new();
    let q = build::query(select(vec![proj(n(), "n")], t0()));
    let mut w = SqlWriter::new();
    assert_eq!(
        refused(d.query(&mut w, &q, 1)),
        (
            Refusal::DIALECT_CAPABILITY,
            "a nested query reached MongoDb as a subquery: only a relation's own pipeline is written".into()
        )
    );
    let on = call(SqlFn::Equal, vec![n(), n()]);
    let join =
        build::join(t0(), table("t1", vec![out("n", t(Scalar::Bigint))]), lite_sql::JoinKind::Inner, Some(on)).unwrap();
    d.reset();
    let mut w = SqlWriter::new();
    assert_eq!(
        refused(pipeline::flat_source(&d, &mut w, &join)),
        (
            Refusal::DIALECT_CAPABILITY,
            "a join on the right side of a join reached MongoDb: a lookup over a joined source has no shape yet".into()
        )
    );
}

#[test]
fn reduce_array_counts_by_size_where_array_reduction_takes_count_first() {
    let d = MongoDb::new();
    d.reset();
    let mut w = SqlWriter::new();
    aggregates::reduce_array(&d, &mut w, AggFn::Count, &[], false).unwrap();
    assert_eq!(w.into_text().unwrap(), "{\"$size\": \"$$a\"}");
    let mut w = SqlWriter::new();
    aggregates::array_reduction(&d, &mut w, AggFn::Count, &text("\"$xs\""), &[], false).unwrap();
    assert_eq!(w.into_text().unwrap(), "{\"$let\": {\"vars\": {\"a\": \"$xs\"}, \"in\": {\"$size\": \"$$a\"}}}");
}

#[test]
fn the_float_forms_are_the_base_compositions() {
    // expanded before every pass, so no oracle line reaches them: the base's portable decompositions, written here
    let d = MongoDb::new();
    let x = col("d", t(Scalar::Double));
    let canon = call(SqlFn::FloatCanon, vec![x.clone()]);
    assert_eq!(expr_text(&d, &canon), expr_text(&d, &crate::float_text::canon_of(x.clone())));
    let print = call(SqlFn::FloatPrint, vec![x.clone()]);
    assert_eq!(expr_text(&d, &print), expr_text(&d, &crate::float_text::print_of(x)));
}

#[test]
fn a_window_over_a_grouped_aggregate_is_javas_class_cast() {
    // the arms oracle dropped Java's answer (its message names the JVM's class loader): the group's rewrite replaces
    // the window's reducer by its marker, which no window function is
    let inner = SqlExpr::Reducer(build::reducer(AggFn::Sum, vec![n()], false, Vec::new()));
    let sum = build::reducer(AggFn::Sum, vec![inner], false, Vec::new());
    let window = build::window_call(SqlAgg::Reducer(sum), Vec::new(), Vec::new(), None);
    let mut q = select(vec![proj(s(), "s"), proj(window, "v")], t0());
    q.group_by = vec![s()];
    let r = refused(AnsiSqlRenderer::render_statement(&MongoDb::new(), build::query(q)));
    assert_eq!(
        r,
        (
            Refusal::CLASS_CAST,
            "class com.legend.sql.SqlExpr$Column cannot be cast to class com.legend.sql.SqlAgg".to_owned()
        )
    );
}

#[test]
fn an_nth_value_of_one_argument_is_javas_index_out_of_bounds() {
    let nth = SqlAgg::ValueFn { r#fn: AggFn::NthValue, args: vec![n()] };
    let order = vec![build::sort_key(n(), true, None, None)];
    let window = build::window_call(nth, vec![s()], order, None);
    let q = select(vec![proj(window, "v")], t0());
    assert_eq!(
        refused(AnsiSqlRenderer::render_statement(&MongoDb::new(), build::query(q))),
        (Refusal::INDEX_OUT_OF_BOUNDS, "Index 1 out of bounds for length 1".to_owned())
    );
}

#[test]
fn a_source_of_no_fields_misses_its_comma_as_javas() {
    // Java's own text, reproduced: the `_id` exclusion's comma is written only before a field
    let q = select(vec![proj(int_lit(1), "one")], table("t0", Vec::new()));
    let text = AnsiSqlRenderer::render(&MongoDb::new(), build::query(q)).unwrap();
    assert!(text.contains("{\"$project\": {\"_id\": 0\"t0\": {}}}"), "{text}");
}

#[test]
fn the_insert_holds_values_alone() {
    let d = MongoDb::new();
    let insert = |cells: Vec<SqlExpr>| SqlDml::InsertValues {
        schema: None,
        table: "T".into(),
        quoting: NameQuoting::default(),
        columns: (0..cells.len()).map(|i| format!("c{i}")).collect(),
        quoted_columns: Vec::new(),
        rows: vec![cells],
    };
    let ok = AnsiSqlRenderer::render_dml(
        &d,
        &insert(vec![
            build::cast(string_lit("x"), t(Scalar::Varchar)),
            build::float_lit(2.5),
            build::decimal_lit(lite_sql::BigDecimal::parse("1E+2").unwrap()),
            SqlExpr::TimestampLit { iso: "2024-01-02 03:04:05.5".into(), r#type: TypeFact::Unknown },
        ]),
    )
    .unwrap();
    assert_eq!(
        ok,
        "{\"insert\": \"T\", \"documents\": [{\"c0\": \"x\", \"c1\": {\"$numberDouble\": \"2.5\"}, \"c2\": \
         {\"$numberDecimal\": \"100\"}, \"c3\": {\"$date\": \"2024-01-02T03:04:05.500Z\"}}]}"
    );
    // a Float's literal spelling is expanded before every pass, so a DML row (written with no pass) holds the call
    let canon = call(SqlFn::FloatCanon, vec![build::float_lit(1.0)]);
    assert_eq!(
        refused(AnsiSqlRenderer::render_dml(&d, &insert(vec![canon]))),
        (Refusal::DIALECT_CAPABILITY, "a row's cell is Call: a MongoDb insert holds values, not expressions".into())
    );
    let short = SqlDml::InsertValues {
        schema: None,
        table: "T".into(),
        quoting: NameQuoting::default(),
        columns: vec!["a".into(), "b".into()],
        quoted_columns: Vec::new(),
        rows: vec![vec![int_lit(1)]],
    };
    assert_eq!(
        refused(AnsiSqlRenderer::render_dml(&d, &short)),
        (Refusal::ILLEGAL_STATE, "a row of 1 values for 2 columns".into())
    );
}

#[test]
fn a_date_literal_is_a_bson_date_to_the_millisecond() {
    let date = |iso: &str| json::date_literal(iso);
    assert_eq!(date("2024-01-02").unwrap(), "{\"$date\": \"2024-01-02T00:00:00Z\"}");
    assert_eq!(date(" 2024-01-02 03:04:05.1 ").unwrap(), "{\"$date\": \"2024-01-02T03:04:05.100Z\"}");
    assert_eq!(date("2024-01-02T03:04:05.123000").unwrap(), "{\"$date\": \"2024-01-02T03:04:05.123Z\"}");
    assert_eq!(date("2024-01-02T03:04:05+05:30").unwrap(), "{\"$date\": \"2024-01-02T03:04:05+05:30\"}");
    assert_eq!(date("2024-01-02T03:04:05-0530").unwrap(), "{\"$date\": \"2024-01-02T03:04:05-0530\"}");
    assert_eq!(date("2024-01-02T03:04:05Z").unwrap(), "{\"$date\": \"2024-01-02T03:04:05Z\"}");
    // a line terminator before the offset: Java's `.` takes none, so the offset is not read and a Z is added
    assert_eq!(date("2024-01-02\n03:04+0530").unwrap(), "{\"$date\": \"2024-01-02\\n03:04+0530Z\"}");
    assert_eq!(
        refused(date("2024-01-02T03:04:05.1234")),
        (
            Refusal::DIALECT_CAPABILITY,
            "a date-time finer than a millisecond (2024-01-02T03:04:05.1234) has no BSON date: refused, never cut"
                .into()
        )
    );
}

#[test]
fn names_keys_and_strings_are_mongodbs() {
    assert_eq!(json::variable("_tds0"), "v__tds0");
    assert_eq!(json::variable("a-b"), "v_a_b");
    // a character outside ASCII is one UTF-16 unit replaced, a character past the BMP two
    assert_eq!(json::variable("é"), "v__");
    assert_eq!(json::variable("𝔸x"), "v___x");
    for bad in ["", "$x", "a.b", "a\0b"] {
        assert_eq!(refused(json::key(bad)).0, Refusal::DIALECT_CAPABILITY, "{bad:?}");
    }
    assert_eq!(json::string("a\"b\\c\n\r\t\u{1}"), "\"a\\\"b\\\\c\\n\\r\\t\\u0001\"");
    assert_eq!(json::path(&["t0", "n"]), "\"$t0.n\"");
    // a literal array is an operator's one argument; the numeric reducers keep it bare
    let d = MongoDb::new();
    d.reset();
    let list = build::array_lit(vec![int_lit(1)]);
    let mut w = SqlWriter::new();
    json::op1(&d, &mut w, "$reverseArray", &list).unwrap();
    assert_eq!(w.into_text().unwrap(), "{\"$reverseArray\": [[{\"$numberLong\": \"1\"}]]}");
    let mut w = SqlWriter::new();
    json::op1(&d, &mut w, "$sum", &list).unwrap();
    assert_eq!(w.into_text().unwrap(), "{\"$sum\": [{\"$numberLong\": \"1\"}]}");
}

#[test]
fn the_records_text_is_javas() {
    // as the oracle's refusals spell them
    assert_eq!(java_text::expr(&n()), N_TEXT);
    let date = SqlExpr::DateLit { iso: "2024-01-02".into(), r#type: crate::typing::scalar(Scalar::Date) };
    assert_eq!(java_text::expr(&date), "DateLit[iso=2024-01-02, type=Typed[type=DATE, nullable=false]]");
    let less = call(SqlFn::Less, vec![n(), col("d", t(Scalar::Double))]);
    assert_eq!(
        java_text::expr(&less),
        format!(
            "Call[fn=LESS, args=[{N_TEXT}, Column[table=t0, name=d, type=Typed[type=DOUBLE, nullable=true], \
             origin=PHYSICAL]], type=Typed[type=BOOLEAN, nullable=true]]"
        )
    );
    let lambda = build::lambda(&["x"], build::derived(None, "x"));
    assert_eq!(
        java_text::expr(&lambda),
        "Lambda[params=[x], body=Column[table=null, name=x, type=Unknown[], origin=DERIVED], type=Unknown[]]"
    );
    assert_eq!(java_text::strings(&["a".into(), "b".into()]), "[a, b]");
}

#[test]
fn a_marker_with_no_read_and_an_empty_scope_are_refused() {
    let d = MongoDb::new();
    assert_eq!(
        refused(expr_result(&d, &build::derived(None, "__agg0"))),
        (Refusal::ILLEGAL_STATE, "marker __agg0 has no read in this scope".into())
    );
    // a fresh dialect, before any render: Java's empty deque
    let mut w = SqlWriter::new();
    assert_eq!(refused(MongoDb::new().expr(&mut w, &n(), 0)).0, "java.util.NoSuchElementException");
}

#[test]
fn every_render_starts_its_scopes_over() {
    // the pipeline's scopes and the lambda variables are reset by each query render, so one dialect answers a
    // statement as a fresh one does, even after a render that failed inside a lookup
    let d = MongoDb::new();
    let inner = select(vec![proj(n(), "n")], table("t1", vec![out("n", t(Scalar::Bigint))]));
    // a join whose ON reads a column no field can be named: refused inside the lookup's scope, a sole alias set
    let dotted = SqlExpr::Column(Column {
        table: Some("t1".into()),
        name: "a.b".into(),
        r#type: TypeFact::Unknown,
        origin: Some(Origin::Physical),
    });
    let right = table("t1", vec![out("n", t(Scalar::Bigint))]);
    let on = call(SqlFn::Equal, vec![n(), dotted]);
    let failing = select(vec![proj(s(), "s")], build::join(t0(), right, lite_sql::JoinKind::Inner, Some(on)).unwrap());
    assert_eq!(refused(AnsiSqlRenderer::render(&d, build::query(failing))).0, Refusal::DIALECT_CAPABILITY);
    let q = || {
        let mut q = select(vec![proj(s(), "s")], t0());
        q.r#where = Some(build::in_subquery(n(), build::query(inner.clone())));
        build::query(q)
    };
    let reused = AnsiSqlRenderer::render(&d, q()).unwrap();
    assert_eq!(reused, AnsiSqlRenderer::render(&MongoDb::new(), q()).unwrap());
    assert!(reused.contains("\"$lookup\""), "{reused}");
}

#[test]
fn the_facts_and_the_pass_list_are_javas() {
    let d = MongoDb::new();
    assert!(!d.relational());
    assert!(!d.temporary_tables());
    assert_eq!(d.keys_snapshot().unwrap(), KeysSnapshot::None);
    assert_eq!(d.result_column_limit().unwrap(), None);
    let names: Vec<&str> = d.passes().iter().map(|p| p.name()).collect();
    assert_eq!(names, ["CarrierStrategies"]);
    let rendered: Vec<&str> = ansi::render_passes(&d).iter().map(|p| p.name()).collect();
    assert_eq!(rendered, ["FloatPrintText.Expansion", "FloatCanonText.Expansion", "CarrierStrategies", "StoredReads"]);
    // a stored objectId is read as its hex text; nothing else is
    assert!(<MongoDb as AnsiSqlRenderer>::reads_stored(&ColumnType::Plain { kind: ColumnKind::Other }));
    assert!(!<MongoDb as AnsiSqlRenderer>::reads_stored(&ColumnType::Plain { kind: ColumnKind::Distinct }));
    let column = Column { table: Some("t0".into()), name: "_id".into(), r#type: TypeFact::Unknown, origin: None };
    d.reset();
    let mut w = SqlWriter::new();
    d.stored_read(&mut w, &column, &ColumnType::Plain { kind: ColumnKind::Other }).unwrap();
    assert_eq!(w.into_text().unwrap(), "{\"$toString\": \"$t0._id\"}");
}
