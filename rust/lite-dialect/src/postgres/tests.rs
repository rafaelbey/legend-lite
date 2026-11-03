// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Postgres's hooks held to their Java (`Postgres.java`) and to the texts core's own Postgres tests pin
//! (`PostgresDialectTest`, `PostgresCatalogTest`, `TempTableAsTest`, `NullVariantCastTest`): each spelling and refusal
//! on a hand-built node -- the arms the golden oracles hold none of first among them (`LIST_DISTINCT`, `LIST_INIT`,
//! the list Boolean reductions, `REPEAT_VALUE`, `STRUCT_INSERT`, the JSON length and pretty-print, the epoch
//! conversions, the day and month names, every wall, the refusals of a shape Postgres cannot spell), and
//! `fromEngineText`, which the oracles hold no line of. The oracles hold the rest (`tests/dialect_differential.rs`).

use super::*;
use crate::SqlDialect;
use crate::build::{call, cast, float_lit, int_lit, null_lit, string_lit};
use crate::catalog_model::Read;
use lite_sql::{CatalogGrid, JavaNumber, OutputCol, SqlSelect, SqlSource};

// ---- builders

fn typed(s: Scalar) -> TypeFact {
    TypeFact::Typed { r#type: SqlType::Scalar(s), nullable: true }
}

fn list_of(s: Scalar) -> TypeFact {
    TypeFact::Typed { r#type: SqlType::Array { element: Box::new(SqlType::Scalar(s)) }, nullable: true }
}

fn column(name: &str, t: TypeFact, origin: Option<Origin>) -> SqlExpr {
    SqlExpr::Column(Column { table: Some("t0".into()), name: name.into(), r#type: t, origin })
}

/// A physical column of `t0`, typed.
fn col(name: &str, s: Scalar) -> SqlExpr {
    column(name, typed(s), Some(Origin::Physical))
}

/// A physical list column of `t0`, its elements typed.
fn list(name: &str, s: Scalar) -> SqlExpr {
    column(name, list_of(s), Some(Origin::Physical))
}

fn part(p: DatePart) -> SqlExpr {
    SqlExpr::DatePartLit { part: p, r#type: TypeFact::Unknown }
}

fn lambda(params: &[&str], body: SqlExpr) -> SqlExpr {
    crate::build::lambda(params, body)
}

fn render_result(e: &SqlExpr) -> Result<String, Refusal> {
    let mut w = SqlWriter::new();
    Postgres::new().expr(&mut w, e, 0)?;
    w.into_text()
}

fn render(e: &SqlExpr) -> String {
    render_result(e).unwrap_or_else(|r| panic!("{r}"))
}

fn refusal(e: &SqlExpr) -> Refusal {
    render_result(e).unwrap_err()
}

fn capability(message: &str) -> Refusal {
    Refusal::capability(message)
}

fn reducer(f: AggFn, args: Vec<SqlExpr>, distinct: bool, order_by: Vec<SortKey>) -> SqlExpr {
    SqlExpr::Reducer(crate::build::reducer(f, args, distinct, order_by))
}

fn key(e: SqlExpr, ascending: bool) -> SortKey {
    SortKey { expr: e, ascending, null_order: None, output_name: None }
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

fn projection(expr: SqlExpr, alias: Option<&str>, out: Option<(&str, SqlType)>) -> Projection {
    Projection {
        expr,
        alias: alias.map(str::to_owned),
        out: out.map(|(name, t)| OutputCol { name: name.into(), r#type: t, nullable: true, origin: Origin::Derived }),
    }
}

fn written(p: &Projection) -> Result<String, Refusal> {
    let mut w = SqlWriter::new();
    Postgres::new().projection(&mut w, p)?;
    w.into_text()
}

// ---- names

#[test]
fn every_name_the_query_writes_is_quoted_and_a_stores_bare_name_is_folded() {
    let d = Postgres::new();
    // an alias or a derived label keeps the case it is written in
    assert_eq!(d.alias_ident("id"), "\"id\"");
    assert_eq!(d.alias_ident("Id"), "\"Id\"");
    assert_eq!(d.alias_ident("\"x\""), "\"x\"");
    assert_eq!(d.alias_ident("a\"b"), "\"a\"\"b\"");
    // a lone quote, or one inside a quoted name, is no pre-quoted name
    assert_eq!(d.alias_ident("\""), "\"\"\"\"");
    assert_eq!(d.alias_ident("\"a\"b\""), "\"\"\"a\"\"b\"\"\"");
    // a store's name: folded, bare where it may be, delimited where it must be (a keyword, a non-word)
    assert_eq!(AnsiSqlRenderer::physical_name(&d, "TRADES"), "trades");
    assert_eq!(AnsiSqlRenderer::physical_name(&d, "Order"), "\"order\"");
    assert_eq!(AnsiSqlRenderer::physical_name(&d, "first name"), "\"first name\"");
    assert_eq!(AnsiSqlRenderer::physical_name(&d, "a$b"), "\"a$b\"");
    assert_eq!(AnsiSqlRenderer::physical_name(&d, "_x9"), "_x9");
    assert_eq!(AnsiSqlRenderer::physical_name(&d, "\"firstName\""), "\"firstName\"");
    // declared quoted keeps its case; a DDL column declared bare is a physical name
    assert_eq!(AnsiSqlRenderer::stored_name(&d, "Orders", true), "\"Orders\"");
    assert_eq!(AnsiSqlRenderer::stored_name(&d, "Orders", false), "orders");
    assert_eq!(AnsiSqlRenderer::insert_column_name(&d, "Amt", true), "\"Amt\"");
    assert_eq!(AnsiSqlRenderer::insert_column_name(&d, "ORDER", false), "\"order\"");
    // each part of a stored table a stored name (TempTableAsTest: the staged table a query reads)
    let bare = NameQuoting::default();
    let quoted = NameQuoting { schema: true, table: true };
    assert_eq!(AnsiSqlRenderer::table_name(&d, "legend_stage_rowsA", bare), "legend_stage_rowsa");
    assert_eq!(AnsiSqlRenderer::table_name(&d, "Sales.Orders", quoted), "\"Sales\".\"Orders\"");
    assert_eq!(AnsiSqlRenderer::table_name(&d, "s.User", bare), "s.\"user\"");
    // a leading dot names no schema (Java's `dot <= 0`): the whole name one stored name
    assert_eq!(AnsiSqlRenderer::table_name(&d, ".T", bare), "\".t\"");
    // a catalog name a bare declaration reaches is its own fold
    assert_eq!(d.bare_name_reaches("orders"), Ok(true));
    assert_eq!(d.bare_name_reaches("Orders"), Ok(false));
    assert_eq!(d.star_except_name("v"), "\"v\"");
}

#[test]
fn a_reference_spells_by_its_origin() {
    assert_eq!(render(&col("SYM", Scalar::Varchar)), "\"t0\".sym");
    assert_eq!(render(&col("user", Scalar::Varchar)), "\"t0\".\"user\"");
    assert_eq!(render(&column("Sym", typed(Scalar::Varchar), Some(Origin::PhysicalQuoted))), "\"t0\".\"Sym\"");
    assert_eq!(render(&column("sym", typed(Scalar::Varchar), Some(Origin::Derived))), "\"t0\".\"sym\"");
    assert_eq!(render(&column("sym", typed(Scalar::Varchar), None)), "\"t0\".\"sym\"");
    assert_eq!(
        refusal(&SqlExpr::RowOrder { table: None, r#type: typed(Scalar::Bigint) }),
        capability("row order reached Postgres, which has no stable row-order column (ctid moves on update)")
    );
    let except = SqlExpr::StarExcept { table: None, except: vec!["a".into()], r#type: TypeFact::Unknown };
    assert_eq!(
        refusal(&except),
        capability(
            "SELECT * EXCLUDE reached Postgres, which has no star exclusion; the column list must be expanded upstream"
        )
    );
}

// ---- literals

#[test]
fn literals_postgres_cannot_hold_are_refused_and_finer_digits_cut() {
    let d = Postgres::new();
    assert_eq!(d.string_lit("it's"), Ok("'it''s'".to_owned()));
    assert_eq!(
        d.string_lit("a\0b"),
        Err(capability("a string holding NUL reached Postgres, whose text type cannot hold it"))
    );
    // finer digits than a microsecond truncated (PostgresDialectTest: the 9999-12-31 sentinel)
    assert_eq!(d.timestamp_lit("9999-12-31T23:59:59.999999999"), "TIMESTAMP '9999-12-31T23:59:59.999999'");
    assert_eq!(d.timestamp_lit("2024-01-02T03:04:05.123456789+01:00"), "TIMESTAMP '2024-01-02T03:04:05.123456+01:00'");
    assert_eq!(d.timestamp_lit("2024-01-02T03:04:05.123"), "TIMESTAMP '2024-01-02T03:04:05.123'");
    assert_eq!(d.timestamp_lit("2024-01-02"), "TIMESTAMP '2024-01-02'");
    assert_eq!(
        d.hole_type(ValueKind::DateTimeNanos),
        Ok(TypeSpelling { name: "TIMESTAMP".into(), digits: Digits::None, fraction: Some(6) })
    );
    assert_eq!(d.hole_type(ValueKind::DateTime), Ok(TypeSpelling::new("TIMESTAMP", Digits::None)));
    // a list literal is typed, its elements held as the list holds them
    let ints = SqlExpr::ArrayLit { elements: vec![int_lit(1), int_lit(2)], r#type: list_of(Scalar::Bigint) };
    assert_eq!(render(&ints), "CAST(ARRAY[1, 2] AS BIGINT[])");
    let nested = SqlExpr::ArrayLit {
        elements: vec![SqlExpr::ArrayLit { elements: vec![int_lit(1)], r#type: list_of(Scalar::Bigint) }],
        r#type: TypeFact::Typed {
            r#type: SqlType::Array { element: Box::new(list_of_type(Scalar::Bigint)) },
            nullable: true,
        },
    };
    assert_eq!(render(&nested), "CAST(ARRAY[to_jsonb(CAST(ARRAY[1] AS BIGINT[]))] AS JSONB[])");
    assert_eq!(
        refusal(&SqlExpr::ArrayLit { elements: vec![], r#type: TypeFact::Unknown }),
        capability("a list literal of Unknown[] reached Postgres: a list of unknown element type")
    );
}

fn list_of_type(s: Scalar) -> SqlType {
    SqlType::Array { element: Box::new(SqlType::Scalar(s)) }
}

// ---- the calls the oracles hold none of

#[test]
fn the_list_calls_the_oracles_do_not_hold() {
    let xs = list("xs", Scalar::Bigint);
    assert_eq!(
        render(&call(SqlFn::ListDistinct, vec![xs.clone()])),
        "(CASE WHEN \"t0\".xs IS NULL THEN NULL ELSE ARRAY(SELECT \"__x\" FROM unnest((SELECT \"t0\".xs)) WITH \
         ORDINALITY AS __u(\"__x\", __o) WHERE \"__x\" IS NOT NULL GROUP BY \"__x\" ORDER BY min(__o)) END)"
    );
    assert_eq!(render(&call(SqlFn::ListInit, vec![xs.clone()])), "(\"t0\".xs)[:cardinality(\"t0\".xs) - 1]");
    assert_eq!(render(&call(SqlFn::ListTail, vec![xs.clone()])), "(\"t0\".xs)[2:]");
    let bs = list("bs", Scalar::Boolean);
    assert_eq!(
        render(&call(SqlFn::ListBoolAnd, vec![bs.clone()])),
        "(SELECT bool_and(\"__x\") FROM unnest((SELECT \"t0\".bs)) AS __u(\"__x\"))"
    );
    assert_eq!(
        render(&call(SqlFn::ListBoolOr, vec![bs.clone()])),
        "(SELECT bool_or(\"__x\") FROM unnest((SELECT \"t0\".bs)) AS __u(\"__x\"))"
    );
    // a Boolean list's extremes: false < true, and Postgres has no max(boolean)
    assert_eq!(
        render(&call(SqlFn::ListMax, vec![bs])),
        "(SELECT bool_or(\"__x\") FROM unnest((SELECT \"t0\".bs)) AS __u(\"__x\"))"
    );
    assert_eq!(
        render(&call(SqlFn::RepeatValue, vec![string_lit("a"), int_lit(3)])),
        "array_fill('a', ARRAY[CAST(3 AS INTEGER)])"
    );
    assert_eq!(render(&call(SqlFn::ListAppend, vec![xs.clone(), int_lit(4)])), "array_append(\"t0\".xs, 4)");
    // a list of no known element type is refused by name, at once
    let untyped = column("u", TypeFact::Unknown, Some(Origin::Physical));
    assert_eq!(
        refusal(&call(SqlFn::ListLength, vec![untyped.clone()])),
        capability("LIST_LENGTH over Unknown[] reached Postgres: a list of unknown element type")
    );
    // a filter's lambda: the kept element, ordered by position
    let filter = call(
        SqlFn::ListFilter,
        vec![xs.clone(), lambda(&["x"], call(SqlFn::Greater, vec![crate::build::derived(None, "x"), int_lit(1)]))],
    );
    assert_eq!(
        render(&filter),
        "(CASE WHEN \"t0\".xs IS NULL THEN NULL ELSE ARRAY(SELECT \"x\" FROM unnest((SELECT \"t0\".xs)) WITH \
         ORDINALITY AS __u(\"x\", __o) WHERE \"x\" > 1 ORDER BY __o) END)"
    );
    // a lambda argument that is no lambda: Java's cast
    assert_eq!(refusal(&call(SqlFn::ListFilter, vec![xs.clone(), int_lit(1)])).class, Refusal::CLASS_CAST);
    // the empty collection: an element or a rearrangement of it, which the IR types Bottom, is the empty collection
    // itself; nested too
    let empty = call(SqlFn::ListMax, vec![null_lit()]);
    assert_eq!(empty.r#type(), &TypeFact::Bottom);
    assert_eq!(render(&empty), "NULL");
    assert_eq!(render(&call(SqlFn::ListGet, vec![crate::build::compact_list(empty), int_lit(1)])), "NULL");
    // a sum over it is typed Unknown, so it is no empty collection: its list of no element type is refused by name
    assert_eq!(
        refusal(&call(SqlFn::ListSum, vec![null_lit()])),
        capability("LIST_SUM over Bottom[] reached Postgres: a list of unknown element type")
    );
    // exists over the empty collection is false, forAll true
    let pred = lambda(&["x"], crate::build::bool_lit(true));
    assert_eq!(render(&call(SqlFn::ListExists, vec![null_lit(), pred.clone()])), "FALSE");
    assert_eq!(render(&call(SqlFn::ListForAll, vec![null_lit(), pred])), "TRUE");
    assert_eq!(render(&call(SqlFn::Unnest, vec![null_lit()])), "unnest(CAST(NULL AS TEXT[]))");
}

#[test]
fn the_scalar_calls_the_oracles_do_not_hold() {
    let s = col("s", Scalar::Varchar);
    let ts = col("ts", Scalar::Timestamp);
    let d = col("d", Scalar::Date);
    let j = col("j", Scalar::Json);
    assert_eq!(
        render(&call(SqlFn::StructInsert, vec![j.clone(), string_lit("k"), int_lit(1)])),
        "(\"t0\".j || jsonb_build_object('k', 1))"
    );
    assert_eq!(render(&call(SqlFn::JsonArrayLength, vec![j.clone()])), "jsonb_array_length(\"t0\".j)");
    assert_eq!(render(&call(SqlFn::JsonPretty, vec![j])), "jsonb_pretty(\"t0\".j)");
    assert_eq!(
        render(&call(SqlFn::EpochSeconds, vec![ts.clone()])),
        "CAST(extract(epoch FROM \"t0\".ts) AS DOUBLE PRECISION)"
    );
    assert_eq!(
        render(&call(SqlFn::FromEpochSeconds, vec![col("n", Scalar::Bigint)])),
        "TIMESTAMP '1970-01-01 00:00:00' + \"t0\".n * INTERVAL '1 second'"
    );
    // wrapped under an operator
    let plus = call(SqlFn::Plus, vec![int_lit(1), call(SqlFn::FromEpochMs, vec![col("n", Scalar::Bigint)])]);
    assert_eq!(
        render(&plus),
        "1 + (TIMESTAMP '1970-01-01 00:00:00' + CAST(\"t0\".n AS BIGINT) * INTERVAL '1 millisecond')"
    );
    // a date is read as a naive timestamp, a timestamp as itself
    assert_eq!(render(&call(SqlFn::Dayname, vec![d.clone()])), "to_char(CAST(\"t0\".d AS TIMESTAMP), 'FMDay')");
    assert_eq!(render(&call(SqlFn::Monthname, vec![ts])), "to_char(\"t0\".ts, 'FMMonth')");
    assert_eq!(render(&call(SqlFn::ConcatJoin, vec![s.clone(), string_lit(",")])), "concat(\"t0\".s, ',')");
    assert_eq!(render(&call(SqlFn::FloorRaw, vec![col("f", Scalar::Double)])), "floor(\"t0\".f)");
    assert_eq!(render(&call(SqlFn::Chr, vec![col("n", Scalar::Bigint)])), "chr(CAST(\"t0\".n AS INTEGER))");
    assert_eq!(render(&call(SqlFn::BitNot, vec![int_lit(5)])), "(~ CAST(5 AS BIGINT))");
    assert_eq!(
        render(&call(SqlFn::BitShiftRight, vec![col("n", Scalar::Bigint), int_lit(2)])),
        "(CAST(\"t0\".n AS BIGINT) >> CAST(2 AS INTEGER))"
    );
    assert_eq!(
        render(&call(SqlFn::BitXor, vec![col("n", Scalar::Bigint), int_lit(2)])),
        "(CAST(\"t0\".n AS BIGINT) # CAST(2 AS BIGINT))"
    );
    // a split of the empty text is [''], as DuckDB's
    assert_eq!(
        render(&call(SqlFn::Split, vec![s.clone(), string_lit(",")])),
        "(CASE WHEN \"t0\".s = '' THEN ARRAY[''] ELSE string_to_array(\"t0\".s, ',') END)"
    );
    // Pure's splitPart: the non-empty tokens, 1-based
    assert_eq!(
        render(&call(SqlFn::PureSplitPart, vec![s.clone(), string_lit(","), int_lit(2)])),
        "(array_remove(string_to_array(\"t0\".s, ','), ''))[2]"
    );
    // a computed text is no literal format
    assert_eq!(
        refusal(&call(SqlFn::Strftime, vec![d, s])),
        capability("a date format that is not a format literal reached Postgres: its codes are DuckDB's")
    );
}

#[test]
fn every_wall_is_refused_by_name() {
    let x = col("x", Scalar::Varchar);
    for (f, why) in [
        (SqlFn::Strptime, "to_timestamp(text, fmt) is lenient and zone-bound; not yet probed against the reference"),
        (SqlFn::JsonMergePatch, "variant over jsonb is leg P4"),
        (SqlFn::ListFlatten, "collections over the jsonb carrier are leg P4"),
        (SqlFn::ListZip, "collections over the jsonb carrier are leg P4"),
        (SqlFn::ListProduct, "collections over the jsonb carrier are leg P4"),
        (SqlFn::Typeof, "collections over the jsonb carrier are leg P4"),
    ] {
        assert_eq!(refusal(&call(f, vec![x.clone()])), capability(&format!("{f} reached Postgres: {why}")));
    }
    for f in [SqlFn::EpochNs, SqlFn::FromEpochNs] {
        assert_eq!(
            refusal(&call(f, vec![x.clone()])),
            capability(
                "a nanosecond epoch: Postgres keeps microseconds (the Cypher island's nanosecond leg is DuckDB's)"
            )
        );
    }
    let f8 = col("f", Scalar::Double);
    assert_eq!(
        refusal(&call(SqlFn::Mod, vec![f8.clone(), int_lit(2)])),
        capability("MOD over a Float reached Postgres, which has no double-precision floored remainder")
    );
    assert_eq!(
        refusal(&call(SqlFn::Rem, vec![call(SqlFn::Random, vec![]), f8.clone()])),
        capability(
            "REM over a Float of random() or a uuid reached Postgres: its exact remainder writes each operand where it \
             reads it, which would draw it again"
        )
    );
    assert_eq!(
        refusal(&call(SqlFn::MakeTimestamp, vec![int_lit(1); 3])),
        capability("make_timestamp of 3 arguments has no Postgres spelling")
    );
    let ts = col("ts", Scalar::Timestamp);
    assert_eq!(
        refusal(&call(SqlFn::Extract, vec![part(DatePart::Millisecond), ts.clone()])),
        capability("date part 'millisecond' has no probed Postgres spelling")
    );
    assert_eq!(
        refusal(&call(SqlFn::Extract, vec![part(DatePart::Nanosecond), ts.clone()])),
        capability("a nanosecond part: Postgres keeps microseconds (the Cypher island's nanosecond leg is DuckDB's)")
    );
    assert_eq!(
        refusal(&call(SqlFn::DateTrunc, vec![part(DatePart::DayOfYear), ts.clone()])),
        capability("date_trunc part 'doy' has no probed Postgres spelling")
    );
    // a week's date_diff is DuckDB's day count / 7, unprobed: a wall, raised before its operands are read
    assert_eq!(
        refusal(&call(SqlFn::DateDiff, vec![part(DatePart::Week)])),
        capability("date_diff part 'week' has no probed Postgres spelling")
    );
    assert_eq!(
        refusal(&call(SqlFn::AddInterval, vec![part(DatePart::Quarter), int_lit(1), ts.clone()])),
        capability("interval unit 'QUARTER' has no Postgres spelling")
    );
    assert_eq!(
        refusal(&call(SqlFn::Format, vec![x.clone(), int_lit(1)])),
        capability("a computed format reached Postgres, which has no printf: only a literal one is unrolled")
    );
    assert_eq!(
        refusal(&call(SqlFn::Format, vec![string_lit("%x")])),
        capability("a format directive other than %s, %d and %f reached Postgres: %x")
    );
    assert_eq!(
        refusal(&call(SqlFn::Format, vec![string_lit("%s %s"), x.clone()])),
        capability("a format with more directives than arguments reached Postgres")
    );
    assert_eq!(
        refusal(&call(SqlFn::Format, vec![string_lit("%s"), x.clone(), x.clone()])),
        capability("a format with more arguments than directives reached Postgres")
    );
    assert_eq!(
        refusal(&call(SqlFn::ToVariant, vec![x.clone(), x.clone()])),
        capability("toVariant of 2 arguments reached Postgres")
    );
    assert_eq!(
        refusal(&call(SqlFn::MapKeys, vec![x.clone()])),
        capability("MAP_KEYS reached Postgres: a map of unknown key and value types")
    );
    assert_eq!(
        refusal(&call(SqlFn::MapFromEntries, vec![list("e", Scalar::Bigint)])),
        capability("MAP_FROM_ENTRIES reached Postgres: entries of BIGINT")
    );
    assert_eq!(
        refusal(&call(SqlFn::MapFromEntries, vec![x])),
        capability("MAP_FROM_ENTRIES reached Postgres: entries of null")
    );
}

#[test]
fn format_unrolls_a_literal_template() {
    let n = col("n", Scalar::Bigint);
    let s = col("s", Scalar::Varchar);
    assert_eq!(render(&call(SqlFn::Format, vec![string_lit("100%%")])), "('100%')");
    assert_eq!(render(&call(SqlFn::Format, vec![string_lit("")])), "('')");
    assert_eq!(
        render(&call(SqlFn::Format, vec![string_lit("a %s b %d"), s.clone(), n.clone()])),
        "('a ' || \"t0\".s || ' b ' || CAST(\"t0\".n AS VARCHAR))"
    );
    // a width pads on the left, never cuts; a 0 pads with zeros after the sign
    assert_eq!(
        render(&call(SqlFn::Format, vec![string_lit("%3s"), s])),
        "(lpad(\"t0\".s, greatest(3, length(\"t0\".s)), ' '))"
    );
    assert_eq!(
        render(&call(SqlFn::Format, vec![string_lit("%03d"), n.clone()])),
        "((CASE WHEN \"t0\".n < 0 THEN '-' || lpad(CAST(abs(\"t0\".n) AS VARCHAR), greatest(2, \
         length(CAST(abs(\"t0\".n) AS VARCHAR))), '0') ELSE lpad(CAST(abs(\"t0\".n) AS VARCHAR), greatest(3, \
         length(CAST(abs(\"t0\".n) AS VARCHAR))), '0') END))"
    );
    // %.Nf rounds the value's decimal text half-even; a negative that rounds to zero keeps its sign
    let rounded = "(SELECT round(CASE WHEN __hv - floor(__hv) = 0.5 THEN (CASE WHEN mod(floor(__hv), 2) = 0 THEN \
                   floor(__hv) ELSE floor(__hv) + 1 END) ELSE round(__hv) END / 100, 2) FROM (SELECT \
                   CAST(CAST(\"t0\".n AS TEXT) AS NUMERIC) * 100 AS __hv) AS __he)";
    assert_eq!(
        render(&call(SqlFn::Format, vec![string_lit("%.2f!"), n])),
        format!(
            "((CASE WHEN \"t0\".n < 0 AND {rounded} = 0 THEN '-' ELSE '' END || CAST({rounded} AS VARCHAR)) || '!')"
        )
    );
}

#[test]
fn a_pattern_is_postgres_own_flavour_its_re2_flags_its_options() {
    let s = col("s", Scalar::Varchar);
    let m = |p: SqlExpr| render(&call(SqlFn::Matches, vec![s.clone(), p]));
    assert_eq!(m(string_lit("a.b")), "(\"t0\".s ~ '(?p)a.b')");
    assert_eq!(m(string_lit("(?i)a")), "(\"t0\".s ~ '(?ip)a')");
    assert_eq!(m(string_lit("(?ms)a")), "(\"t0\".s ~ '(?w)a')");
    assert_eq!(m(string_lit("(?m)a")), "(\"t0\".s ~ '(?n)a')");
    assert_eq!(m(string_lit("(?s)a")), "(\"t0\".s ~ '(?s)a')");
    // a pattern computed at run time: its flags taken off a literal prefix
    assert_eq!(m(s.clone()), "(\"t0\".s ~ ('(?p)' || \"t0\".s))");
    let flagged = call(SqlFn::Concat, vec![string_lit("(?i)"), s.clone()]);
    assert_eq!(m(flagged), "(\"t0\".s ~ ('(?ip)' || \"t0\".s))");
    assert_eq!(
        render(&call(SqlFn::RegexpFullMatch, vec![s.clone(), s.clone()])),
        "(\"t0\".s ~ ('(?p)^(?:' || \"t0\".s || ')$'))"
    );
}

#[test]
fn a_floats_remainder_is_exact_and_rounding_is_half_even() {
    let f8 = col("f", Scalar::Double);
    let n = col("n", Scalar::Bigint);
    // integers take the base's MOD forms
    assert_eq!(render(&call(SqlFn::Rem, vec![n.clone(), int_lit(3)])), "MOD(\"t0\".n, 3)");
    let rem = render(&call(SqlFn::Rem, vec![f8.clone(), float_lit(1.5)]));
    assert!(
        rem.starts_with(
            "(CASE WHEN CAST(\"t0\".f AS DOUBLE PRECISION) IS NULL OR CAST(1.5 AS DOUBLE PRECISION) IS NULL"
        ),
        "{rem}"
    );
    assert!(rem.contains("power(CAST(2 AS NUMERIC), greatest(CASE WHEN abs("), "{rem}");
    assert_eq!(render(&call(SqlFn::Round, vec![f8.clone()])), "round(CAST(\"t0\".f AS DOUBLE PRECISION))");
    assert_eq!(
        render(&call(SqlFn::Round, vec![f8.clone(), int_lit(2)])),
        "(round(CAST(\"t0\".f AS DOUBLE PRECISION) * power(CAST(10 AS DOUBLE PRECISION), CAST(2 AS INTEGER))) / \
         power(CAST(10 AS DOUBLE PRECISION), CAST(2 AS INTEGER)))"
    );
    let v = "(CAST(\"t0\".n AS NUMERIC) * power(CAST(10 AS NUMERIC), CAST(1 AS INTEGER)))";
    assert_eq!(
        render(&call(SqlFn::Round, vec![n.clone(), int_lit(1)])),
        format!(
            "(CASE WHEN {v} - floor({v}) = 0.5 THEN (CASE WHEN mod(floor({v}), 2) = 0 THEN floor({v}) ELSE \
             floor({v}) + 1 END) ELSE round({v}) END / power(CAST(10 AS NUMERIC), CAST(1 AS INTEGER)))"
        )
    );
    // HALF_UP over numeric, a double cast back
    assert_eq!(
        render(&call(SqlFn::RoundHalfUp, vec![f8, int_lit(2)])),
        "CAST(round(CAST(\"t0\".f AS NUMERIC), CAST(2 AS INTEGER)) AS DOUBLE PRECISION)"
    );
    assert_eq!(render(&call(SqlFn::RoundHalfUp, vec![n])), "round(CAST(\"t0\".n AS NUMERIC))");
}

// ---- aggregates and windows

fn agg(r: SqlExpr) -> Result<String, Refusal> {
    render_result(&r)
}

#[test]
fn every_aggregate_is_decided_here() {
    let n = col("n", Scalar::Bigint);
    let b = col("b", Scalar::Boolean);
    let j = col("j", Scalar::Json);
    let s = col("s", Scalar::Varchar);
    assert_eq!(agg(reducer(AggFn::AnyValue, vec![n.clone()], false, vec![])), Ok("ANY_VALUE(\"t0\".n)".into()));
    assert_eq!(agg(reducer(AggFn::Variance, vec![n.clone()], false, vec![])), Ok("VARIANCE(\"t0\".n)".into()));
    assert_eq!(agg(reducer(AggFn::Stddev, vec![n.clone()], true, vec![])), Ok("STDDEV(DISTINCT \"t0\".n)".into()));
    // no max/min over boolean (bool_or/bool_and), nor over jsonb (the first in jsonb's order)
    assert_eq!(agg(reducer(AggFn::Max, vec![b.clone()], false, vec![])), Ok("bool_or(\"t0\".b)".into()));
    assert_eq!(agg(reducer(AggFn::Min, vec![b], false, vec![])), Ok("bool_and(\"t0\".b)".into()));
    assert_eq!(
        agg(reducer(AggFn::Max, vec![j.clone()], false, vec![])),
        Ok("(array_agg(\"t0\".j ORDER BY \"t0\".j DESC NULLS LAST))[1]".into())
    );
    assert_eq!(
        agg(reducer(AggFn::Min, vec![j.clone()], false, vec![])),
        Ok("(array_agg(\"t0\".j ORDER BY \"t0\".j ASC NULLS LAST))[1]".into())
    );
    assert_eq!(
        agg(reducer(AggFn::Median, vec![s.clone()], false, vec![])),
        Err(capability("median of a non-number reached Postgres, whose percentile_cont interpolates numbers only"))
    );
    assert_eq!(
        agg(reducer(AggFn::Mode, vec![n.clone()], true, vec![])),
        Err(capability("a distinct or ordered mode reached Postgres"))
    );
    assert_eq!(
        agg(reducer(AggFn::QuantileCont, vec![n.clone()], false, vec![])),
        Err(capability("QUANTILE_CONT of this shape reached Postgres"))
    );
    assert_eq!(
        agg(reducer(AggFn::QuantileDisc, vec![n.clone(), float_lit(0.5)], false, vec![key(n.clone(), false)])),
        Ok("percentile_disc(0.5) WITHIN GROUP (ORDER BY \"t0\".n DESC)".into())
    );
    assert_eq!(
        agg(reducer(AggFn::List, vec![n.clone()], true, vec![key(n.clone(), true)])),
        Err(capability("a LIST aggregate of this shape reached Postgres"))
    );
    assert_eq!(
        agg(reducer(AggFn::List, vec![list("xs", Scalar::Bigint)], false, vec![key(n.clone(), true)])),
        Ok("array_agg(to_jsonb(\"t0\".xs) ORDER BY \"t0\".n ASC)".into())
    );
    assert_eq!(
        agg(reducer(AggFn::ArgMax, vec![n.clone()], false, vec![])),
        Err(capability("ARG_MAX of this shape reached Postgres"))
    );
    assert_eq!(
        agg(reducer(AggFn::Wavg, vec![n.clone()], false, vec![])),
        Err(Refusal::illegal_state("lowering marker WAVG reached the renderer"))
    );
    assert_eq!(
        agg(reducer(AggFn::RowNumber, vec![], false, vec![])),
        Err(Refusal::illegal_state("ROW_NUMBER is a window function: windowCall writes it"))
    );
}

#[test]
fn a_window_casts_its_float_aggregates_and_refuses_an_ordered_set() {
    let n = col("n", Scalar::Bigint);
    let window = |f: AggFn, frame: Option<Frame>| {
        crate::build::window_call(
            SqlAgg::Reducer(crate::build::reducer(f, vec![n.clone()], false, vec![])),
            vec![col("g", Scalar::Varchar)],
            vec![key(n.clone(), true)],
            frame,
        )
    };
    assert_eq!(
        render(&window(AggFn::Avg, None)),
        "CAST(AVG(\"t0\".n) OVER (PARTITION BY \"t0\".g ORDER BY \"t0\".n NULLS LAST) AS DOUBLE PRECISION)"
    );
    assert_eq!(
        refusal(&window(AggFn::Median, None)),
        capability("MEDIAN over a window reached Postgres, where ordered-set aggregates take no OVER clause")
    );
    let frame = |unit: &str, n: i64| Frame {
        kind: lite_sql::FrameKind::Range,
        from: Bound::IntervalPreceding { n, unit: unit.into() },
        to: Bound::CurrentRow,
    };
    assert_eq!(
        render(&window(AggFn::Sum, Some(frame("weeks", 2)))),
        "SUM(\"t0\".n) OVER (PARTITION BY \"t0\".g ORDER BY \"t0\".n NULLS LAST RANGE BETWEEN INTERVAL '14' DAY \
         PRECEDING AND CURRENT ROW)"
    );
    assert_eq!(
        refusal(&window(AggFn::Sum, Some(frame("decades", 1)))),
        capability("a decades window frame offset has no Postgres interval qualifier")
    );
    // Math.multiplyExact
    assert_eq!(
        refusal(&window(AggFn::Sum, Some(frame("WEEKS", i64::MAX / 2)))),
        Refusal { class: "java.lang.ArithmeticException", message: "long overflow".into() }
    );
    let d = Postgres::new();
    assert_eq!(d.bound(&Bound::Preceding { n: JavaNumber::Long(3) }), Ok("3 PRECEDING".into()));
    assert_eq!(
        d.bound(&Bound::IntervalFollowing { n: 3, unit: "Hours".into() }),
        Ok("INTERVAL '3' HOUR FOLLOWING".into())
    );
}

// ---- casts, variants, structs, maps

#[test]
fn each_cast_is_the_carriers_own() {
    let s = col("s", Scalar::Varchar);
    let j = col("j", Scalar::Json);
    let f8 = col("f", Scalar::Double);
    let to = |v: SqlExpr, t: SqlType| render(&cast(v, t));
    // a Variant is jsonb: text parses into it, any other value converts, NULL typed (NullVariantCastTest)
    assert_eq!(to(s.clone(), SqlType::Scalar(Scalar::Json)), "CAST(\"t0\".s AS JSONB)");
    assert_eq!(to(int_lit(1), SqlType::Scalar(Scalar::Json)), "to_jsonb(1)");
    assert_eq!(to(null_lit(), SqlType::Scalar(Scalar::Json)), "CAST(NULL AS JSONB)");
    // a whole value to a scalar reads its text; to text it is its compact JSON text
    assert_eq!(to(j.clone(), SqlType::Scalar(Scalar::Bigint)), "CAST((\"t0\".j #>> '{}') AS BIGINT)");
    assert_eq!(
        to(j.clone(), SqlType::Scalar(Scalar::Varchar)),
        r#"regexp_replace(CAST("t0".j AS VARCHAR), '("(?:[^"\\]|\\.)*")|([,:]) ', '\1\2', 'g')"#
    );
    // a Float's text keeps an integral value's '.0'
    assert_eq!(
        to(f8, SqlType::Scalar(Scalar::Varchar)),
        r"regexp_replace(CAST(&quot;t0&quot;.f AS VARCHAR), '^(-?[0-9]+)$', '\1.0')".replace("&quot;", "\"")
    );
    // a text literal under a cast is spelled as a text first
    assert_eq!(to(string_lit("7"), SqlType::Scalar(Scalar::Bigint)), "CAST(CAST('7' AS VARCHAR) AS BIGINT)");
    assert_eq!(to(string_lit("7"), SqlType::Scalar(Scalar::Varchar)), "CAST('7' AS VARCHAR)");
    // the marker casts never render
    assert_eq!(to(s.clone(), SqlType::Scalar(Scalar::TemporalText)), "\"t0\".s");
    // a map: a Variant's members as entries; NULL typed as the carrier; any other value refused
    let map = SqlType::Map {
        key: Box::new(SqlType::Scalar(Scalar::Varchar)),
        value: Box::new(SqlType::Scalar(Scalar::Json)),
    };
    assert_eq!(to(null_lit(), map.clone()), "CAST(NULL AS JSONB[])");
    assert!(
        to(j.clone(), map.clone()).starts_with("(CASE WHEN \"t0\".j IS NULL OR jsonb_typeof(\"t0\".j) = 'null' THEN")
    );
    assert_eq!(
        refusal(&cast(s.clone(), map)),
        capability("a cast of Typed[type=VARCHAR, nullable=true] to Map[key=VARCHAR, value=JSON] reached Postgres")
    );
    // a list: a Variant array decoded, a list of scalars cast element-wise, the empty collection typed
    let ints = list_of_type(Scalar::Bigint);
    assert_eq!(to(null_lit(), ints.clone()), "CAST(NULL AS BIGINT[])");
    assert_eq!(
        to(SqlExpr::ArrayLit { elements: vec![], r#type: TypeFact::Unknown }, ints.clone()),
        "CAST(ARRAY[] AS BIGINT[])"
    );
    assert_eq!(to(list("xs", Scalar::Integer), ints.clone()), "CAST(\"t0\".xs AS BIGINT[])");
    assert_eq!(
        to(j, ints.clone()),
        "(CASE WHEN \"t0\".j IS NULL OR jsonb_typeof(\"t0\".j) = 'null' THEN NULL ELSE ARRAY(SELECT \
         CAST((__e0 #>> '{}') AS BIGINT) FROM jsonb_array_elements(\"t0\".j) WITH ORDINALITY AS __j0(__e0, __o0) ORDER \
         BY __o0) END)"
    );
    assert_eq!(
        refusal(&cast(s, ints)),
        capability("a cast of Typed[type=VARCHAR, nullable=true] to Array[element=BIGINT] reached Postgres")
    );
}

#[test]
fn variants_and_structs_are_jsonb() {
    let j = col("j", Scalar::Json);
    let n = col("n", Scalar::Bigint);
    assert_eq!(render(&call(SqlFn::VariantGet, vec![j.clone(), string_lit("$")])), "\"t0\".j");
    assert_eq!(render(&call(SqlFn::VariantGet, vec![j.clone(), string_lit("k")])), "(\"t0\".j -> 'k')");
    assert_eq!(render(&call(SqlFn::VariantGet, vec![j.clone(), int_lit(0)])), "(\"t0\".j -> 0)");
    // a computed index: -> takes an int, never a bigint
    assert_eq!(render(&call(SqlFn::VariantGet, vec![j.clone(), n.clone()])), "(\"t0\".j -> CAST(\"t0\".n AS INTEGER))");
    assert_eq!(
        render(&cast(call(SqlFn::VariantGet, vec![j.clone(), n.clone()]), SqlType::Scalar(Scalar::Bigint))),
        "CAST((\"t0\".j ->> CAST(\"t0\".n AS INTEGER)) AS BIGINT)"
    );
    assert_eq!(render(&call(SqlFn::ToVariant, vec![string_lit("x")])), "to_jsonb(CAST('x' AS VARCHAR))");
    assert_eq!(
        render(&call(SqlFn::ToVariant, vec![col("f", Scalar::Double)])),
        "(CASE WHEN \"t0\".f = trunc(\"t0\".f) AND abs(\"t0\".f) < CAST('Infinity' AS DOUBLE PRECISION) THEN \
         to_jsonb(round(CAST(\"t0\".f AS NUMERIC), 1)) ELSE to_jsonb(\"t0\".f) END)"
    );
    // a struct literal and its field read
    let st = crate::build::struct_lit(vec![StructLitField { name: "a".into(), value: int_lit(1), declared: None }]);
    assert_eq!(render(&st), "jsonb_build_object('a', 1)");
    let source = column(
        "o",
        TypeFact::Typed {
            r#type: SqlType::Struct {
                fields: vec![StructField { name: "a".into(), r#type: SqlType::Scalar(Scalar::Bigint) }],
            },
            nullable: true,
        },
        Some(Origin::Physical),
    );
    assert_eq!(render(&crate::build::struct_get(source.clone(), "a")), "CAST(((\"t0\".o -> 'a') #>> '{}') AS BIGINT)");
    assert_eq!(
        refusal(&crate::build::struct_get_given(j, "b".into(), TypeFact::Unknown)),
        capability("a field 'b' of unknown type reached Postgres")
    );
}

// ---- projections, keys, the clauses

#[test]
fn a_projection_labels_from_its_output_and_types_its_nulls_and_raises() {
    let ints = list_of_type(Scalar::Bigint);
    assert_eq!(
        written(&projection(null_lit(), None, Some(("xs", ints)))),
        Ok("CAST(NULL AS BIGINT[]) AS \"xs\"".into())
    );
    // an alias-less projection labelled explicitly from its declared output
    assert_eq!(
        written(&projection(col("n", Scalar::Bigint), None, Some(("N", SqlType::Scalar(Scalar::Bigint))))),
        Ok("\"t0\".n AS \"N\"".into())
    );
    // error() projected under a slot is typed as the slot
    let raise = call(SqlFn::Error, vec![string_lit("boom")]);
    assert_eq!(
        written(&projection(raise, Some("d"), Some(("d", SqlType::Scalar(Scalar::Date))))),
        Ok("CAST(CAST(CAST(chr(31) || ('boom') || chr(31) AS TIMESTAMPTZ) AS VARCHAR) AS DATE) AS \"d\"".into())
    );
    // the empty collection in a scalar slot is written typed as the slot
    let empty = call(SqlFn::ListMax, vec![null_lit()]);
    assert_eq!(
        written(&projection(empty, Some("m"), Some(("m", SqlType::Scalar(Scalar::Bigint))))),
        Ok("CAST(NULL AS BIGINT) AS \"m\"".into())
    );
    // a raise whose value is a list carries an empty volatile term
    let raise_list = SqlExpr::Call {
        r#fn: SqlFn::Error,
        args: vec![string_lit("e"), string_lit("1:2")],
        r#type: TypeFact::Typed { r#type: list_of_type(Scalar::Bigint), nullable: true },
    };
    assert_eq!(
        render(&raise_list),
        "CAST(CAST(chr(31) || '1:2' || chr(30) || ('e') || chr(31) || left(CAST(random() AS VARCHAR), 0) AS \
         TIMESTAMPTZ) AS VARCHAR)"
    );
}

#[test]
fn a_constant_key_is_a_typed_expression_and_qualify_reads_through_outputs() {
    // PostgresDialectTest.aConstantKeyIsATypedExpression's hand-built ORDER BY
    let k = string_lit("x");
    let mut s = select(vec![projection(k.clone(), Some("k"), None)], SqlSource::Dual);
    s.order_by = vec![key(k, true)];
    s.outputs = vec![OutputCol {
        name: "k".into(),
        r#type: SqlType::Scalar(Scalar::Varchar),
        nullable: false,
        origin: Origin::Derived,
    }];
    assert_eq!(
        AnsiSqlRenderer::render(&Postgres::new(), SqlQuery::Select(Box::new(s))),
        Ok("SELECT 'x' AS \"k\"\nORDER BY CAST('x' AS VARCHAR) NULLS LAST".into())
    );
}

#[test]
fn the_ddl_and_the_store_writes() {
    let d = Postgres::new();
    let plain = |kind: ColumnKind| ColumnType::Plain { kind };
    assert_eq!(d.ddl_type(&plain(ColumnKind::Tinyint)), Ok("SMALLINT".into()));
    assert_eq!(d.ddl_type(&plain(ColumnKind::Float)), Ok("DOUBLE PRECISION".into()));
    assert_eq!(d.ddl_type(&plain(ColumnKind::Bit)), Ok("BOOLEAN".into()));
    assert_eq!(d.ddl_type(&plain(ColumnKind::Json)), Ok("JSONB".into()));
    assert_eq!(
        d.ddl_type(&ColumnType::Scaled { kind: "DECIMAL".into(), precision: 18, scale: 6 }),
        Ok("DECIMAL(18, 6)".into())
    );
    assert_eq!(
        d.ddl_type(&plain(ColumnKind::TimestampNs)),
        Err(capability("a TIMESTAMP_NS column: Postgres keeps microseconds"))
    );
    assert_eq!(
        d.ddl_type(&plain(ColumnKind::Array)),
        Err(Refusal::illegal_state("no DDL spelling for declared column type ARRAY"))
    );
    let table = SqlDdl::CreateTable {
        schema: Some("S".into()),
        table: "Trades".into(),
        quoting: NameQuoting::default(),
        columns: vec![lite_sql::DdlColumn {
            name: "ID".into(),
            r#type: plain(ColumnKind::Bigint),
            primary_key: true,
            not_null: false,
            declared_quoted: false,
        }],
        temporary: false,
    };
    assert_eq!(
        AnsiSqlRenderer::render_ddl(&d, &table),
        Ok("Create Table s.trades(id BIGINT NOT NULL, PRIMARY KEY(id));".into())
    );
    // the staged rows: the standard temporary table, its columns named (TempTableAsTest)
    let values = SqlSource::Values {
        rows: vec![vec![string_lit("a")]],
        columns: vec!["v0".into()],
        alias: "v".into(),
        outputs: vec![OutputCol {
            name: "v0".into(),
            r#type: SqlType::Scalar(Scalar::Varchar),
            nullable: false,
            origin: Origin::Derived,
        }],
    };
    let rows = SqlQuery::Select(Box::new(select(
        vec![projection(
            SqlExpr::Column(crate::build::column_of(
                Some("v"),
                &OutputCol {
                    name: "v0".into(),
                    r#type: SqlType::Scalar(Scalar::Varchar),
                    nullable: false,
                    origin: Origin::Derived,
                },
            )),
            Some("v0"),
            None,
        )],
        values,
    )));
    let staged = SqlDml::CreateTempTableAs {
        table: "legend_stage_rowsA".into(),
        query: rows,
        columns: vec!["v0".into()],
        staged: true,
    };
    let text = AnsiSqlRenderer::render_dml(&d, &staged).unwrap();
    assert!(text.starts_with("CREATE TEMPORARY TABLE legend_stage_rowsa (\"v0\") AS SELECT "), "{text}");
    assert_eq!(
        d.cte_as(&Cte {
            name: "c".into(),
            query: SqlQuery::Select(Box::new(select(vec![], SqlSource::Dual))),
            materialized: true
        }),
        " AS MATERIALIZED ("
    );
}

#[test]
fn the_type_tables() {
    let d = Postgres::new();
    assert_eq!(d.scalar_type_name(Scalar::Hugeint), Ok("NUMERIC(38)".into()));
    assert_eq!(d.scalar_type_name(Scalar::Json), Ok("JSONB".into()));
    assert_eq!(d.scalar_type_name(Scalar::Time), Ok("TIME".into()));
    assert_eq!(
        d.scalar_type_name(Scalar::DecimalText),
        Err(Refusal::illegal_state("DECIMAL_TEXT cast reached a dialect without DECIMAL_TEXT support"))
    );
    assert_eq!(d.struct_type_name(&[]), Err(capability("a STRUCT type reached a dialect without struct support")));
    assert_eq!(d.map_type_name("VARCHAR", "BIGINT"), "JSONB[]");
    assert_eq!(d.json_kind_name(JsonKind::UnsignedInteger), Ok("UBIGINT".into()));
    assert_eq!(d.json_kind_name(JsonKind::String), Ok("VARCHAR".into()));
    assert_eq!(
        d.typeof_name(Scalar::Bigint),
        Err(capability("typeof reached Postgres: its type names are Postgres's"))
    );
    assert_eq!(
        d.format_text(&[
            DateFmt::Part(DateFmtPart::Year4),
            DateFmt::Text { s: "-\"T\\".into() },
            DateFmt::Part(DateFmtPart::SubsecNano),
            DateFmt::Part(DateFmtPart::Hour12Nopad),
        ]),
        Ok("YYYY\"-\\\"T\\\\\"US\"000\"FMHH12".into())
    );
    assert_eq!(d.max_function_arguments(), 100);
}

#[test]
fn a_nested_store_column_is_read_as_jsonb() {
    let plain = |kind: ColumnKind| ColumnType::Plain { kind };
    // json cast, an array or a composite converted, OTHER as text, every other as held
    assert!(<Postgres as AnsiSqlRenderer>::reads_stored(&plain(ColumnKind::Json)));
    assert!(<Postgres as AnsiSqlRenderer>::reads_stored(&plain(ColumnKind::Array)));
    assert!(<Postgres as AnsiSqlRenderer>::reads_stored(&plain(ColumnKind::Object)));
    assert!(<Postgres as AnsiSqlRenderer>::reads_stored(&plain(ColumnKind::Other)));
    assert!(!<Postgres as AnsiSqlRenderer>::reads_stored(&plain(ColumnKind::Varchar)));
    assert!(!<Postgres as AnsiSqlRenderer>::reads_stored(&ColumnType::Sized { kind: "VARCHAR".into(), size: 3 }));
    let read = |kind: ColumnKind| {
        render(&crate::build::stored_read(
            Column {
                table: Some("t0".into()),
                name: "v".into(),
                r#type: typed(Scalar::Json),
                origin: Some(Origin::Physical),
            },
            plain(kind),
        ))
    };
    // StoredReadsTest's spellings
    assert_eq!(read(ColumnKind::Json), "CAST(\"t0\".v AS JSONB)");
    assert_eq!(read(ColumnKind::Array), "to_jsonb(\"t0\".v)");
    assert_eq!(read(ColumnKind::Other), "CAST(\"t0\".v AS VARCHAR)");
}

// ---- the engine's text

#[test]
fn the_engines_h2_text_is_read_with_postgres_types_and_folded_bare_names() {
    let d = Postgres::new();
    let t = |s: &str| AnsiSqlRenderer::from_engine_text(&d, s).unwrap();
    assert_eq!(
        t("CREATE TABLE S.T (ID INT, Px DOUBLE, F FLOAT, B BIT, Tiny TINYINT, D DOUBLE PRECISION, \"Quoted\" DOUBLE)"),
        "CREATE TABLE S.T (\"id\" INT, \"px\" DOUBLE PRECISION, \"f\" DOUBLE PRECISION, \"b\" BOOLEAN, \"tiny\" \
         SMALLINT, \"d\" DOUBLE PRECISION, \"Quoted\" DOUBLE PRECISION)"
    );
    // (?i): any case; a word that only starts DOUBLE or TINYINT is left
    assert_eq!(
        t("create table t (a double  precision, b doubles, c tinyint2, d Double)"),
        "create table t (\"a\" double  precision, \"b\" doubles, \"c\" tinyint2, \"d\" DOUBLE PRECISION)"
    );
    assert_eq!(t("insert into T (ID, \"Name\") values (1, 'x')"), "insert into T (\"id\", \"Name\") values (1, 'x')");
    assert_eq!(t("select CURRENT_TIMESTAMP() from dual"), "select CURRENT_TIMESTAMP from dual");
    assert_eq!(d.raw("select 1", Some("H2")), Ok("select 1".into()));
    assert_eq!(d.raw("CREATE TABLE T (A TINYINT)", Some("H2")), Ok("CREATE TABLE T (\"a\" SMALLINT)".into()));
    assert_eq!(d.raw("CREATE TABLE T (A TINYINT)", Some("Postgres")), Ok("CREATE TABLE T (A TINYINT)".into()));
}

// ---- the catalog, the interface

/// PostgresCatalogTest's probe: each column's declaration by Postgres's rules.
#[test]
fn each_catalog_column_is_declared_by_postgres_rules() {
    let d = Postgres::new();
    let col = |name: &str, data: &str, logical: &str, precision: Option<i32>, scale: Option<i32>| CatalogColumn {
        name: name.into(),
        data_type: data.into(),
        logical_type: Some(logical.into()),
        precision,
        scale,
        not_null: false,
    };
    let declared = |c: CatalogColumn| {
        let t = d.catalog_type(&c).unwrap();
        if t.read == Read::LeftOut { "left out".to_owned() } else { t.declared.unwrap() }
    };
    for (logical, expected) in [
        ("int4", "INTEGER"),
        ("int2", "SMALLINT"),
        ("int8", "BIGINT"),
        ("numeric", "DOUBLE"),
        ("float4", "REAL"),
        ("float8", "DOUBLE"),
        ("bool", "BIT"),
        ("text", "VARCHAR(4096)"),
        ("bpchar", "VARCHAR(4096)"),
        ("timestamptz", "TIMESTAMP"),
        ("timetz", "OTHER"),
        ("json", "SEMISTRUCTURED"),
        ("ARRAY", "OTHER"),
        ("bytea", "left out"),
        ("ENUM", "OTHER"),
        ("COMPOSITE", "OTHER"),
        ("USER-DEFINED", "OTHER"),
        ("pg_mcv_list", "OTHER"),
    ] {
        assert_eq!(declared(col("c", logical, logical, None, None)), expected, "{logical}");
    }
    assert_eq!(declared(col("n12", "numeric(12,3)", "numeric", Some(12), Some(3))), "DECIMAL(12,3)");
    let tstz = d.catalog_type(&col("tstz", "timestamp with time zone", "timestamptz", None, None)).unwrap();
    assert_eq!(tstz.read, Read::CopyConverted);
    assert_eq!(tstz.conversion.as_deref(), Some("CAST(timezone('UTC', %s) AS TIMESTAMP)"));
    assert_eq!(
        d.catalog_type(&col("g", "geometry", "geometry", None, None)),
        Err(capability(
            "a column of Postgres type 'geometry' cannot be declared in a Pure Database (a type this dialect does not read)"
        ))
    );
    // every type the table decides, in Java's order, OTHER's last
    let types = catalog_types();
    assert_eq!(types.len(), 16 + OTHER_TYPES.len());
    assert_eq!(types[0].0, "BOOL");
    assert_eq!(types.last().unwrap().0, "PG_BRIN_MINMAX_MULTI_SUMMARY");
}

#[test]
fn the_interface_as_postgres_answers_it() {
    let d = Postgres::new();
    assert_eq!(d.session_setup(), vec!["SET TimeZone='UTC'".to_owned()]);
    assert_eq!(d.script(&["a".into(), "b".into()]), "BEGIN TRANSACTION;\na;\nb;\nCOMMIT;");
    assert_eq!(d.script_abort().as_deref(), Some("ROLLBACK"));
    assert_eq!(d.keys_snapshot(), Ok(KeysSnapshot::RepeatableRead));
    assert_eq!(d.result_column_limit(), Ok(Some(1664)));
    assert!(d.needs_static_pivot());
    assert!(!d.in_process());
    assert!(!d.loads_rows_in_bulk());
    assert!(d.temporary_tables());
    assert_eq!(d.server_versions(), Ok(None));
    assert!(d.for_server("16.4").is_none());
    let map = SqlType::Map {
        key: Box::new(SqlType::Scalar(Scalar::Varchar)),
        value: Box::new(SqlType::Scalar(Scalar::Bigint)),
    };
    assert!(d.delivers_own(&map, "ARRAY"));
    assert!(!d.delivers_own(&map, "VARCHAR"));
    assert!(!d.delivers_own(&SqlType::Scalar(Scalar::Bigint), "ARRAY"));
    assert_eq!(d.copy_select_list(&[]), Ok("*".into()));
    assert_eq!(d.failing_statement("x", &["a".into()]), None);
    assert_eq!(
        d.catalog_columns_query("s", "t").unwrap_err().message,
        "the platform asks no catalog question of Postgres"
    );
    // a catalog grid is refused by the base
    let grid = SqlSource::Catalog {
        grid: CatalogGrid::Tables,
        schema_pattern: None,
        table_pattern: None,
        column_pattern: None,
        alias: "c".into(),
        outputs: Vec::new(),
    };
    let mut w = SqlWriter::new();
    assert_eq!(
        d.catalog(&mut w, &grid),
        Err(capability("a TABLES catalog grid (fetchDb*MetaData) reached a dialect without a catalog reading"))
    );
}

#[test]
fn the_passes_are_postgres_list() {
    let names: Vec<&str> = Postgres::new().passes().iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        [
            "CarrierStrategies",
            "StarExceptToColumns",
            "WholePartitionOrderedSets",
            "SubstringClamp",
            "StringCountsAsInt",
            "AggregatesDeliverDouble",
            "QualifyToSubselect",
            "ConstantKeysAsExpressions",
            "RootNumericTypes",
        ]
    );
    let all: Vec<&str> = ansi::render_passes(&Postgres::new()).iter().map(|p| p.name()).collect();
    assert_eq!(all.first(), Some(&"FloatPrintText.Expansion"));
    assert_eq!(all.last(), Some(&"StoredReads"));
}
