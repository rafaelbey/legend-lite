// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! H2's and H2Modern's hooks held to their Java (`H2.java`, `H2Modern.java`): each spelling and refusal on a
//! hand-built node, the inputs the golden oracles hold few or none of among them (`split_part`, the format template's
//! refusals, a week-long frame bound). The oracles hold the rest (`tests/dialect_differential.rs`).

use super::*;
use crate::SqlDialect;
use crate::build::{bool_lit, call, cast, float_lit, int_lit, null_lit, string_lit};
use crate::statement::list_text;
use crate::writer::SqlWriter;
use lite_sql::{CatalogGrid, JavaNumber, ParamKind, ValueTyping};

// ---- builders

fn typed(s: Scalar) -> TypeFact {
    TypeFact::Typed { r#type: SqlType::Scalar(s), nullable: true }
}

fn column(name: &str, t: TypeFact, origin: Option<Origin>) -> SqlExpr {
    SqlExpr::Column(Column { table: Some("t0".into()), name: name.into(), r#type: t, origin })
}

/// A physical BIGINT column of `t0`.
fn col(name: &str) -> SqlExpr {
    column(name, typed(Scalar::Bigint), Some(Origin::Physical))
}

fn part(p: DatePart) -> SqlExpr {
    SqlExpr::DatePartLit { part: p, r#type: TypeFact::Unknown }
}

fn render_with(d: &H2, e: &SqlExpr) -> Result<String, Refusal> {
    let mut w = SqlWriter::new();
    d.expr(&mut w, e, 0)?;
    w.into_text()
}

fn render(e: &SqlExpr) -> String {
    render_with(&H2::new(), e).unwrap_or_else(|r| panic!("{r}"))
}

fn modern(e: &SqlExpr) -> String {
    render_with(&H2::modern(), e).unwrap_or_else(|r| panic!("{r}"))
}

fn refusal(e: &SqlExpr) -> Refusal {
    render_with(&H2::new(), e).unwrap_err()
}

fn capability(message: &str) -> Refusal {
    Refusal::capability(message)
}

fn reducer(f: AggFn, args: Vec<SqlExpr>, distinct: bool, order_by: Vec<SortKey>) -> SqlExpr {
    SqlExpr::Reducer(crate::build::reducer(f, args, distinct, order_by))
}

fn key(e: SqlExpr, ascending: bool, null_order: Option<NullOrder>) -> SortKey {
    SortKey { expr: e, ascending, null_order, output_name: None }
}

fn param(name: &str, r#type: TypeFact, by_value: Option<ValueTyping>) -> SqlExpr {
    SqlExpr::PlanParam {
        name: name.into(),
        kind: ParamKind::Other,
        optional: false,
        enum_map_fn: None,
        r#type,
        by_value,
    }
}

fn one_projection(p: Projection) -> SqlQuery {
    SqlQuery::Select(Box::new(SqlSelect {
        projections: vec![p],
        distinct: false,
        from: SqlSource::Dual,
        r#where: None,
        group_by: Vec::new(),
        having: None,
        qualify: None,
        order_by: Vec::new(),
        limit: None,
        offset: None,
        outputs: Vec::new(),
    }))
}

// ---- names

#[test]
fn every_alias_and_derived_label_quotes() {
    let d = H2::new();
    assert_eq!(d.alias_ident("id"), "\"id\"");
    assert_eq!(d.alias_ident("\"x\""), "\"x\"");
    assert_eq!(d.alias_ident("a\"b"), "\"a\"\"b\"");
    // a lone quote is no pre-quoted name
    assert_eq!(d.alias_ident("\""), "\"\"\"\"");
    assert_eq!(d.star_except_name("v"), "\"v\"");
    assert_eq!(d.star_except_keyword(), "EXCEPT");
    assert_eq!(d.row_order_column(), "_ROWID_");
}

#[test]
fn a_reference_spells_by_its_origin() {
    assert_eq!(render(&col("ID")), "\"t0\".ID");
    assert_eq!(render(&col("order")), "\"t0\".\"order\"");
    assert_eq!(render(&col("a$b")), "\"t0\".a$b");
    assert_eq!(render(&column("Id", typed(Scalar::Bigint), Some(Origin::PhysicalQuoted))), "\"t0\".\"Id\"");
    assert_eq!(render(&column("id", typed(Scalar::Bigint), Some(Origin::Derived))), "\"t0\".\"id\"");
    // an origin-less reference is refused, naming it
    assert_eq!(
        refusal(&column("x", TypeFact::Unknown, None)),
        capability(
            "origin-less column reference reached the case-sensitive H2 renderer: t0.x — stamp the construction site \
             (Column.derived/physical or an OutputCol door)"
        )
    );
    let bare = SqlExpr::Column(Column { table: None, name: "x".into(), r#type: TypeFact::Unknown, origin: None });
    assert!(refusal(&bare).message.contains("H2 renderer: x — stamp"), "{}", refusal(&bare).message);
    // a per-key name is written by the expression writer, never by the column rule
    let per_key = Column { table: None, name: "k".into(), r#type: TypeFact::Unknown, origin: Some(Origin::PerKey) };
    assert_eq!(
        H2::new().column_name(&per_key).unwrap_err(),
        Refusal::illegal_state(
            "a per-key name 'k' spelled without its key's position: the expression writer writes it inside its \
             per-key region"
        )
    );
}

#[test]
fn a_schema_qualified_table_spells_each_part_as_its_ddl_did() {
    let d = H2::new();
    let quoting = |schema, table| NameQuoting { schema, table };
    assert_eq!(d.table_name("S.T", quoting(false, false)), "S.T");
    assert_eq!(d.table_name("S.order", quoting(false, false)), "S.\"order\"");
    assert_eq!(d.table_name("s.T", quoting(true, false)), "\"s\".T");
    // the table part is everything after the first dot
    assert_eq!(d.table_name("S.T.U", quoting(false, true)), "S.\"T.U\"");
    // a single part: the base's rule
    assert_eq!(d.table_name("T", quoting(false, false)), "T");
    assert_eq!(d.table_name("T", quoting(false, true)), "\"T\"");
    assert_eq!(d.table_name(".T", quoting(false, false)), "\".T\"");
}

#[test]
fn the_lexicon_is_the_engines_and_right() {
    let d = H2::new();
    assert_eq!(d.lexicon().reserved_words().count(), ENGINE_RESERVED.len() + 1);
    assert!(ENGINE_RESERVED.iter().all(|w| d.lexicon().is_reserved(w)));
    assert!(d.lexicon().is_reserved("right") && !ENGINE_RESERVED.contains(&"right"));
    assert_eq!(d.lexicon().quote_char(), '"');
}

// ---- functions

#[test]
fn h2_spells_its_own_functions() {
    let d = col("D");
    let s = col("S");
    let cases: Vec<(SqlExpr, &str)> = vec![
        (call(SqlFn::IntDivide, vec![col("A"), int_lit(2)]), "CAST(\"t0\".A / 2 AS BIGINT)"),
        (call(SqlFn::AddInterval, vec![part(DatePart::Day), int_lit(3), d.clone()]), "dateadd(DAY, 3, \"t0\".D)"),
        (call(SqlFn::Extract, vec![part(DatePart::Hour), d.clone()]), "extract(HOUR FROM \"t0\".D)"),
        (call(SqlFn::Extract, vec![part(DatePart::DayOfYear), d.clone()]), "extract(DOY FROM \"t0\".D)"),
        (call(SqlFn::DateDiff, vec![part(DatePart::Day), col("A"), col("B")]), "DATEDIFF(DAY, \"t0\".A, \"t0\".B)"),
        (call(SqlFn::StartsWith, vec![s.clone(), string_lit("ab")]), "(LEFT(\"t0\".S, CHAR_LENGTH('ab')) = 'ab')"),
        (call(SqlFn::EndsWith, vec![s.clone(), string_lit("ab")]), "(RIGHT(\"t0\".S, CHAR_LENGTH('ab')) = 'ab')"),
        (call(SqlFn::Strpos, vec![s.clone(), string_lit("b")]), "LOCATE('b', \"t0\".S)"),
        (call(SqlFn::BoolToText, vec![col("B")]), "CASE WHEN \"t0\".B THEN 'true' WHEN NOT \"t0\".B THEN 'false' END"),
        (call(SqlFn::RegexpFullMatch, vec![s.clone(), string_lit("a.c")]), "REGEXP_LIKE(\"t0\".S, '^(?:a.c)$')"),
        (
            call(SqlFn::RegexpFullMatch, vec![s.clone(), col("P")]),
            "REGEXP_LIKE(\"t0\".S, CONCAT('^(?:', \"t0\".P, ')$'))",
        ),
        (call(SqlFn::Error, vec![string_lit("m")]), "SIGNAL('45000', CHAR(31) || ('m') || CHAR(31))"),
        (
            call(SqlFn::Error, vec![string_lit("m"), string_lit("3:4")]),
            "SIGNAL('45000', CHAR(31) || '3:4' || CHAR(30) || ('m') || CHAR(31))",
        ),
        (call(SqlFn::EpochSeconds, vec![d.clone()]), "EXTRACT(EPOCH FROM \"t0\".D)"),
        (call(SqlFn::EpochMs, vec![d.clone()]), "CAST(EXTRACT(EPOCH FROM \"t0\".D) * 1000 AS BIGINT)"),
        (call(SqlFn::Random, vec![]), "RAND()"),
        (call(SqlFn::Now, vec![]), "now()"),
        (call(SqlFn::Guid, vec![]), "CAST(uuid() AS VARCHAR)"),
        (call(SqlFn::Md5, vec![s.clone()]), "RAWTOHEX(HASH('MD5', \"t0\".S))"),
        (call(SqlFn::Sha1, vec![s.clone()]), "RAWTOHEX(HASH('SHA-1', \"t0\".S))"),
        (call(SqlFn::Sha256, vec![s.clone()]), "RAWTOHEX(HASH('SHA-256', \"t0\".S))"),
        (call(SqlFn::DateTrunc, vec![part(DatePart::Month), d.clone()]), "CAST(date_trunc('month', \"t0\".D) AS DATE)"),
        (call(SqlFn::DateTrunc, vec![part(DatePart::Hour), d.clone()]), "date_trunc('hour', \"t0\".D)"),
        // the pad character is a space where Pure gives none; the length is written as it is
        (call(SqlFn::Lpad, vec![s.clone(), col("N")]), "lpad(\"t0\".S, \"t0\".N, ' ')"),
        (call(SqlFn::Strftime, vec![d.clone(), string_lit("x")]), "formatdatetime(\"t0\".D, 'x')"),
        (call(SqlFn::Strptime, vec![s.clone(), string_lit("x")]), "parsedatetime(\"t0\".S, 'x')"),
        (call(SqlFn::Matches, vec![s.clone(), string_lit("x")]), "regexp_like(\"t0\".S, 'x')"),
        (call(SqlFn::ListLength, vec![col("L")]), "len(\"t0\".L)"),
        (call(SqlFn::DateAtTime, vec![d.clone(), col("T")]), "\"t0\".D + \"t0\".T"),
    ];
    for (e, want) in cases {
        assert_eq!(render(&e), want);
    }
}

#[test]
fn h2_refuses_what_it_cannot_spell_by_name() {
    let s = col("S");
    let cases: Vec<(SqlExpr, Refusal)> = vec![
        (
            call(SqlFn::AddInterval, vec![part(DatePart::Quarter), int_lit(1), col("D")]),
            capability("interval unit 'QUARTER' reached a dialect without a dateadd unit"),
        ),
        (
            call(SqlFn::Extract, vec![part(DatePart::Nanosecond), col("D")]),
            capability(
                "a nanosecond part has no measured H2 spelling (the Cypher island's nanosecond leg is DuckDB's)",
            ),
        ),
        (
            call(SqlFn::EpochNs, vec![col("D")]),
            capability("a nanosecond epoch: H2 keeps microseconds (the Cypher island's nanosecond leg is DuckDB's)"),
        ),
        (
            call(SqlFn::JsonMergePatch, vec![s.clone(), s.clone()]),
            capability("a JSON merge reached H2, which has no json_merge_patch()"),
        ),
        (call(SqlFn::BitNot, vec![col("A")]), capability("bitNot reached H2, which has no xor()")),
        (call(SqlFn::MapEmpty, vec![]), capability("an empty map reached H2, which has no map type")),
        (call(SqlFn::TimeBucket, vec![]), capability("a time bucket reached H2, which has no time_bucket()")),
        // the base's refusals: no list, bit-op, hash or unnest encoding here
        (call(SqlFn::ListSum, vec![col("L")]), capability("LIST_SUM reached a dialect without a list encoding")),
        (call(SqlFn::BitAnd, vec![col("A"), col("B")]), capability("BIT_AND reached a dialect without bit-op support")),
        (call(SqlFn::Hash, vec![s.clone()]), capability("signed 64-bit hashCode reached a dialect without a spelling")),
        (call(SqlFn::Unnest, vec![col("L")]), capability("UNNEST reached a dialect without an unnest placement")),
        (
            call(SqlFn::VariantGet, vec![col("J"), string_lit("k")]),
            capability("variant navigation reached a dialect without JSON support"),
        ),
    ];
    for (e, want) in cases {
        assert_eq!(refusal(&e), want);
    }
}

#[test]
fn a_boolean_compared_with_text_takes_the_boolean_the_text_names() {
    let is_null = call(SqlFn::IsNull, vec![col("A")]);
    assert_eq!(render(&call(SqlFn::Equal, vec![is_null.clone(), string_lit("Y")])), "(\"t0\".A IS NULL = TRUE)");
    assert_eq!(render(&call(SqlFn::Equal, vec![string_lit("no"), bool_lit(true)])), "(TRUE = FALSE)");
    // text that names no boolean: the comparison as it is
    assert_eq!(render(&call(SqlFn::Equal, vec![bool_lit(true), string_lit("maybe")])), "TRUE = 'maybe'");
    assert_eq!(render(&call(SqlFn::Equal, vec![col("A"), string_lit("1")])), "\"t0\".A = '1'");
}

#[test]
fn a_literal_format_template_is_a_concatenation() {
    let format = |template: &str, args: Vec<SqlExpr>| {
        let mut a = vec![string_lit(template)];
        a.extend(args);
        call(SqlFn::Format, a)
    };
    assert_eq!(
        render(&format("%s is %d%%", vec![col("A"), col("B")])),
        "(CAST(\"t0\".A AS VARCHAR) || ' is ' || CAST(\"t0\".B AS VARCHAR) || '%')"
    );
    assert_eq!(render(&format("it's", vec![])), "('it''s')");
    assert_eq!(render(&format("", vec![])), "('')");
    assert_eq!(
        refusal(&format("%x", vec![col("A")])),
        capability("format specifier '%x' reached H2, which has no printf — only %d / %s concatenate")
    );
    assert_eq!(refusal(&format("a%", vec![])), capability("format template ends in a bare '%' — no H2 spelling"));
    assert_eq!(refusal(&format("%s %s", vec![col("A")])), capability("format template has more slots than arguments"));
    // a template that is no literal: printf, as written
    assert_eq!(render(&call(SqlFn::Format, vec![col("F"), col("A")])), "printf(\"t0\".F, \"t0\".A)");
}

#[test]
fn split_part_and_regexp_extract_are_spelled_with_h2s_regexes() {
    let s = col("S");
    assert_eq!(
        render(&call(SqlFn::SplitPart, vec![s.clone(), string_lit(","), int_lit(2)])),
        "CASE WHEN (CHAR_LENGTH(\"t0\".S) - CHAR_LENGTH(REPLACE(\"t0\".S, ',', ''))) < 1 THEN '' ELSE \
         REGEXP_REPLACE(\"t0\".S, '^(?:[^\\Q,\\E]*\\Q,\\E){1}([^\\Q,\\E]*).*$', '$1') END"
    );
    // a separator of more than one character, or an index below one: the plain call
    assert_eq!(
        render(&call(SqlFn::SplitPart, vec![s.clone(), string_lit(",,"), int_lit(1)])),
        "split_part(\"t0\".S, ',,', 1)"
    );
    assert_eq!(
        render(&call(SqlFn::SplitPart, vec![s.clone(), string_lit(","), int_lit(0)])),
        "split_part(\"t0\".S, ',', 0)"
    );
    // a literal one-character separator reads the index, which Java's get(2) refuses past the end
    assert_eq!(
        refusal(&call(SqlFn::SplitPart, vec![s.clone(), string_lit(",")])),
        Refusal { class: Refusal::INDEX_OUT_OF_BOUNDS, message: "Index 2 out of bounds for length 2".into() }
    );
    assert_eq!(
        render(&call(SqlFn::RegexpExtract, vec![s.clone(), string_lit("p")])),
        "COALESCE(REGEXP_SUBSTR(\"t0\".S, 'p', 1, 1, NULL, 0), '')"
    );
    assert_eq!(
        render(&call(SqlFn::RegexpExtract, vec![s.clone(), string_lit("p"), int_lit(1)])),
        "COALESCE(REGEXP_SUBSTR(\"t0\".S, 'p', 1, 1, NULL, 1), '')"
    );
    assert_eq!(
        render(&call(SqlFn::RegexpExtract, vec![s.clone(), string_lit("p"), int_lit(1), int_lit(2)])),
        "regexp_extract(\"t0\".S, 'p', 1, 2)"
    );
    // Pure's splitPart: each separator character `\x{h}`
    assert_eq!(
        render(&call(SqlFn::PureSplitPart, vec![s.clone(), string_lit(" ,"), int_lit(2)])),
        "REGEXP_SUBSTR(\"t0\".S, '[^\\x{20}\\x{2c}]+', 1, 2)"
    );
    let needs = capability("splitPart on H2 needs a literal, non-empty separator");
    assert_eq!(refusal(&call(SqlFn::PureSplitPart, vec![s.clone(), string_lit(""), int_lit(1)])), needs);
    assert_eq!(refusal(&call(SqlFn::PureSplitPart, vec![s.clone(), col("P"), int_lit(1)])), needs);
}

#[test]
fn translate_is_spelled_only_where_h2s_means_the_same() {
    let s = col("S");
    assert_eq!(
        render(&call(SqlFn::Translate, vec![s.clone(), string_lit("abc"), string_lit("xyz")])),
        "translate(\"t0\".S, 'abc', 'xyz')"
    );
    let refused = capability(
        "TRANSLATE reached H2 with a to shorter than its from, or either not a literal: H2's TRANSLATE keeps the \
         characters translate removes",
    );
    assert_eq!(refusal(&call(SqlFn::Translate, vec![s.clone(), string_lit("abc"), string_lit("x")])), refused);
    assert_eq!(refusal(&call(SqlFn::Translate, vec![s.clone(), col("F"), string_lit("x")])), refused);
}

#[test]
fn h2modern_counts_a_json_arrays_elements() {
    let l = col("L");
    assert_eq!(modern(&call(SqlFn::ListLength, vec![l.clone()])), "CARDINALITY(\"t0\".L)");
    let as_array = cast(l.clone(), SqlType::Array { element: Box::new(SqlType::Scalar(Scalar::Bigint)) });
    assert_eq!(modern(&call(SqlFn::ListLength, vec![as_array])), "CARDINALITY(\"t0\".L)");
    // not one argument: H2's own spelling
    assert_eq!(modern(&call(SqlFn::ListLength, vec![l.clone(), l.clone()])), "len(\"t0\".L, \"t0\".L)");
}

// ---- aggregates

#[test]
fn h2s_aggregates() {
    let a = col("A");
    let s = col("S");
    let cases: Vec<(SqlExpr, &str)> = vec![
        (reducer(AggFn::Sum, vec![a.clone()], false, vec![]), "SUM(\"t0\".A)"),
        (reducer(AggFn::Count, vec![], false, vec![]), "COUNT(*)"),
        // an ordered aggregate pins the reference NULL placement
        (
            reducer(AggFn::StringAgg, vec![s.clone(), string_lit(",")], false, vec![key(a.clone(), true, None)]),
            "STRING_AGG(\"t0\".S, ',' ORDER BY \"t0\".A ASC NULLS LAST)",
        ),
        (
            reducer(AggFn::StringAgg, vec![s.clone(), string_lit(",")], true, vec![key(a.clone(), false, None)]),
            "STRING_AGG(DISTINCT \"t0\".S, ',' ORDER BY \"t0\".A DESC NULLS FIRST)",
        ),
        (
            reducer(AggFn::Count, vec![], false, vec![key(a.clone(), true, Some(NullOrder::NullsFirst))]),
            "COUNT(* ORDER BY \"t0\".A ASC NULLS FIRST)",
        ),
        // first(): MIN over a comparable scalar
        (reducer(AggFn::AnyValue, vec![a.clone()], false, vec![]), "MIN(\"t0\".A)"),
        (reducer(AggFn::AnyValue, vec![a.clone()], true, vec![]), "ANY_VALUE(DISTINCT \"t0\".A)"),
        // a list of values on the JSON carrier
        (reducer(AggFn::List, vec![a.clone()], false, vec![]), "JSON_ARRAYAGG(\"t0\".A NULL ON NULL)"),
        (
            reducer(AggFn::List, vec![a.clone()], false, vec![key(col("B"), false, None)]),
            "JSON_ARRAYAGG(\"t0\".A ORDER BY \"t0\".B DESC NULL ON NULL)",
        ),
        (reducer(AggFn::List, vec![a.clone()], true, vec![]), "LIST(DISTINCT \"t0\".A)"),
        // a percentile WITHIN GROUP, its one order key the direction
        (
            reducer(AggFn::QuantileCont, vec![a.clone(), float_lit(0.5)], false, vec![]),
            "PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY \"t0\".A)",
        ),
        (
            reducer(AggFn::QuantileDisc, vec![a.clone(), float_lit(0.25)], false, vec![key(a.clone(), false, None)]),
            "PERCENTILE_DISC(0.25) WITHIN GROUP (ORDER BY \"t0\".A DESC)",
        ),
        (reducer(AggFn::QuantileCont, vec![a.clone()], false, vec![]), "QUANTILE_CONT(\"t0\".A)"),
    ];
    for (e, want) in cases {
        assert_eq!(render(&e), want);
    }
    let json = column("J", typed(Scalar::Json), Some(Origin::Physical));
    let no_min = |what: &str| {
        capability(&format!(
            "first() over a group of {what} values has no H2 spelling: this H2 has no ANY_VALUE and MIN needs a \
             comparable scalar"
        ))
    };
    assert_eq!(refusal(&reducer(AggFn::AnyValue, vec![json], false, vec![])), no_min("JSON"));
    let list = column(
        "L",
        TypeFact::Typed {
            r#type: SqlType::Array { element: Box::new(SqlType::Scalar(Scalar::Bigint)) },
            nullable: true,
        },
        Some(Origin::Physical),
    );
    assert_eq!(refusal(&reducer(AggFn::AnyValue, vec![list], false, vec![])), no_min("Array[element=BIGINT]"));
    assert_eq!(
        refusal(&reducer(AggFn::AnyValue, vec![column("U", TypeFact::Unknown, Some(Origin::Physical))], false, vec![])),
        no_min("Unknown[]")
    );
    assert_eq!(
        refusal(&reducer(AggFn::Wavg, vec![a.clone()], false, vec![])),
        Refusal::illegal_state("lowering marker WAVG reached the renderer")
    );
    assert_eq!(
        refusal(&reducer(AggFn::RowNumber, vec![], false, vec![])),
        Refusal::illegal_state("ROW_NUMBER is a window function: windowCall writes it")
    );
}

#[test]
fn round_is_bankers_over_the_scaled_value() {
    let scaled = "(\"t0\".A * 1)";
    assert_eq!(
        render(&call(SqlFn::Round, vec![col("A")])),
        format!(
            "(CASE WHEN {scaled} - FLOOR({scaled}) = 0.5 THEN (CASE WHEN MOD(CAST(FLOOR({scaled}) AS BIGINT), 2) = 0 \
             THEN FLOOR({scaled}) ELSE FLOOR({scaled}) + 1 END) ELSE ROUND({scaled}) END / 1)"
        )
    );
    let scaled = "(\"t0\".A * POWER(10, 2))";
    assert_eq!(
        render(&call(SqlFn::Round, vec![col("A"), int_lit(2)])),
        format!(
            "(CASE WHEN {scaled} - FLOOR({scaled}) = 0.5 THEN (CASE WHEN MOD(CAST(FLOOR({scaled}) AS BIGINT), 2) = 0 \
             THEN FLOOR({scaled}) ELSE FLOOR({scaled}) + 1 END) ELSE ROUND({scaled}) END / POWER(10, 2))"
        )
    );
}

// ---- JSON, casts, navigation

#[test]
fn collections_and_objects_ride_the_json_carrier() {
    assert_eq!(render(&crate::build::array_lit(vec![])), "JSON_ARRAY()");
    assert_eq!(render(&crate::build::array_lit(vec![int_lit(1), null_lit()])), "JSON_ARRAY(1, NULL NULL ON NULL)");
    assert_eq!(render(&crate::build::json_array(vec![int_lit(1)])), "JSON_ARRAY(1)");
    // the key-value list alternates; an odd last element is left out
    assert_eq!(
        render(&crate::build::json_object(vec![string_lit("a"), int_lit(1), string_lit("b")])),
        "JSON_OBJECT('a': 1)"
    );
    assert_eq!(
        render(&crate::build::json_array_agg(col("A"), vec![lite_sql::JsonArrayAggKey { expr: col("B"), desc: true }])),
        "COALESCE(JSON_ARRAYAGG(\"t0\".A ORDER BY \"t0\".B DESC NULLS LAST), JSON '[]')"
    );
    assert_eq!(render(&call(SqlFn::ToVariant, vec![col("A")])), "CAST(\"t0\".A AS JSON)");
}

#[test]
fn a_cast_to_json_parses_and_a_boolean_cast_to_text_spells_the_references_words() {
    let json = SqlType::Scalar(Scalar::Json);
    let varchar = SqlType::Scalar(Scalar::Varchar);
    assert_eq!(render(&cast(string_lit("[1]"), json.clone())), "JSON '[1]'");
    assert_eq!(render(&cast(null_lit(), json.clone())), "CAST(NULL AS JSON)");
    assert_eq!(render(&cast(col("S"), json.clone())), "(\"t0\".S FORMAT JSON)");
    assert_eq!(
        render(&cast(call(SqlFn::Equal, vec![col("A"), col("B")]), varchar.clone())),
        "CASE WHEN \"t0\".A = \"t0\".B THEN 'true' WHEN NOT \"t0\".A = \"t0\".B THEN 'false' END"
    );
    // by its type fact too, not only its shape
    let flag = column("F", typed(Scalar::Boolean), Some(Origin::Physical));
    assert_eq!(
        render(&cast(flag, varchar.clone())),
        "CASE WHEN \"t0\".F THEN 'true' WHEN NOT \"t0\".F THEN 'false' END"
    );
    assert_eq!(render(&cast(col("A"), varchar)), "CAST(\"t0\".A AS VARCHAR)");
    assert_eq!(render(&cast(col("A"), SqlType::Scalar(Scalar::Double))), "CAST(\"t0\".A AS DOUBLE PRECISION)");
    assert_eq!(render(&cast(col("A"), SqlType::Scalar(Scalar::Time))), "CAST(\"t0\".A AS TIME(9))");
    assert_eq!(render(&cast(col("A"), SqlType::Scalar(Scalar::Hugeint))), "CAST(\"t0\".A AS NUMERIC(38))");
    let st =
        SqlType::Struct { fields: vec![StructField { name: "a".into(), r#type: SqlType::Scalar(Scalar::Bigint) }] };
    assert_eq!(refusal(&cast(col("A"), st)), capability("a STRUCT type reached a dialect without struct support"));
    assert_eq!(
        H2::new().scalar_type_name(Scalar::DecimalText).unwrap_err(),
        Refusal::illegal_state("DECIMAL_TEXT cast reached a dialect without DECIMAL_TEXT support")
    );
}

#[test]
fn h2modern_navigates_typed_json() {
    let j = col("J");
    let get = |k: SqlExpr| call(SqlFn::VariantGet, vec![j.clone(), k]);
    assert_eq!(modern(&get(string_lit("a\"b"))), "(\"t0\".J).\"a\"\"b\"");
    // the reference's zero-based index, one-based here
    assert_eq!(modern(&get(int_lit(0))), "(\"t0\".J)[1]");
    let dynamic = render_with(&H2::modern(), &get(col("K"))).unwrap_err();
    assert_eq!(dynamic, capability("variant navigation reached a dialect without JSON support"));
    let fields = vec![
        StructLitField { name: "a".into(), value: int_lit(1), declared: None },
        StructLitField { name: "it's".into(), value: string_lit("s"), declared: None },
    ];
    let lit = crate::build::struct_lit(fields);
    assert_eq!(modern(&lit), "JSON_OBJECT('a': 1, 'it''s': 's')");
    assert_eq!(refusal(&lit), capability("a struct literal reached a dialect without struct support"));
    let field = crate::build::struct_get(j.clone(), "f\"");
    assert_eq!(modern(&field), "(\"t0\".J).\"f\"\"\"");
    assert_eq!(refusal(&field), capability("a struct extraction reached a dialect without struct support"));
    // a scalar cast over navigation extracts text and strips its quoting; a cast to JSON parses, as H2's
    let bigint = SqlType::Scalar(Scalar::Bigint);
    assert_eq!(
        modern(&cast(get(string_lit("k")), bigint.clone())),
        "CAST(TRIM(BOTH '\"' FROM CAST((\"t0\".J).\"k\" AS VARCHAR)) AS BIGINT)"
    );
    assert_eq!(
        modern(&cast(field, bigint)),
        "CAST(TRIM(BOTH '\"' FROM CAST((\"t0\".J).\"f\"\"\" AS VARCHAR)) AS BIGINT)"
    );
    assert_eq!(modern(&cast(get(string_lit("k")), SqlType::Scalar(Scalar::Json))), "((\"t0\".J).\"k\" FORMAT JSON)");
}

// ---- clauses, literals, placeholders

#[test]
fn a_frame_interval_is_the_standards_quoted_singular() {
    let d = H2::new();
    assert_eq!(d.bound(&Bound::IntervalPreceding { n: 3, unit: "days".into() }), "INTERVAL '3' DAY PRECEDING");
    assert_eq!(d.bound(&Bound::IntervalPreceding { n: 2, unit: "WEEKS".into() }), "INTERVAL '14' DAY PRECEDING");
    assert_eq!(d.bound(&Bound::IntervalFollowing { n: 1, unit: "MONTH".into() }), "INTERVAL '1' MONTH FOLLOWING");
    assert_eq!(d.bound(&Bound::Preceding { n: JavaNumber::Long(2) }), "2 PRECEDING");
    assert_eq!(d.bound(&Bound::CurrentRow), "CURRENT ROW");
}

#[test]
fn a_date_format_is_javas_patterns() {
    let d = H2::new();
    let parts = vec![
        DateFmt::Part(DateFmtPart::Year4),
        DateFmt::Text { s: "-".into() },
        DateFmt::Part(DateFmtPart::Month2),
        DateFmt::Text { s: "T".into() },
        DateFmt::Part(DateFmtPart::Hour2),
        DateFmt::Part(DateFmtPart::SubsecNano),
        DateFmt::Text { s: "it's".into() },
        DateFmt::Part(DateFmtPart::Ampm),
    ];
    assert_eq!(d.format_text(&parts).unwrap(), "yyyy'-'MM'T'HHnnnnnnnnn'it''s'a");
    assert_eq!(
        d.format_text(&[DateFmt::Part(DateFmtPart::SubsecMin)]).unwrap_err(),
        capability("minimal-fraction date format reached a dialect without a trim-zeros token")
    );
    // the format is a string literal where it is written
    let lit = SqlExpr::FormatLit { parts: vec![DateFmt::Text { s: "a'b".into() }], r#type: TypeFact::Unknown };
    assert_eq!(render(&lit), "'''a''''b'''");
}

#[test]
fn every_placeholder_is_written_typed() {
    let d = H2::new();
    let statement = |e: SqlExpr| {
        let mut w = SqlWriter::new();
        d.expr(&mut w, &e, 0).map(|_| w.into_statement().unwrap())
    };
    let st = statement(param("s", typed(Scalar::Varchar), None)).unwrap();
    assert_eq!(st.sql, "CAST(? AS VARCHAR)");
    assert_eq!(list_text(&st.binds), "[Bind[parameter=s, binding=One[nullType=VARCHAR, hole=null]]]");
    assert_eq!(statement(param("t", typed(Scalar::Timestamp), None)).unwrap().sql, "CAST(? AS TIMESTAMP(9))");
    let by_value = ValueTyping { kinds: vec![ValueKind::Date, ValueKind::Decimal], absent: ValueKind::Decimal };
    let st = statement(param("v", TypeFact::Unknown, Some(by_value))).unwrap();
    assert_eq!(st.sql, "CAST(? AS )");
    assert_eq!(
        list_text(&st.binds),
        "[Bind[parameter=v, binding=One[nullType=DECIMAL, hole=TypeHole[at=10, types={DECIMAL=TypeSpelling[name=NUMERIC, \
         digits=PRECISION_AND_SCALE, fraction=null], DATE=TypeSpelling[name=DATE, digits=NONE, fraction=null]}, \
         absent=DECIMAL]]]]"
    );
    assert_eq!(
        statement(param("p", TypeFact::Unknown, None)).unwrap_err(),
        capability(
            "plan parameter 'p' has no type and is not typed by its value, and H2 types a parameter when it prepares \
             the statement: not bound on H2"
        )
    );
    // the type H2 gives a literal of each kind of value
    let holes: Vec<TypeSpelling> = ValueKind::ALL.iter().map(|k| d.hole_type(*k).unwrap()).collect();
    assert_eq!(
        holes,
        [
            TypeSpelling::new("BIGINT", Digits::None),
            TypeSpelling::new("NUMERIC", Digits::PrecisionAndScale),
            TypeSpelling::new("DECFLOAT", Digits::Precision),
            TypeSpelling::new("DATE", Digits::None),
            TypeSpelling::new("TIMESTAMP(9)", Digits::None),
            TypeSpelling::new("TIMESTAMP(9)", Digits::None),
        ]
    );
}

#[test]
fn an_alias_less_projection_labels_from_its_declared_output() {
    let d = H2::new();
    let out = |name: &str| lite_sql::OutputCol {
        name: name.into(),
        r#type: SqlType::Scalar(Scalar::Bigint),
        nullable: true,
        origin: Origin::Derived,
    };
    let render_query = |p: Projection| SqlDialect::render(&d, one_projection(p)).unwrap();
    assert_eq!(
        render_query(Projection { expr: col("hireType"), alias: None, out: Some(out("hireType")) }),
        "SELECT \"t0\".hireType AS \"hireType\""
    );
    // the declared output's label bypasses the base's typed NULL, which an aliased projection keeps
    assert_eq!(render_query(Projection { expr: null_lit(), alias: None, out: Some(out("x")) }), "SELECT NULL AS \"x\"");
    assert_eq!(
        render_query(Projection { expr: null_lit(), alias: Some("x".into()), out: Some(out("x")) }),
        "SELECT CAST(NULL AS BIGINT) AS \"x\""
    );
    assert_eq!(render_query(Projection { expr: int_lit(1), alias: None, out: None }), "SELECT 1");
}

#[test]
fn ddl_types_are_h2s() {
    let d = H2::new();
    let plain = |kind: ColumnKind| d.ddl_type(&ColumnType::Plain { kind });
    assert_eq!(plain(ColumnKind::Float).unwrap(), "FLOAT");
    assert_eq!(plain(ColumnKind::Double).unwrap(), "DOUBLE");
    assert_eq!(plain(ColumnKind::Bit).unwrap(), "BIT");
    assert_eq!(plain(ColumnKind::Time).unwrap(), "TIME(9)");
    assert_eq!(plain(ColumnKind::TimestampNs).unwrap(), "TIMESTAMP(9)");
    assert_eq!(
        plain(ColumnKind::Object).unwrap_err(),
        Refusal::illegal_state("no DDL spelling for declared column type OBJECT")
    );
    assert_eq!(d.ddl_type(&ColumnType::Sized { kind: "VARCHAR".into(), size: 20 }).unwrap(), "VARCHAR(20)");
    assert_eq!(
        d.ddl_type(&ColumnType::Scaled { kind: "DECIMAL".into(), precision: 10, scale: 2 }).unwrap(),
        "DECIMAL(10, 2)"
    );
}

#[test]
fn a_staged_temporary_table_is_local_and_dropped_on_commit() {
    let d = H2::new();
    let ct = |staged: bool| SqlDml::CreateTempTableAs {
        table: "tmp".into(),
        query: one_projection(Projection { expr: int_lit(1), alias: Some("x".into()), out: None }),
        columns: vec!["x".into()],
        staged,
    };
    assert_eq!(
        SqlDialect::render_dml(&d, &ct(true)).unwrap(),
        "CREATE LOCAL TEMPORARY TABLE tmp ON COMMIT DROP TRANSACTIONAL AS SELECT 1 AS \"x\";"
    );
    assert_eq!(SqlDialect::render_dml(&d, &ct(false)).unwrap(), "CREATE TEMPORARY TABLE tmp AS SELECT 1 AS \"x\";");
}

#[test]
fn a_catalog_grid_reads_h2s_information_schema() {
    let outputs = ["TABLE_CAT", "TABLE_SCHEM", "TABLE_NAME", "COLUMN_NAME", "DATA_TYPE", "TYPE_NAME", "SQL_TYPE_NAME"]
        .iter()
        .map(|n| lite_sql::OutputCol {
            name: (*n).into(),
            r#type: SqlType::Scalar(Scalar::Varchar),
            nullable: true,
            origin: Origin::Derived,
        })
        .collect();
    let grid = SqlSource::Catalog {
        grid: CatalogGrid::Columns,
        schema_pattern: None,
        table_pattern: Some("T%".into()),
        column_pattern: None,
        alias: "c".into(),
        outputs,
    };
    let mut w = SqlWriter::new();
    H2::new().catalog(&mut w, &grid).unwrap();
    let sql = w.into_text().unwrap();
    assert!(
        sql.contains(
            "CASE COALESCE(declared_data_type, data_type) WHEN 'INTEGER' THEN 4 WHEN 'CHARACTER VARYING' THEN 12"
        ),
        "{sql}"
    );
    assert!(sql.contains(" ELSE 1111 END AS \"DATA_TYPE\", CASE WHEN declared_data_type = 'DECIMAL' THEN 'DECIMAL' ELSE data_type END AS \"TYPE_NAME\""), "{sql}");
    assert!(
        sql.contains(
            "WHEN 'BINARY VARYING' THEN 'VARBINARY' WHEN 'BINARY' THEN 'BINARY' ELSE 'OTHER' END AS \"SQL_TYPE_NAME\""
        ),
        "{sql}"
    );
    assert!(sql.ends_with(" AND upper(table_name) LIKE 'T%' ORDER BY 2, 3, ordinal_position"), "{sql}");
}

#[test]
fn json_kinds_and_typeof_names() {
    for d in [H2::new(), H2::modern()] {
        assert_eq!(d.json_kind_name(JsonKind::String).unwrap(), "VARCHAR");
        assert_eq!(d.json_kind_name(JsonKind::UnsignedInteger).unwrap(), "UBIGINT");
        assert_eq!(d.typeof_name(Scalar::Date).unwrap(), "DATE");
        // the literal "H2", on H2Modern too
        assert_eq!(
            d.typeof_name(Scalar::Varchar).unwrap_err(),
            capability("a TYPEOF compared with VARCHAR's name reached H2")
        );
    }
}

#[test]
fn the_passes_are_h2s_in_its_order() {
    let names = |ps: Vec<Box<dyn SqlRewriter>>| ps.iter().map(|p| p.name()).collect::<Vec<_>>();
    let pipeline = [
        "CarrierStrategies",
        "ConstantLateralsAsRows",
        "LateralExplodeToUnion",
        "AggregatesDeliverDouble",
        "SourceSpelling",
    ];
    assert_eq!(names(H2::new().passes()), pipeline);
    assert_eq!(names(H2::modern().passes()), pipeline);
    // QUALIFY wrapped only where it is not native: never on H2
    let mut wrapped = pipeline.to_vec();
    wrapped.insert(1, "QualifyToSubselect");
    assert_eq!(names(h2_passes(false)), wrapped);
    // render runs the Float expansions first and the stored reads last
    let all = names(ansi::render_passes(&H2::new()));
    assert_eq!(all.first(), Some(&"FloatPrintText.Expansion"));
    assert_eq!(all.last(), Some(&"StoredReads"));
}
