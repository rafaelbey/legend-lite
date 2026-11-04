// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `Postgres` (`core/src/main/java/com/legend/sql/dialect/Postgres.java`), the PostgreSQL 16+ execution dialect:
//! the base renderer with every identifier the query writes quoted (Postgres folds a bare name to lower case) and a
//! store's bare name folded (`physicalName`, decision D8), every `SqlFn` and aggregate decided by an exhaustive match,
//! lists as native arrays (a nested list, a struct or a map element held as jsonb), structs and Variants as jsonb, the
//! raise as a TIMESTAMPTZ cast of the sentinel-wrapped message, and -- as its [`crate::SqlDialect`] -- its session,
//! scripts, catalog (`pg_catalog`'s type names), keys snapshot and host operations (`PostgresHostOperations`:
//! [`crate::host_ops::PostgresHostOperations`]).
//!
//! Java's `SqlWriter.Piece` -- SQL that writes itself where a helper writes it, as often as it does -- is [`P`], a
//! shared closure: the helpers compose pieces exactly as Java's lambdas do, and what Java evaluates when a piece is
//! made (a type's name, a string literal) is evaluated when Rust makes it.

use crate::ansi::{self, cast_type_name, part};
use crate::catalog_model::{CatalogColumn, CatalogRules, CatalogType};
use crate::dialect::KeysSnapshot;
use crate::host_ops::{HostOperations, PostgresHostOperations};
use crate::java::{simple_name, sql_type_text, type_fact_text};
use crate::refusal::{Refusal, arg, tail};
use crate::renderer::{AnsiSqlRenderer, AnsiState, Lexicon};
use crate::rewriter::SqlRewriter;
use crate::statement::{Digits, RenderedStatement, TypeSpelling};
use crate::writer::SqlWriter;
use crate::{build, carrier, raw_sql};
use lite_sql::{
    AggFn, Bound, Column, ColumnKind, ColumnType, Cte, DateFmt, DateFmtPart, DatePart, Frame, JsonArrayAggKey,
    JsonKind, Lambda, NameQuoting, Origin, Projection, Reducer, Scalar, SortKey, SqlAgg, SqlDdl, SqlDml, SqlExpr,
    SqlFn, SqlQuery, SqlType, StructField, StructLitField, TypeFact, ValueKind, When,
};
use std::rc::Rc;
use std::sync::LazyLock;

/// The PostgreSQL dialect: one per render, as Java's `new Postgres()`. It carries no state across renders.
#[derive(Debug, Default)]
pub struct Postgres {
    ansi: AnsiState,
}

impl Postgres {
    /// Java's `new Postgres()`.
    pub fn new() -> Postgres {
        Postgres::default()
    }
}

/// The documented reserved set (the SQL key words appendix, "reserved" in the PostgreSQL column), Java's `LEXICON`'s
/// words. Postgres quotes EVERY identifier, so this list never decides a spelling there; it is the reserved set for any
/// reader that asks (a store's bare name a reserved word is delimited by `physicalName`).
pub const RESERVED: &[&str] = &[
    "all",
    "analyse",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "asymmetric",
    "authorization",
    "binary",
    "both",
    "case",
    "cast",
    "check",
    "collate",
    "collation",
    "column",
    "concurrently",
    "constraint",
    "create",
    "cross",
    "current_catalog",
    "current_date",
    "current_role",
    "current_schema",
    "current_time",
    "current_timestamp",
    "current_user",
    "default",
    "deferrable",
    "desc",
    "distinct",
    "do",
    "else",
    "end",
    "except",
    "false",
    "fetch",
    "for",
    "foreign",
    "freeze",
    "from",
    "full",
    "grant",
    "group",
    "having",
    "ilike",
    "in",
    "initially",
    "inner",
    "intersect",
    "into",
    "is",
    "isnull",
    "join",
    "lateral",
    "leading",
    "left",
    "like",
    "limit",
    "localtime",
    "localtimestamp",
    "natural",
    "not",
    "notnull",
    "null",
    "offset",
    "on",
    "only",
    "or",
    "order",
    "outer",
    "overlaps",
    "placing",
    "primary",
    "references",
    "returning",
    "right",
    "select",
    "session_user",
    "similar",
    "some",
    "symmetric",
    "system_user",
    "table",
    "tablesample",
    "then",
    "to",
    "trailing",
    "true",
    "union",
    "unique",
    "user",
    "using",
    "variadic",
    "verbose",
    "when",
    "where",
    "window",
    "with",
];

/// Java's `Postgres.LEXICON`: `"` and [`RESERVED`].
static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| Lexicon::new('"', RESERVED));

/// Java's `Postgres.CAPS`: the portable carrier strategies (no native-list carrier: jsonb), correlated explosion, the
/// JSON carrier, and FULL OUTER natively (no H2 emulation).
pub const CAPS: crate::carrier::Caps =
    crate::carrier::Caps { native_lists: false, correlated_explode: true, json_carrier: true, native_full_outer: true };

/// Java's `DOUBLE_AGGREGATES`: the Float aggregates Postgres answers as numeric over an integer or a numeric, cast over
/// their exact result (`AggregatesDeliverDouble`), grouped or windowed.
pub const DOUBLE_AGGREGATES: &[AggFn] = &[
    AggFn::Avg,
    AggFn::StddevSamp,
    AggFn::StddevPop,
    AggFn::Stddev,
    AggFn::VarSamp,
    AggFn::VarPop,
    AggFn::Variance,
    AggFn::Corr,
    AggFn::CovarSamp,
    AggFn::CovarPop,
];

/// Every built-in Postgres 17 base type (its `pg_catalog` name, upper-cased) and every information_schema kind,
/// decided (Java's `CATALOG_TYPES`, in its order): a number, a text, a date or a timestamp declared as itself; json and
/// jsonb a Variant; bytes left out; every other type OTHER, a Pure String read as its text.
pub fn catalog_types() -> Vec<(&'static str, CatalogType)> {
    let text = || CatalogType::as_stored("VARCHAR(4096)");
    let mut m = vec![
        ("BOOL", CatalogType::as_stored("BIT")),
        ("INT2", CatalogType::as_stored("SMALLINT")),
        ("INT4", CatalogType::as_stored("INTEGER")),
        ("INT8", CatalogType::as_stored("BIGINT")),
        ("FLOAT4", CatalogType::as_stored("REAL")),
        ("FLOAT8", CatalogType::as_stored("DOUBLE")),
        ("TEXT", text()),
        ("VARCHAR", text()),
        ("BPCHAR", text()),
        ("NAME", text()),
        ("DATE", CatalogType::as_stored("DATE")),
        ("TIMESTAMP", CatalogType::as_stored("TIMESTAMP")),
        // a zoned timestamp is its UTC instant: read in place under the UTC session the attach pins, and a copy (in
        // the tab's DuckDB, where conversions run) holds its UTC wall time
        ("TIMESTAMPTZ", CatalogType::copy_converted("TIMESTAMP", "CAST(timezone('UTC', %s) AS TIMESTAMP)")),
        ("JSON", CatalogType::as_stored("SEMISTRUCTURED")),
        ("JSONB", CatalogType::as_stored("SEMISTRUCTURED")),
        ("BYTEA", CatalogType::left_out("bytes: no Pure Database type holds them")),
    ];
    for t in OTHER_TYPES {
        m.push((t, CatalogType::as_stored("OTHER")));
    }
    m
}

/// The types Java's `catalogTypes()` declares OTHER, in its order.
const OTHER_TYPES: &[&str] = &[
    // the kinds outside pg_catalog
    "ARRAY",
    "ENUM",
    "COMPOSITE",
    "RANGE",
    "USER-DEFINED",
    // numbers Pure cannot hold as numbers: money is formatted, an oid an identifier
    "MONEY",
    "OID",
    "REGCLASS",
    "REGCOLLATION",
    "REGCONFIG",
    "REGDICTIONARY",
    "REGNAMESPACE",
    "REGOPER",
    "REGOPERATOR",
    "REGPROC",
    "REGPROCEDURE",
    "REGROLE",
    "REGTYPE",
    // times of day and spans
    "TIME",
    "TIMETZ",
    "INTERVAL",
    // geometric, network, bit strings
    "POINT",
    "LINE",
    "LSEG",
    "BOX",
    "PATH",
    "POLYGON",
    "CIRCLE",
    "INET",
    "CIDR",
    "MACADDR",
    "MACADDR8",
    "BIT",
    "VARBIT",
    // built-in ranges and multiranges
    "INT4RANGE",
    "INT8RANGE",
    "NUMRANGE",
    "DATERANGE",
    "TSRANGE",
    "TSTZRANGE",
    "INT4MULTIRANGE",
    "INT8MULTIRANGE",
    "NUMMULTIRANGE",
    "DATEMULTIRANGE",
    "TSMULTIRANGE",
    "TSTZMULTIRANGE",
    // the rest of pg_catalog's base types
    "UUID",
    "XML",
    "JSONPATH",
    "TSVECTOR",
    "TSQUERY",
    "GTSVECTOR",
    "CHAR",
    "ACLITEM",
    "CID",
    "TID",
    "XID",
    "XID8",
    "PG_LSN",
    "PG_SNAPSHOT",
    "TXID_SNAPSHOT",
    "REFCURSOR",
    "PG_NODE_TREE",
    "PG_NDISTINCT",
    "PG_DEPENDENCIES",
    "PG_MCV_LIST",
    "PG_BRIN_BLOOM_SUMMARY",
    "PG_BRIN_MINMAX_MULTI_SUMMARY",
];

/// Postgres's catalog decisions as one [`CatalogRules`] (Java's `CATALOG_RULES`): no alias, nothing refused by name,
/// an unconstrained numeric a DOUBLE.
pub static CATALOG_RULES: LazyLock<CatalogRules> = LazyLock::new(|| CatalogRules {
    database: "Postgres",
    types: catalog_types(),
    aliases: Vec::new(),
    refused: Vec::new(),
    decimal: "NUMERIC",
    unsized_decimal: Some(CatalogType::as_stored("DOUBLE")),
});

// ==================================================================
// Pieces and the type facts the arms read
// ==================================================================

/// Java's `SqlWriter.Piece`: SQL that writes itself, shareable among the helpers that write it.
pub type P<'a> = Rc<dyn Fn(&mut SqlWriter) -> Result<(), Refusal> + 'a>;

fn pc<'a>(f: impl Fn(&mut SqlWriter) -> Result<(), Refusal> + 'a) -> P<'a> {
    Rc::new(f)
}

/// A piece of fixed text.
fn text<'a>(s: impl Into<String>) -> P<'a> {
    let s = s.into();
    pc(move |w| {
        w.append(&s);
        Ok(())
    })
}

/// Java's `typeOf(e)`: the type a node is typed as, or none.
fn type_of(e: &SqlExpr) -> Option<&SqlType> {
    fact_type(e.r#type())
}

fn fact_type(f: &TypeFact) -> Option<&SqlType> {
    match f {
        TypeFact::Typed { r#type, .. } => Some(r#type),
        _ => None,
    }
}

/// Java's `listElement(e)`: the element type of a list, or none for a value that is no list.
fn list_element(e: &SqlExpr) -> Option<&SqlType> {
    fact_list_element(e.r#type())
}

fn fact_list_element(f: &TypeFact) -> Option<&SqlType> {
    match fact_type(f) {
        Some(SqlType::Array { element }) => Some(element),
        _ => None,
    }
}

/// A value Postgres holds as itself inside a list: a scalar (a Variant, a struct, a map or a list is held as jsonb).
fn held_as_itself(t: &SqlType) -> bool {
    matches!(t, SqlType::Decimal { .. }) || matches!(t, SqlType::Scalar(sc) if *sc != Scalar::Json)
}

/// Whether a list holds a value of type `t` as jsonb and rebuilds it when read: a list, or a map.
fn rebuilt(t: Option<&SqlType>) -> bool {
    matches!(t, Some(SqlType::Array { .. } | SqlType::Map { .. }))
}

fn json_type() -> SqlType {
    SqlType::Scalar(Scalar::Json)
}

/// The type a list's elements are held as: their own when held as themselves, jsonb otherwise.
fn held_type(list: &SqlExpr) -> Result<SqlType, Refusal> {
    let Some(e) = list_element(list) else {
        return Err(Refusal::null_pointer("a list's element type"));
    };
    Ok(if held_as_itself(e) { e.clone() } else { json_type() })
}

fn is_scalar(e: &SqlExpr, s: Scalar) -> bool {
    matches!(type_of(e), Some(SqlType::Scalar(t)) if *t == s)
}

fn is_double(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Double)
}

fn is_boolean(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Boolean)
}

fn is_text(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Varchar)
}

fn is_json(e: &SqlExpr) -> bool {
    is_scalar(e, Scalar::Json)
}

fn is_numeric(e: &SqlExpr) -> bool {
    matches!(
        type_of(e),
        Some(
            SqlType::Scalar(Scalar::Integer | Scalar::Bigint | Scalar::Hugeint | Scalar::Double)
                | SqlType::Decimal { .. }
        )
    )
}

/// The calls over the empty collection whose answer is the empty collection (Java's `EMPTY_IN_EMPTY_OUT`).
const EMPTY_IN_EMPTY_OUT: &[SqlFn] = &[
    SqlFn::ListMin,
    SqlFn::ListMax,
    SqlFn::ListSum,
    SqlFn::ListAvg,
    SqlFn::ListMedian,
    SqlFn::ListMode,
    SqlFn::ListGet,
    SqlFn::ListSlice,
    SqlFn::ListTail,
    SqlFn::ListInit,
    SqlFn::ListSort,
    SqlFn::ListSortDesc,
    SqlFn::ListReverse,
    SqlFn::ListDistinct,
];

/// The empty list literal, typed or not.
fn empty_list_literal(e: &SqlExpr) -> bool {
    let v = match e {
        SqlExpr::Cast { value, target: SqlType::Array { .. }, .. } => value,
        other => other,
    };
    matches!(v, SqlExpr::ArrayLit { elements, .. } if elements.is_empty())
}

/// The empty collection where a list goes: a NULL of no type (the IR's Bottom), compacted or not, or one of the
/// [`EMPTY_IN_EMPTY_OUT`] calls over it, which the IR types Bottom too.
fn empty_collection(e: &SqlExpr) -> bool {
    match e {
        SqlExpr::CompactList { list, .. } => empty_collection(list),
        SqlExpr::Call { r#fn, args, r#type } => empty_call(*r#fn, args, r#type),
        SqlExpr::NullLit { r#type } => matches!(r#type, TypeFact::Bottom),
        _ => false,
    }
}

fn empty_call(f: SqlFn, args: &[SqlExpr], t: &TypeFact) -> bool {
    matches!(t, TypeFact::Bottom) && EMPTY_IN_EMPTY_OUT.contains(&f) && !args.is_empty() && empty_collection(&args[0])
}

/// The element type of `list`; refused by name when it is no list of a known element type.
fn element_of<'e>(list: &'e SqlExpr, what: &str) -> Result<&'e SqlType, Refusal> {
    list_element(list).ok_or_else(|| {
        Refusal::capability(format!(
            "{what} over {} reached Postgres: a list of unknown element type",
            type_fact_text(list.r#type())
        ))
    })
}

/// `x` as Java's `(SqlExpr.Lambda) x` reads it.
fn lambda_arg(x: &SqlExpr) -> Result<&Lambda, Refusal> {
    match x {
        SqlExpr::Lambda(l) => Ok(l),
        other => Err(Refusal::class_cast(simple_name(other), "Lambda")),
    }
}

/// A text literal under a cast, spelled as a text first (Java's `typedText`): Postgres reads a bare literal under a
/// cast as a constant of the cast's type when it parses the statement, before any CASE around it is weighed. Any other
/// value is itself.
fn typed_text(x: &SqlExpr) -> SqlExpr {
    match x {
        SqlExpr::StringLit { .. } => build::cast(x.clone(), SqlType::Scalar(Scalar::Varchar)),
        other => other.clone(),
    }
}

fn wall(f: SqlFn, why: &str) -> Refusal {
    Refusal::capability(format!("{f} reached Postgres: {why}"))
}

/// Whether `e` holds a call that draws afresh at each reading (the database's random, a uuid).
fn draws_afresh(e: &SqlExpr) -> bool {
    if let SqlExpr::Call { r#fn: SqlFn::Random | SqlFn::Guid, .. } = e {
        return true;
    }
    crate::typing::children(e).iter().any(|k| draws_afresh(k))
}

/// A date part as Postgres names it: `extract`'s field, `date_trunc`'s unit.
fn date_part(p: DatePart) -> Result<&'static str, Refusal> {
    Ok(match p {
        DatePart::Year => "year",
        DatePart::Quarter => "quarter",
        DatePart::Month => "month",
        DatePart::Week => "week",
        DatePart::Day => "day",
        DatePart::Hour => "hour",
        DatePart::Minute => "minute",
        DatePart::Second => "second",
        DatePart::Millisecond => "millisecond",
        DatePart::Microsecond => "microsecond",
        DatePart::Nanosecond => {
            return Err(Refusal::capability(
                "a nanosecond part: Postgres keeps microseconds (the Cypher island's nanosecond leg is DuckDB's)",
            ));
        }
        DatePart::DayOfYear => "doy",
        DatePart::IsoDayOfWeek => "isodow",
    })
}

/// An interval's unit as Postgres names it (`INTERVAL '1 day'`).
fn interval_unit(unit: DatePart) -> Result<&'static str, Refusal> {
    Ok(match unit {
        DatePart::Year => "year",
        DatePart::Month => "month",
        DatePart::Week => "week",
        DatePart::Day => "day",
        DatePart::Hour => "hour",
        DatePart::Minute => "minute",
        DatePart::Second => "second",
        DatePart::Millisecond => "millisecond",
        DatePart::Microsecond => "microsecond",
        DatePart::Quarter | DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
            return Err(Refusal::capability(format!("interval unit '{unit}' has no Postgres spelling")));
        }
    })
}

/// date_bin's fixed-length units and their origins (Java's `BUCKET_ORIGINS`; month and year are absent).
fn bucket_origin(unit: &str) -> Option<&'static str> {
    match unit {
        "day" | "hour" | "minute" | "second" | "millisecond" | "microsecond" => Some("TIMESTAMP '1970-01-01 00:00:00'"),
        "week" => Some("TIMESTAMP '1969-12-29 00:00:00'"),
        _ => None,
    }
}

/// The calendar units date_bin cannot bin, by their length in months (Java's `CALENDAR_UNITS`).
fn calendar_months(unit: &str) -> Option<i32> {
    match unit {
        "month" => Some(1),
        "year" => Some(12),
        _ => None,
    }
}

/// A window frame's interval offset (Java's `frameInterval`): the SQL standard's quoted singular form, a week as 7
/// days; `Math.multiplyExact` refuses an overflow.
fn frame_interval(n: i64, unit: &str) -> Result<String, Refusal> {
    let u = unit.to_uppercase();
    let qualifier = match u.as_str() {
        "YEARS" => "YEAR",
        "MONTHS" => "MONTH",
        "WEEKS" | "DAYS" => "DAY",
        "HOURS" => "HOUR",
        "MINUTES" => "MINUTE",
        "SECONDS" => "SECOND",
        _ => {
            return Err(Refusal::capability(format!(
                "a {unit} window frame offset has no Postgres interval qualifier"
            )));
        }
    };
    let per: i64 = if u == "WEEKS" { 7 } else { 1 };
    let Some(v) = n.checked_mul(per) else {
        return Err(Refusal { class: "java.lang.ArithmeticException", message: "long overflow".to_owned() });
    };
    Ok(format!("INTERVAL '{v}' {qualifier}"))
}

/// How a stored type reads as jsonb (Java's `JsonbRead`): json is cast; an array or a composite is converted; any
/// other type is not nested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonbRead {
    Cast,
    Convert,
    None,
}

fn jsonb_read(t: &ColumnType) -> JsonbRead {
    match t {
        ColumnType::Plain { kind } => match kind {
            ColumnKind::Json => JsonbRead::Cast,
            ColumnKind::Array | ColumnKind::Object => JsonbRead::Convert,
            ColumnKind::Bigint
            | ColumnKind::Smallint
            | ColumnKind::Tinyint
            | ColumnKind::Integer
            | ColumnKind::Float
            | ColumnKind::Double
            | ColumnKind::Real
            | ColumnKind::Bit
            | ColumnKind::Timestamp
            | ColumnKind::Date
            | ColumnKind::Time
            | ColumnKind::TimestampNs
            | ColumnKind::Varchar
            | ColumnKind::Other
            | ColumnKind::Distinct => JsonbRead::None,
        },
        ColumnType::Sized { .. } | ColumnType::Scaled { .. } => JsonbRead::None,
    }
}

/// `[a-z_][a-z0-9_]*`, the whole name (Java's `BARE`).
fn bare(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(b) if b.is_ascii_lowercase() || b == b'_' => {
            bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        }
        _ => false,
    }
}

/// A quote-bearing name that is already its own spelling: `"..."`, its interior a valid quoted body.
fn pre_quoted(name: &str) -> bool {
    name.len() > 1
        && name.starts_with('"')
        && name.ends_with('"')
        && !name[1..name.len() - 1].replace("\"\"", "").contains('"')
}

/// RE2's inline flags at the front of a pattern (Java's `RE2_FLAGS`, `\(\?([ims]+)\)`): the flags and the index after
/// the group, matched at the start (`lookingAt`).
fn re2_flags(s: &str) -> Option<(&str, usize)> {
    let rest = s.strip_prefix("(?")?;
    let n = rest.bytes().take_while(|b| matches!(b, b'i' | b'm' | b's')).count();
    if n == 0 || rest.as_bytes().get(n) != Some(&b')') {
        return None;
    }
    Some((&rest[..n], n + 3))
}

/// One printf directive the format arm reads (Java's `DIRECTIVE`, `%(%|(0?)([0-9]*)(?:\.([0-9]+))?([sdf]))`), at
/// `start`.
struct Directive<'t> {
    start: usize,
    end: usize,
    /// `%%`
    percent: bool,
    zero: bool,
    width: &'t str,
    precision: Option<&'t str>,
    conversion: u8,
}

/// The directive matched at `i` (a `%`), or none.
fn directive_at(s: &str, i: usize) -> Option<Directive<'_>> {
    let b = s.as_bytes();
    if b.get(i) != Some(&b'%') {
        return None;
    }
    let mut j = i + 1;
    if b.get(j) == Some(&b'%') {
        return Some(Directive {
            start: i,
            end: j + 1,
            percent: true,
            zero: false,
            width: "",
            precision: None,
            conversion: b'%',
        });
    }
    let zero = b.get(j) == Some(&b'0');
    if zero {
        j += 1;
    }
    let w0 = j;
    while b.get(j).is_some_and(u8::is_ascii_digit) {
        j += 1;
    }
    let width = &s[w0..j];
    let mut precision = None;
    if b.get(j) == Some(&b'.') {
        let p0 = j + 1;
        let mut k = p0;
        while b.get(k).is_some_and(u8::is_ascii_digit) {
            k += 1;
        }
        if k > p0 && matches!(b.get(k), Some(b's' | b'd' | b'f')) {
            precision = Some(&s[p0..k]);
            j = k;
        }
    }
    match b.get(j) {
        Some(&c @ (b's' | b'd' | b'f')) => {
            Some(Directive { start: i, end: j + 1, percent: false, zero, width, precision, conversion: c })
        }
        _ => None,
    }
}

/// Every directive of `s`, as `Matcher.find` finds them in turn.
fn directives(s: &str) -> Vec<Directive<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        match directive_at(s, i) {
            Some(d) => {
                i = d.end;
                out.push(d);
            }
            None => i += 1,
        }
    }
    out
}

/// Java's `Integer.parseInt` of a run of ASCII digits.
fn parse_int(s: &str) -> Result<i32, Refusal> {
    s.parse::<i32>()
        .map_err(|_| Refusal { class: Refusal::NUMBER_FORMAT, message: format!("For input string: \"{s}\"") })
}

// ==================================================================
// The dialect's own helpers (Java's private methods)
// ==================================================================

impl Postgres {
    /// `e`, written at `prec`, as a piece.
    fn ex<'a>(&'a self, e: &'a SqlExpr, prec: i32) -> P<'a> {
        pc(move |w| {
            w.expr(self, e, prec)?;
            Ok(())
        })
    }

    /// `a`'s `i`-th argument, written at `prec`, as a piece: read when written (Java's `w -> w.expr(a.get(i), prec)`).
    fn arg_ex<'a>(&'a self, a: &'a [SqlExpr], i: usize, prec: i32) -> P<'a> {
        pc(move |w| {
            w.expr(self, arg(a, i)?, prec)?;
            Ok(())
        })
    }

    /// `e` cast to DOUBLE PRECISION, as a piece.
    fn as_double<'a>(&'a self, e: &'a SqlExpr) -> P<'a> {
        pc(move |w| {
            w.append("CAST(").expr(self, e, 0)?.append(" AS DOUBLE PRECISION)");
            Ok(())
        })
    }

    /// The `i`-th lambda parameter, spelled (Java's `param(l.params().get(i))`).
    fn param_at(&self, params: &[String], i: usize) -> Result<String, Refusal> {
        match params.get(i) {
            Some(p) => self.param(p),
            None => Err(Refusal {
                class: Refusal::INDEX_OUT_OF_BOUNDS,
                message: format!("Index {i} out of bounds for length {}", params.len()),
            }),
        }
    }

    /// An owned expression, written at `prec`, as a piece.
    fn ex_owned<'a>(&'a self, e: SqlExpr, prec: i32) -> P<'a> {
        pc(move |w| {
            w.expr(self, &e, prec)?;
            Ok(())
        })
    }

    /// The SQL type an element of type `t` is held as in a list (Java's `carrier`).
    fn carrier(&self, t: &SqlType) -> Result<String, Refusal> {
        if held_as_itself(t) { cast_type_name(self, t) } else { Ok("JSONB".to_owned()) }
    }

    /// `list`, as a piece, when it is a list; refused by name otherwise, at once (Java's `listOf`).
    fn list_of<'a>(&'a self, list: &'a SqlExpr, what: &str) -> Result<P<'a>, Refusal> {
        element_of(list, what)?;
        Ok(self.ex(list, 0))
    }

    /// A value of type `t` as jsonb: a struct or a Variant is jsonb already, anything else converted, as its type.
    fn as_jsonb<'a>(&'a self, v: P<'a>, t: &SqlType) -> Result<P<'a>, Refusal> {
        if matches!(t, SqlType::Struct { .. }) || *t == json_type() {
            return Ok(v);
        }
        let ty = match t {
            SqlType::Array { element } => format!("{}[]", self.carrier(element)?),
            other => cast_type_name(self, other)?,
        };
        Ok(pc(move |w| {
            w.append("to_jsonb(CAST(");
            v(w)?;
            w.append(" AS ").append(&ty).append("))");
            Ok(())
        }))
    }

    /// `entries` when their keys are each non-NULL and distinct; DuckDB's error otherwise.
    fn keys_checked<'a>(&'a self, entries: P<'a>) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN (SELECT count(*) FROM ");
            unnest(entries.clone())(w)?;
            w.append(" AS __mk(e) WHERE jsonb_typeof(e -> 'key') = 'null') > 0 THEN ");
            self.map_raise("Map keys can not be NULL.")(w)?;
            w.append(" WHEN (SELECT count(DISTINCT e -> 'key') FROM ");
            unnest(entries.clone())(w)?;
            w.append(" AS __mk(e)) < cardinality(");
            entries(w)?;
            w.append(") THEN ");
            self.map_raise("Map keys must be unique.")(w)?;
            w.append(" ELSE ");
            entries(w)?;
            w.append(" END)");
            Ok(())
        })
    }

    /// DuckDB's map error, raised as a map (the CASE's type).
    fn map_raise<'a>(&'a self, message: &str) -> P<'a> {
        let raised = self.raise(vec![build::string_lit(message)], None, true);
        pc(move |w| {
            w.append("CAST(");
            raised(w)?;
            w.append(" AS JSONB[])");
            Ok(())
        })
    }

    /// The map calls: a map is its entries in order, a `JSONB[]` of `{"key": k, "value": v}`.
    fn maps(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        match f {
            SqlFn::MapEmpty => {
                w.append("CAST(ARRAY[] AS JSONB[])");
            }
            // each entry a struct of two fields (a pair's first and second), by position whatever their names
            SqlFn::MapFromEntries => {
                let xs = arg(a, 0)?;
                let entry = match list_element(xs) {
                    Some(SqlType::Struct { fields }) if fields.len() == 2 => fields,
                    other => {
                        let what = other.map_or_else(|| "null".to_owned(), sql_type_text);
                        return Err(wall(f, &format!("entries of {what}")));
                    }
                };
                let k = self.string_lit(&entry[0].name)?;
                let v = self.string_lit(&entry[1].name)?;
                let xs_p = self.ex(xs, 0);
                let entries = pc(move |w| {
                    w.append(&format!(
                        "ARRAY(SELECT jsonb_build_object('key', __me -> {k}, 'value', __me -> {v}) FROM "
                    ));
                    unnest(xs_p.clone())(w)?;
                    w.append(" WITH ORDINALITY AS __mu(__me, __mo) ORDER BY __mo)");
                    Ok(())
                });
                w.append("(CASE WHEN ").expr(self, xs, 4)?.append(" IS NULL THEN NULL ELSE ");
                self.keys_checked(entries)(w)?;
                w.append(" END)");
            }
            // DuckDB's map(ks, vs): the keys zipped with the values by position
            SqlFn::MapFromLists => {
                let ks = arg(a, 0)?;
                let vs = arg(a, 1)?;
                let kt = element_of(ks, f.as_str())?.clone();
                let vt = element_of(vs, f.as_str())?.clone();
                let ks_p = self.ex(ks, 0);
                let entries = pc(move |w| {
                    w.append("ARRAY(SELECT jsonb_build_object('key', ");
                    self.as_jsonb(text("__mk"), &kt)?(w)?;
                    w.append(", 'value', ");
                    let at = pc(move |w| {
                        w.append("(").expr(self, vs, 0)?.append(")[__mo]");
                        Ok(())
                    });
                    self.as_jsonb(at, &vt)?(w)?;
                    w.append(") FROM ");
                    unnest(ks_p.clone())(w)?;
                    w.append(" WITH ORDINALITY AS __mu(__mk, __mo) ORDER BY __mo)");
                    Ok(())
                });
                w.append("(CASE WHEN ").expr(self, ks, 4)?.append(" IS NULL OR ").expr(self, vs, 4)?;
                w.append(" IS NULL THEN NULL WHEN cardinality(").expr(self, ks, 0)?.append(") <> cardinality(");
                w.expr(self, vs, 0)?.append(") THEN ");
                self.map_raise("The map key list does not align with the map value list.")(w)?;
                w.append(" ELSE ");
                self.keys_checked(entries)(w)?;
                w.append(" END)");
            }
            // the entries of every map, each key at its first place (map, then position) with the last map's value
            SqlFn::MapConcat => {
                w.append(
                    "ARRAY(SELECT jsonb_build_object('key', __mg.k, 'value', (array_agg(__mg.v ORDER BY __mg.s \
                     DESC))[1]) FROM (",
                );
                for (i, map) in a.iter().enumerate() {
                    w.append(if i > 0 { " UNION ALL " } else { "" })
                        .append(&format!("SELECT {i} AS s, __me -> 'key' AS k, __me -> 'value' AS v, __mo AS o FROM "));
                    unnest(self.ex(map, 0))(w)?;
                    w.append(&format!(" WITH ORDINALITY AS __mu{i}(__me, __mo)"));
                }
                w.append(") AS __mg GROUP BY __mg.k ORDER BY min(__mg.s), (array_agg(__mg.o ORDER BY __mg.s))[1])");
            }
            SqlFn::MapKeys | SqlFn::MapValues | SqlFn::MapExtract => {
                let m = arg(a, 0)?;
                let Some(SqlType::Map { key, value }) = type_of(m) else {
                    return Err(wall(f, "a map of unknown key and value types"));
                };
                let keys = f == SqlFn::MapKeys;
                w.append("(CASE WHEN ").expr(self, m, 4)?.append(" IS NULL THEN NULL ELSE ARRAY(SELECT ");
                let entry = text(if keys { "__me -> 'key'" } else { "__me -> 'value'" });
                self.held(entry, if keys { key } else { value }, 0)(w)?;
                w.append(" FROM ");
                unnest(self.ex(m, 0))(w)?;
                w.append(" WITH ORDINALITY AS __mu(__me, __mo)");
                if f == SqlFn::MapExtract {
                    w.append(" WHERE __me -> 'key' = ");
                    self.as_jsonb(self.arg_ex(a, 1, 0), key)?(w)?;
                }
                w.append(" ORDER BY __mo) END)");
            }
            other => return Err(Refusal::illegal_state(format!("not a map call: {other}"))),
        }
        Ok(())
    }

    /// A value of type `t` read out of jsonb `json`: a scalar by its text, a list rebuilt as a native list, a struct
    /// NULL for jsonb's null too, anything else the jsonb itself.
    fn decode<'a>(&'a self, json: P<'a>, t: &SqlType, depth: usize) -> P<'a> {
        if held_as_itself(t) {
            let t = t.clone();
            return pc(move |w| {
                w.append("CAST((");
                json(w)?;
                w.append(" #>> '{}') AS ");
                let name = cast_type_name(self, &t)?;
                w.append(&name).append(")");
                Ok(())
            });
        }
        match t {
            // a map is its entries, a jsonb[]: rebuilt as a list of jsonb
            SqlType::Map { .. } => self.decode(json, &SqlType::Array { element: Box::new(json_type()) }, depth),
            // NULL for SQL's NULL or jsonb's null; any other value that is not an array is jsonb_array_elements'
            // error, as Pure's toMany raises one and DuckDB's cast does
            SqlType::Array { element } => {
                let e = format!("__e{depth}");
                let o = format!("__o{depth}");
                let inner = self.held(text(e.clone()), element, depth + 1);
                pc(move |w| {
                    w.append("(CASE WHEN ");
                    json(w)?;
                    w.append(" IS NULL OR jsonb_typeof(");
                    json(w)?;
                    w.append(") = 'null' THEN NULL ELSE ARRAY(SELECT ");
                    inner(w)?;
                    w.append(" FROM jsonb_array_elements(");
                    json(w)?;
                    w.append(&format!(") WITH ORDINALITY AS __j{depth}({e}, {o}) ORDER BY {o}")).append(") END)");
                    Ok(())
                })
            }
            // an instance: NULL for jsonb's null too, as a list's is
            SqlType::Struct { .. } => pc(move |w| {
                w.append("NULLIF(");
                json(w)?;
                w.append(", 'null'::jsonb)");
                Ok(())
            }),
            _ => json,
        }
    }

    /// An element of type `t` read from jsonb as a list holds it: a scalar decoded, anything else kept as jsonb.
    fn held<'a>(&'a self, json: P<'a>, t: &SqlType, depth: usize) -> P<'a> {
        if held_as_itself(t) { self.decode(json, t, depth) } else { json }
    }

    /// `{list}'s elements, each bound to `elem` as its type reads (a list element held as jsonb rebuilt), its 1-based
    /// position to `idx` (Java's `elementsFrom`).
    fn elements_from<'a>(&'a self, xs: P<'a>, element: &SqlType, elem: String, idx: String) -> P<'a> {
        if !rebuilt(Some(element)) {
            return pc(move |w| {
                unnest(xs.clone())(w)?;
                w.append(&format!(" WITH ORDINALITY AS __u({elem}, {idx})"));
                Ok(())
            });
        }
        let decoded = self.decode(text("__ue"), element, 0);
        pc(move |w| {
            w.append("(SELECT ");
            decoded(w)?;
            w.append(&format!(" AS {elem}, __uo AS {idx} FROM "));
            unnest(xs.clone())(w)?;
            w.append(" WITH ORDINALITY AS __w(__ue, __uo)) AS __u");
            Ok(())
        })
    }

    /// A lambda's body, its struct parameter's fields read as jsonb fields (Java's `bodyOver`): a field read off the
    /// parameter (`p.first`) becomes a field read of the jsonb object, typed as the struct declares it.
    fn body_over(l: &Lambda, element: &SqlType) -> Result<Lambda, Refusal> {
        if !matches!(element, SqlType::Struct { .. }) {
            return Ok(l.clone());
        }
        let Some(param) = l.params.first() else {
            return Err(Refusal {
                class: Refusal::INDEX_OUT_OF_BOUNDS,
                message: "Index 0 out of bounds for length 0".to_owned(),
            });
        };
        let body = struct_fields(&l.body, param, element)?.unwrap_or_else(|| l.body.as_ref().clone());
        Ok(build::lambda_of(l.params.clone(), body))
    }

    /// A reference to a lambda parameter (or an unnest's element or index), spelled as the body spells it.
    fn param(&self, name: &str) -> Result<String, Refusal> {
        self.column_ref(&Column {
            table: None,
            name: name.to_owned(),
            r#type: TypeFact::Unknown,
            origin: Some(Origin::Derived),
        })
    }

    /// `(CASE WHEN xs IS NULL THEN NULL ELSE ARRAY(SELECT select FROM <elements> [WHERE where] ORDER BY order) END)`.
    #[allow(clippy::too_many_arguments)]
    fn over_elements<'a>(
        &'a self,
        xs: P<'a>,
        element: SqlType,
        elem: String,
        idx: String,
        select: P<'a>,
        r#where: Option<P<'a>>,
        order: P<'a>,
    ) -> P<'a> {
        pc(move |w| {
            w.append("(CASE WHEN ");
            xs(w)?;
            w.append(" IS NULL THEN NULL ELSE ARRAY(SELECT ");
            select(w)?;
            w.append(" FROM ");
            self.elements_from(xs.clone(), &element, elem.clone(), idx.clone())(w)?;
            if let Some(wh) = &r#where {
                w.append(" WHERE ");
                wh(w)?;
            }
            w.append(" ORDER BY ");
            order(w)?;
            w.append(") END)");
            Ok(())
        })
    }

    /// The ORDER BY of a list's elements, each `x` of type `t`: a struct by its fields in their declared order (as
    /// DuckDB's), anything else by itself.
    fn sort_key_of<'a>(&'a self, x: P<'a>, t: &SqlType, direction: &'static str) -> P<'a> {
        let SqlType::Struct { fields } = t else {
            return pc(move |w| {
                x(w)?;
                w.append(direction);
                Ok(())
            });
        };
        let fields = fields.clone();
        pc(move |w| {
            w.join(&fields, ", ", |v, f| {
                let x = x.clone();
                let name = f.name.clone();
                let field = pc(move |u| {
                    u.append("(");
                    x(u)?;
                    let lit = self.string_lit(&name)?;
                    u.append(" -> ").append(&lit).append(")");
                    Ok(())
                });
                let read =
                    if matches!(f.r#type, SqlType::Struct { .. }) { field } else { self.decode(field, &f.r#type, 0) };
                self.sort_key_of(read, &f.r#type, direction)(v)
            })?;
            Ok(())
        })
    }

    /// A lambda's element and index parameters, spelled; the index is a fresh name when the lambda has none.
    fn lambda_params(&self, l: &Lambda) -> Result<(String, String), Refusal> {
        let element = self.param_at(&l.params, 0)?;
        let index = if l.params.len() > 1 { self.param_at(&l.params, 1)? } else { "__o".to_owned() };
        Ok((element, index))
    }

    /// A reduction of a list's elements: `(SELECT agg FROM unnest(xs) AS __u(__x))`.
    fn over_reduce<'a>(&'a self, xs: P<'a>, agg: String) -> P<'a> {
        pc(move |w| {
            w.append(&format!("(SELECT {agg} FROM "));
            unnest(xs.clone())(w)?;
            let x = self.param("__x")?;
            w.append(&format!(" AS __u({x}))"));
            Ok(())
        })
    }

    /// The element of `xs` at a 1-based index, as its type reads: a scalar itself, a list from its jsonb.
    fn element_at<'a>(&'a self, xs: &'a SqlExpr, index: &'static str, element: &SqlType) -> P<'a> {
        let at = pc(move |w| {
            w.append("(").expr(self, xs, 0)?.append(")[").append(index).append("]");
            Ok(())
        });
        if held_as_itself(element) { at } else { self.decode(at, element, 0) }
    }

    /// error(msg[, 'line:col']): the sentinel-wrapped message cast to TIMESTAMPTZ (which cannot parse it), then text,
    /// then the slot's own type where a CASE branch IS the raise; a raise whose value is a list or a map (the kind
    /// unnest takes), or one written where the planner evaluates, carries an empty VOLATILE term (Java's `raise`).
    fn raise<'a, A: AsRef<[SqlExpr]> + 'a>(&'a self, a: A, slot: Option<SqlType>, unnestable: bool) -> P<'a> {
        let text_piece = pc(move |w| {
            let a = a.as_ref();
            w.append("CAST(CAST(chr(31) || ");
            if a.len() > 1 {
                w.expr(self, &a[1], 0)?.append(" || chr(30) || ");
            }
            w.append("(").expr(self, arg(a, 0)?, 0)?.append(") || chr(31)");
            let volatile = unnestable || w.in_planned();
            w.append(if volatile { " || left(CAST(random() AS VARCHAR), 0)" } else { "" });
            w.append(" AS TIMESTAMPTZ) AS VARCHAR)");
            Ok(())
        });
        // typed as its slot (never evaluated: the TIMESTAMPTZ cast raises first) -- a list of scalars too
        let typed = match &slot {
            Some(SqlType::Scalar(_) | SqlType::Decimal { .. }) => true,
            Some(SqlType::Array { element }) => {
                matches!(element.as_ref(), SqlType::Scalar(_) | SqlType::Decimal { .. })
            }
            _ => false,
        };
        match slot {
            Some(slot) if typed => pc(move |w| {
                w.append("CAST(");
                text_piece(w)?;
                let name = cast_type_name(self, &slot)?;
                w.append(" AS ").append(&name).append(")");
                Ok(())
            }),
            _ => text_piece,
        }
    }

    /// `raise(a, slot)`: unnestable when the slot is a list or a map.
    fn raise_in<'a>(&'a self, a: &'a [SqlExpr], slot: Option<&SqlType>) -> P<'a> {
        let unnestable = matches!(slot, Some(SqlType::Array { .. } | SqlType::Map { .. }));
        self.raise(a, slot.cloned(), unnestable)
    }

    /// A CASE branch: one that IS error() raised in the CASE's own type.
    fn branch<'a>(&'a self, value: &'a SqlExpr, slot: Option<&SqlType>) -> P<'a> {
        match value {
            SqlExpr::Call { r#fn: SqlFn::Error, args, .. } => self.raise_in(args, slot),
            other => self.ex(other, 0),
        }
    }

    /// COALESCE: an empty list arm takes another arm's type (`'{}'`, which Postgres resolves against them).
    fn coalesce_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.iter().all(empty_list_literal) {
            w.function(self, "coalesce", a)?;
            return Ok(());
        }
        w.append("coalesce(").join(a, ", ", |w, x| {
            if empty_list_literal(x) {
                w.append("'{}'");
            } else {
                w.expr(self, x, 0)?;
            }
            Ok(())
        })?;
        w.append(")");
        Ok(())
    }

    /// SPLIT: DuckDB's string_split('', d) is [''], Postgres's string_to_array is {}.
    fn split(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("(CASE WHEN ").expr(self, arg(a, 0)?, 0)?.append(" = '' THEN ARRAY[''] ELSE string_to_array(");
        w.expr(self, arg(a, 0)?, 0)?.append(", ").expr(self, arg(a, 1)?, 0)?.append(") END)");
        Ok(())
    }

    /// STRUCT_INSERT: a struct with one field set, the jsonb object with the field replaced or added.
    fn struct_insert_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("(").expr(self, arg(a, 0)?, 0)?.append(" || jsonb_build_object(").expr(self, arg(a, 1)?, 0)?;
        w.append(", ").expr(self, arg(a, 2)?, 0)?.append("))");
        Ok(())
    }

    /// The LIST_CONCAT arm: a concatenation with the empty collection is its other lists.
    fn list_concat(&self, w: &mut SqlWriter, a: &[SqlExpr], t: &TypeFact, parent_prec: i32) -> Result<(), Refusal> {
        if !a.iter().any(empty_collection) {
            return self.list_call(w, SqlFn::ListConcat, a);
        }
        let lists: Vec<SqlExpr> = a.iter().filter(|e| !empty_collection(e)).cloned().collect();
        if lists.is_empty() {
            let Some(element) = fact_list_element(t) else {
                w.append("NULL");
                return Ok(());
            };
            let carrier = self.carrier(element)?;
            w.append(&format!("CAST(NULL AS {carrier}[])"));
            return Ok(());
        }
        if lists.len() == 1 {
            return w.expr(self, &lists[0], parent_prec).map(|_| ());
        }
        let concat = build::call(SqlFn::ListConcat, lists);
        let SqlExpr::Call { args, r#type, .. } = &concat else { unreachable!("a call") };
        self.call(w, SqlFn::ListConcat, args, r#type, parent_prec)
    }

    /// The RANGE_FN arm: DuckDB's range, [start, stop) by step (start 0, step 1 by default).
    fn range(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let start = if a.len() == 1 { text("0") } else { self.ex(arg(a, 0)?, 0) };
        let stop = self.ex(arg(a, if a.len() == 1 { 0 } else { 1 })?, 0);
        let step = if a.len() > 2 { self.ex(&a[2], 0) } else { text("1") };
        // (generate_series's arguments: Postgres's planner estimates its rows from them, so a raise within them is
        // written as no planner evaluates)
        w.append("ARRAY(SELECT g FROM generate_series(CAST(").planned(&*start)?.append(" AS BIGINT), CAST(");
        w.planned(&*stop)?.append(" AS BIGINT), CAST(").planned(&*step)?.append(" AS BIGINT)) AS g WHERE CASE WHEN ");
        step(w)?;
        w.append(" > 0 THEN g < ");
        stop(w)?;
        w.append(" ELSE g > ");
        stop(w)?;
        w.append(" END)");
        Ok(())
    }

    /// The LIST_TRANSFORM arm: each result held as the RESULT list holds it (its type is the call's).
    fn list_transform(&self, w: &mut SqlWriter, a: &[SqlExpr], t: &TypeFact) -> Result<(), Refusal> {
        let xs = arg(a, 0)?;
        let element = element_of(xs, SqlFn::ListTransform.as_str())?.clone();
        let Some(result) = fact_list_element(t).cloned() else {
            return Err(Refusal::capability("a list transform of unknown result type reached Postgres"));
        };
        let l = Postgres::body_over(lambda_arg(arg(a, 1)?)?, &element)?;
        let (p0, p1) = self.lambda_params(&l)?;
        let body = encode(self.ex_owned(*l.body, 0), Some(&result));
        let order = text(p1.clone());
        self.over_elements(self.ex(xs, 0), element, p0, p1, body, None, order)(w)
    }

    /// A date-time from its parts.
    fn make_timestamp(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() != 6 {
            return Err(Refusal::capability(format!(
                "make_timestamp of {} arguments has no Postgres spelling",
                a.len()
            )));
        }
        w.append("make_timestamp(").join(&a[..5], ", ", |w, x| {
            w.append("CAST(").expr(self, x, 0)?.append(" AS INTEGER)");
            Ok(())
        })?;
        w.append(", CAST(").expr(self, &a[5], 0)?.append(" AS DOUBLE PRECISION))");
        Ok(())
    }

    /// Pure's HALF_UP rounding: round over NUMERIC (half away from zero), a double cast back.
    fn round_half_up(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let r = pc(move |w| {
            w.append("round(CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS NUMERIC)");
            if a.len() > 1 {
                w.append(", CAST(").expr(self, &a[1], 0)?.append(" AS INTEGER)");
            }
            w.append(")");
            Ok(())
        });
        if is_double(arg(a, 0)?) {
            w.append("CAST(");
            r(w)?;
            w.append(" AS DOUBLE PRECISION)");
            return Ok(());
        }
        r(w)
    }

    /// DuckDB's time_bucket on Postgres: date_bin for fixed-length units, the month index for calendar ones.
    fn time_bucket(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let unit = interval_unit(part(arg(a, 0)?)?)?;
        if let Some(origin) = bucket_origin(unit) {
            w.append("date_bin(").expr(self, arg(a, 1)?, 6)?.append(" * INTERVAL '1 ").append(unit).append("', ");
            self.naive(arg(a, 2)?)(w)?;
            w.append(", ").append(origin).append(")");
            return Ok(());
        }
        let Some(months_per_unit) = calendar_months(unit) else {
            return Err(Refusal::capability(format!("a {unit} time bucket has no Postgres spelling")));
        };
        // the month index since 1970-01, floored to the bucket's size in months
        let t = self.naive(arg(a, 2)?);
        let n = arg(a, 1)?;
        let size = pc(move |w| {
            w.append("(CAST(").expr(self, n, 0)?.append(&format!(" AS INTEGER) * {months_per_unit})"));
            Ok(())
        });
        w.append("(TIMESTAMP '1970-01-01 00:00:00' + CAST(floor(((extract(year FROM ");
        t(w)?;
        w.append(") - 1970) * 12 + extract(month FROM ");
        t(w)?;
        w.append(") - 1) / ");
        size(w)?;
        w.append(") AS INTEGER) * ");
        size(w)?;
        w.append(" * INTERVAL '1 month')");
        Ok(())
    }

    /// exists/forAll over a list's elements, the empty answer for no element.
    fn list_predicate(&self, w: &mut SqlWriter, args: &[SqlExpr], agg: &str, empty: &str) -> Result<(), Refusal> {
        let xs = arg(args, 0)?;
        if empty_collection(xs) {
            // no element: the empty answer, as the coalesce below gives a NULL list
            w.append(empty);
            return Ok(());
        }
        let element = element_of(xs, "exists/forAll")?.clone();
        let l = Postgres::body_over(lambda_arg(arg(args, 1)?)?, &element)?;
        let (p0, p1) = self.lambda_params(&l)?;
        w.append(&format!("coalesce((SELECT {agg}(")).expr(self, &l.body, 0)?.append(") FROM ");
        self.elements_from(self.ex(xs, 0), &element, p0, p1)(w)?;
        w.append(&format!("), {empty})"));
        Ok(())
    }

    /// A list literal, typed (so an empty one has a type), each element held as the list holds it.
    fn array_literal(&self, w: &mut SqlWriter, elements: &[SqlExpr], t: &TypeFact) -> Result<(), Refusal> {
        let Some(element) = fact_list_element(t) else {
            return Err(Refusal::capability(format!(
                "a list literal of {} reached Postgres: a list of unknown element type",
                type_fact_text(t)
            )));
        };
        w.append("CAST(ARRAY[").join(elements, ", ", |w, e| encode(self.ex(e, 0), Some(element))(w))?;
        let carrier = self.carrier(element)?;
        w.append(&format!("] AS {carrier}[])"));
        Ok(())
    }

    /// DuckDB's list_reduce(xs, (acc, x) -> body): a correlated recursive walk typed as the reduction.
    fn list_reduce(&self, w: &mut SqlWriter, a: &[SqlExpr], t: &TypeFact) -> Result<(), Refusal> {
        let xs = arg(a, 0)?;
        let element = element_of(xs, SqlFn::ListReduce.as_str())?;
        let acc = fact_type(t).unwrap_or(element);
        let scalar_like = |t: &SqlType| matches!(t, SqlType::Scalar(_) | SqlType::Decimal { .. });
        if !(scalar_like(acc) || matches!(acc, SqlType::Array { element } if held_as_itself(element))) {
            return Err(Refusal::capability(format!(
                "a list reduction into a {} reached Postgres",
                sql_type_text(acc)
            )));
        }
        let l = lambda_arg(arg(a, 1)?)?;
        let ty = cast_type_name(self, acc)?;
        let acc_name = self.param_at(&l.params, 0)?;
        let x = self.param_at(&l.params, 1)?;
        let at1 = self.element_at(xs, "1", element);
        let next = self.element_at(xs, "__r.__i + 1", element);
        w.append("(WITH RECURSIVE __r(__i, __acc) AS (SELECT 1, CAST(");
        at1(w)?;
        w.append(&format!(" AS {ty}) UNION ALL SELECT __r.__i + 1, CAST(")).expr(self, &l.body, 0)?;
        w.append(&format!(" AS {ty}) FROM __r CROSS JOIN LATERAL (SELECT "));
        next(w)?;
        w.append(&format!(" AS {x}, __r.__acc AS {acc_name}) AS __b WHERE __r.__i < coalesce(cardinality("));
        w.expr(self, xs, 0)?.append("), 0)) SELECT __acc FROM __r ORDER BY __i DESC LIMIT 1)");
        Ok(())
    }

    /// A temporal operand as a NAIVE timestamp: a DATE implicitly casts to timestamptz in Postgres's function
    /// resolution, which the session zone would then shift; a declared TIMESTAMPTZ stays one.
    fn naive<'a>(&'a self, e: &'a SqlExpr) -> P<'a> {
        if matches!(type_of(e), Some(SqlType::Scalar(Scalar::Timestamp | Scalar::Timestamptz))) {
            return self.ex(e, 0);
        }
        pc(move |w| {
            w.append("CAST(").expr(self, e, 0)?.append(" AS TIMESTAMP)");
            Ok(())
        })
    }

    /// The base's contract: date-grained parts (year/quarter/month/week) deliver a DATE, finer parts a TIMESTAMP.
    fn date_trunc<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let part = date_part(part(arg(a, 0)?)?)?;
        let trunc = pc(move |w| {
            let lit = self.string_lit(part)?;
            w.append("date_trunc(").append(&lit).append(", ");
            self.naive(arg(a, 1)?)(w)?;
            w.append(")");
            Ok(())
        });
        if matches!(part, "year" | "quarter" | "month" | "week") {
            return Ok(pc(move |w| {
                w.append("CAST(");
                trunc(w)?;
                w.append(" AS DATE)");
                Ok(())
            }));
        }
        if matches!(part, "day" | "hour" | "minute" | "second") {
            return Ok(trunc);
        }
        // century/millennium/decade: Postgres counts from year 1 (2001), DuckDB from 2000
        Err(Refusal::capability(format!("date_trunc part '{part}' has no probed Postgres spelling")))
    }

    /// DuckDB's date_diff counts BOUNDARIES crossed, never elapsed time.
    fn date_diff<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let part = date_part(part(arg(a, 0)?)?)?;
        let from = self.arg_ex(a, 1, 0);
        let to = self.arg_ex(a, 2, 0);
        let (f, t) = (from.clone(), to.clone());
        let y = pc(move |w| {
            w.append("(extract(year FROM ");
            t(w)?;
            w.append(") - extract(year FROM ");
            f(w)?;
            w.append("))");
            Ok(())
        });
        let per_year = match part {
            "year" => Some(1),
            "quarter" => Some(4),
            "month" => Some(12),
            _ => None,
        };
        let scale = match part {
            "hour" => Some(" / 3600"),
            "minute" => Some(" / 60"),
            "second" => Some(""),
            "millisecond" => Some(" * 1000"),
            "microsecond" => Some(" * 1000000"),
            _ => None,
        };
        let body: P<'a> = if let Some(per_year) = per_year {
            // year/quarter/month: whole years in the unit, plus the in-year part's step
            if per_year == 1 {
                y
            } else {
                pc(move |w| {
                    y(w)?;
                    w.append(&format!(" * {per_year} + (extract({part} FROM "));
                    to(w)?;
                    w.append(&format!(") - extract({part} FROM "));
                    from(w)?;
                    w.append("))");
                    Ok(())
                })
            }
        } else if let Some(scale) = scale {
            epoch_boundaries(from, to, scale)
        } else if part == "day" {
            pc(move |w| {
                w.append("CAST(");
                to(w)?;
                w.append(" AS DATE) - CAST(");
                from(w)?;
                w.append(" AS DATE)");
                Ok(())
            })
        } else {
            // week is DuckDB's plain day count / 7, not a boundary count: unprobed beyond that, so it walls
            return Err(Refusal::capability(format!("date_diff part '{part}' has no probed Postgres spelling")));
        };
        Ok(pc(move |w| {
            w.append("CAST(");
            body(w)?;
            w.append(" AS BIGINT)");
            Ok(())
        }))
    }

    /// The MOD, REM arm: integers and decimals take the base's MOD forms; a Float's remainder is exact.
    fn mod_arm(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.iter().any(is_double) {
            if f == SqlFn::Mod {
                return Err(Refusal::capability(
                    "MOD over a Float reached Postgres, which has no double-precision floored remainder",
                ));
            }
            return self.exact_remainder(w, a);
        }
        if f == SqlFn::Mod { ansi::mod_floored(self, w, a) } else { ansi::mod_truncated(self, w, a) }
    }

    /// Pure's `rem` of two Floats, exact as Java's `%` (Java's `exactRemainder`): each operand an integer times a
    /// power of two, the numeric remainder of the two integers, scaled back.
    fn exact_remainder(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.iter().any(draws_afresh) {
            return Err(Refusal::capability(
                "REM over a Float of random() or a uuid reached Postgres: its exact remainder writes each operand \
                 where it reads it, which would draw it again",
            ));
        }
        let (a0, a1) = (arg(a, 0)?, arg(a, 1)?);
        let x = self.as_double(a0);
        let y = self.as_double(a1);
        let kx = exponent(x.clone());
        let ky = exponent(y.clone());
        // (the common power never below zero: a larger one scales the integers up, exact in NUMERIC)
        let (kx2, ky2) = (kx.clone(), ky.clone());
        let k = pc(move |w| {
            w.append("greatest(");
            kx2(w)?;
            w.append(", ");
            ky2(w)?;
            w.append(", 0)");
            Ok(())
        });
        let scaled_x = scaled(mantissa(x.clone(), kx.clone()), k.clone(), kx);
        let scaled_y = scaled(mantissa(y.clone(), ky.clone()), k.clone(), ky);
        let r = pc(move |w| {
            w.append("CAST(round(mod(");
            scaled_x(w)?;
            w.append(", ");
            scaled_y(w)?;
            w.append("), 1000) / power(CAST(2 AS NUMERIC), ");
            k(w)?;
            w.append(") AS DOUBLE PRECISION)");
            Ok(())
        });
        w.append("(CASE WHEN ");
        x(w)?;
        w.append(" IS NULL OR ");
        y(w)?;
        w.append(" IS NULL THEN CAST(NULL AS DOUBLE PRECISION) WHEN ");
        x(w)?;
        w.append(" = 'NaN' OR ");
        y(w)?;
        w.append(" = 'NaN' OR ");
        y(w)?;
        w.append(" = 0 OR abs(");
        x(w)?;
        w.append(") = 'Infinity' THEN CAST('NaN' AS DOUBLE PRECISION) WHEN abs(");
        y(w)?;
        w.append(") = 'Infinity' OR ");
        x(w)?;
        w.append(" = 0 OR abs(");
        x(w)?;
        w.append(") < abs(");
        y(w)?;
        w.append(") THEN ");
        x(w)?;
        w.append(" ELSE CASE WHEN ");
        x(w)?;
        w.append(" < 0 THEN CAST(-1 AS DOUBLE PRECISION) ELSE CAST(1 AS DOUBLE PRECISION) END * ");
        r(w)?;
        w.append(" END)");
        Ok(())
    }

    /// The ACOS, ASIN arm: the engine's out-of-domain answer is NaN; Postgres raises.
    fn acos(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let name = if f == SqlFn::Acos { "acos" } else { "asin" };
        w.append("(CASE WHEN (").expr(self, arg(a, 0)?, 0)?.append(") BETWEEN -1 AND 1 THEN ").append(name);
        w.append("(").expr(self, arg(a, 0)?, 0)?.append(") ELSE CAST('NaN' AS DOUBLE PRECISION) END)");
        Ok(())
    }

    /// The REGEXP_EXTRACT arm: regexp_extract(s, p[, g]) is '' on a miss.
    fn regexp_extract(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("coalesce(regexp_substr(").expr(self, arg(a, 0)?, 0)?.append(", ");
        self.pattern(arg(a, 1)?, "", "")?(w)?;
        w.append(", 1, 1, '', ");
        if a.len() > 2 {
            w.expr(self, &a[2], 0)?;
        } else {
            w.append("0");
        }
        w.append("), '')");
        Ok(())
    }

    /// The REGEXP_REPLACE arm: regexp_replace(s, p, r[, options]).
    fn regexp_replace(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("regexp_replace(").expr(self, arg(a, 0)?, 0)?.append(", ");
        self.pattern(arg(a, 1)?, "", "")?(w)?;
        for x in tail(a, 2)? {
            w.append(", ").expr(self, x, 0)?;
        }
        w.append(")");
        Ok(())
    }

    /// The STRFTIME arm: to_char over a literal format.
    fn strftime(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if !matches!(arg(a, 1)?, SqlExpr::FormatLit { .. }) {
            return Err(Refusal::capability(
                "a date format that is not a format literal reached Postgres: its codes are DuckDB's",
            ));
        }
        w.append("to_char(");
        self.naive(arg(a, 0)?)(w)?;
        w.append(", ").expr(self, &a[1], 0)?.append(")");
        Ok(())
    }

    /// The EXTRACT arm: DuckDB's date_part is an integer (seconds truncated); extract is numeric.
    fn extract_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let part = date_part(part(arg(a, 0)?)?)?;
        let probed = ["year", "quarter", "month", "week", "day", "doy", "dow", "isodow", "hour", "minute", "second"];
        if !probed.contains(&part) {
            return Err(Refusal::capability(format!("date part '{part}' has no probed Postgres spelling")));
        }
        w.append("CAST(floor(extract(").append(part).append(" FROM ").expr(self, arg(a, 1)?, 0)?;
        w.append(")) AS BIGINT)");
        Ok(())
    }

    /// The JSON_TYPE arm, in DuckDB's json_type vocabulary.
    fn json_type_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("(CASE jsonb_typeof(").expr(self, arg(a, 0)?, 0)?;
        w.append(") WHEN 'null' THEN 'NULL' WHEN 'string' THEN 'VARCHAR'");
        w.append(" WHEN 'boolean' THEN 'BOOLEAN' WHEN 'array' THEN 'ARRAY' WHEN 'object' THEN 'OBJECT'");
        w.append(" WHEN 'number' THEN CASE WHEN (").expr(self, arg(a, 0)?, 0)?;
        w.append(" #>> '{}') ~ '^-?[0-9]+$' THEN 'BIGINT'").append(" ELSE 'DOUBLE' END END)");
        Ok(())
    }

    /// The REGEXP_EXTRACT_ALL arm: group g of every match, the n-th by regexp_substr.
    fn regexp_extract_all(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let pat = self.pattern(arg(a, 1)?, "", "")?;
        w.append("ARRAY(SELECT regexp_substr(").expr(self, arg(a, 0)?, 0)?.append(", ");
        pat(w)?;
        w.append(", 1, __n, '', ");
        if a.len() > 2 {
            w.expr(self, &a[2], 0)?;
        } else {
            w.append("0");
        }
        w.append(") FROM generate_series(1, regexp_count(").expr(self, &a[0], 0)?.append(", ");
        pat(w)?;
        w.append(")) AS __g(__n) ORDER BY __n)");
        Ok(())
    }

    /// DuckDB's printf over a LITERAL format, unrolled (Java's `format`): its text and each directive's argument
    /// concatenated -- `%s` the argument's text, `%d` an integer's digits (padded to a width, `0` with zeros after the
    /// sign), `%.Nf` a number at N decimals (6 by default) rounded half-even on its decimal text.
    fn format<'a>(&'a self, a: &'a [SqlExpr]) -> Result<P<'a>, Refusal> {
        let SqlExpr::StringLit { value: f, .. } = arg(a, 0)? else {
            return Err(Refusal::capability(
                "a computed format reached Postgres, which has no printf: only a literal one is unrolled",
            ));
        };
        let found = directives(f);
        let mut stripped = String::new();
        let mut at = 0;
        for d in &found {
            stripped.push_str(&f[at..d.start]);
            at = d.end;
        }
        stripped.push_str(&f[at..]);
        if stripped.contains('%') {
            return Err(Refusal::capability(format!(
                "a format directive other than %s, %d and %f reached Postgres: {f}"
            )));
        }
        let mut parts: Vec<P<'a>> = Vec::new();
        let mut at = 0;
        let mut next = 1;
        let mut lead = String::new();
        for d in &found {
            lead.push_str(&f[at..d.start]);
            at = d.end;
            if d.percent {
                lead.push('%');
                continue;
            }
            if !lead.is_empty() {
                parts.push(text(self.string_lit(&lead)?));
                lead.clear();
            }
            if next >= a.len() {
                return Err(Refusal::capability("a format with more directives than arguments reached Postgres"));
            }
            let x = &a[next];
            next += 1;
            let width = if d.width.is_empty() { 0 } else { parse_int(d.width)? };
            let value: P<'a> = match d.conversion {
                b's' | b'd' => {
                    if is_text(x) {
                        self.ex(x, 0)
                    } else {
                        self.ex_owned(build::cast(x.clone(), SqlType::Scalar(Scalar::Varchar)), 0)
                    }
                }
                b'f' => {
                    let precision = match d.precision {
                        None => 6,
                        Some(p) => parse_int(p)?,
                    };
                    // Pure rounds the value's own decimal text half-EVEN; numeric rounds half away from zero, so an
                    // exact half of the scaled value steps to its even neighbour
                    let scale = format!("1{}", "0".repeat(precision.max(0) as usize));
                    let rounded = pc(move |w| {
                        w.append(&format!(
                            "(SELECT round(CASE WHEN __hv - floor(__hv) = 0.5 THEN (CASE WHEN mod(floor(__hv), 2) = 0 \
                             THEN floor(__hv) ELSE floor(__hv) + 1 END) ELSE round(__hv) END / {scale}, {precision}) \
                             FROM (SELECT CAST(CAST("
                        ));
                        w.expr(self, x, 0)?.append(&format!(" AS TEXT) AS NUMERIC) * {scale} AS __hv) AS __he)"));
                        Ok(())
                    });
                    pc(move |w| {
                        w.append("(CASE WHEN ").expr(self, x, 0)?.append(" < 0 AND ");
                        rounded(w)?;
                        w.append(" = 0 THEN '-' ELSE '' END || CAST(");
                        rounded(w)?;
                        w.append(" AS VARCHAR))");
                        Ok(())
                    })
                }
                other => {
                    return Err(Refusal::illegal_state(format!(
                        "the directive pattern admits s, d, f: {}",
                        other as char
                    )));
                }
            };
            if width > 0 && d.conversion == b'd' && d.zero {
                let digits = pc(move |w| {
                    w.append("CAST(abs(").expr(self, x, 0)?.append(") AS VARCHAR)");
                    Ok(())
                });
                let (neg, pos) = (padded(digits.clone(), width - 1, '0'), padded(digits, width, '0'));
                parts.push(pc(move |w| {
                    w.append("(CASE WHEN ").expr(self, x, 0)?.append(" < 0 THEN '-' || ");
                    neg(w)?;
                    w.append(" ELSE ");
                    pos(w)?;
                    w.append(" END)");
                    Ok(())
                }));
            } else if width > 0 {
                parts.push(padded(value, width, ' '));
            } else {
                parts.push(value);
            }
        }
        if next != a.len() {
            return Err(Refusal::capability("a format with more arguments than directives reached Postgres"));
        }
        lead.push_str(&f[at..]);
        if !lead.is_empty() || parts.is_empty() {
            parts.push(text(self.string_lit(&lead)?));
        }
        Ok(pc(move |w| {
            w.append("(").join(&parts, " || ", |v, part| part(v))?;
            w.append(")");
            Ok(())
        }))
    }

    /// A pattern in Postgres's own flavour (ARE), wrapped in `open`/`close`: RE2's inline flags become ARE options,
    /// always spelled, at the very front -- RE2's default `p`, MULTILINE `n`, NON_NEWLINE_SENSITIVE `s`, both `w`,
    /// CASE_INSENSITIVE `i`.
    fn pattern<'a>(&'a self, p: &'a SqlExpr, open: &'static str, close: &'static str) -> Result<P<'a>, Refusal> {
        let mut flags = "";
        let mut literal: Option<&str> = match p {
            SqlExpr::StringLit { value, .. } => Some(value),
            _ => None,
        };
        let mut body = p;
        if let Some(lit) = literal
            && let Some((f, end)) = re2_flags(lit)
        {
            flags = f;
            literal = Some(&lit[end..]);
        } else if let SqlExpr::Call { r#fn: SqlFn::Concat, args, .. } = p
            && args.len() == 2
            && let SqlExpr::StringLit { value, .. } = &args[0]
            && let Some((f, end)) = re2_flags(value)
            && end == value.len()
        {
            flags = f;
            body = &args[1];
        }
        let multiline = flags.contains('m');
        let dot_all = flags.contains('s');
        let options = format!(
            "(?{}{})",
            if flags.contains('i') { "i" } else { "" },
            match (multiline, dot_all) {
                (true, true) => "w",
                (true, false) => "n",
                (false, true) => "s",
                (false, false) => "p",
            }
        );
        if let Some(literal) = literal {
            return Ok(text(self.string_lit(&format!("{options}{open}{literal}{close}"))?));
        }
        Ok(pc(move |w| {
            w.append(&format!("('{options}{open}' || ")).expr(self, body, 0)?;
            w.append(&if close.is_empty() { ")".to_owned() } else { format!(" || '{close}')") });
            Ok(())
        }))
    }

    /// The MAX and MIN arms: no max/min over BOOLEAN (bool_or/bool_and) nor over jsonb (the first in jsonb's order).
    fn extreme(
        &self,
        w: &mut SqlWriter,
        name: &str,
        over_boolean: &str,
        direction: &str,
        r: &Reducer,
    ) -> Result<(), Refusal> {
        if r.args.len() == 1 && r.order_by.is_empty() && is_boolean(&r.args[0]) {
            w.append(over_boolean).append("(").expr(self, &r.args[0], 0)?.append(")");
            return Ok(());
        }
        if r.args.len() == 1 && r.order_by.is_empty() && is_json(&r.args[0]) {
            w.append("(array_agg(").expr(self, &r.args[0], 0)?.append(" ORDER BY ").expr(self, &r.args[0], 0)?;
            w.append(direction).append(" NULLS LAST))[1]");
            return Ok(());
        }
        ansi::aggregate(self, w, name, r)
    }

    /// The MEDIAN arm: percentile_cont interpolates numbers only.
    fn median(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        if r.args.len() != 1 || r.distinct || !r.order_by.is_empty() || !is_numeric(&r.args[0]) {
            return Err(Refusal::capability(
                "median of a non-number reached Postgres, whose percentile_cont interpolates numbers only",
            ));
        }
        w.append("percentile_cont(0.5) WITHIN GROUP (ORDER BY ").expr(self, &r.args[0], 0)?.append(")");
        Ok(())
    }

    /// The MODE arm.
    fn mode(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        if r.args.len() != 1 || r.distinct || !r.order_by.is_empty() {
            return Err(Refusal::capability("a distinct or ordered mode reached Postgres"));
        }
        w.append("mode() WITHIN GROUP (ORDER BY ").expr(self, &r.args[0], 0)?.append(")");
        Ok(())
    }

    /// `name(level) WITHIN GROUP (ORDER BY value[ DESC])`.
    fn percentile(&self, w: &mut SqlWriter, name: &str, r: &Reducer) -> Result<(), Refusal> {
        if r.args.len() != 2 || r.distinct || r.order_by.len() > 1 {
            return Err(Refusal::capability(format!("{} of this shape reached Postgres", r.r#fn)));
        }
        let desc = r.order_by.first().is_some_and(|k| !k.ascending);
        w.append(name).append("(").expr(self, &r.args[1], 0)?.append(") WITHIN GROUP (ORDER BY ");
        w.expr(self, &r.args[0], 0)?.append(if desc { " DESC" } else { "" }).append(")");
        Ok(())
    }

    /// The LIST arm: DuckDB's list(x), each value held as a list holds it.
    fn list_of_values(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        if r.args.len() != 1 || r.distinct && !r.order_by.is_empty() {
            return Err(Refusal::capability("a LIST aggregate of this shape reached Postgres"));
        }
        let x = &r.args[0];
        w.append("array_agg(").append(if r.distinct { "DISTINCT " } else { "" });
        encode(self.ex(x, 0), type_of(x))(w)?;
        if !r.order_by.is_empty() {
            w.append(" ORDER BY ").join(&r.order_by, ", ", |w, k| ansi::aggregate_order_key(self, w, k))?;
        }
        w.append(")");
        Ok(())
    }

    /// The value at the first row in the key's `direction`, rows of a NULL key ignored (DuckDB's arg_max/arg_min).
    fn arg_extreme(&self, w: &mut SqlWriter, direction: &str, r: &Reducer) -> Result<(), Refusal> {
        if r.args.len() != 2 || r.distinct || !r.order_by.is_empty() {
            return Err(Refusal::capability(format!("{} of this shape reached Postgres", r.r#fn)));
        }
        w.append("(array_agg(").expr(self, &r.args[0], 0)?.append(" ORDER BY ").expr(self, &r.args[1], 0)?;
        w.append(direction).append(") FILTER (WHERE ").expr(self, &r.args[1], 0)?.append(" IS NOT NULL))[1]");
        Ok(())
    }

    /// A value out of a variant: its text (`->>` strips JSON quoting; `#>> '{}'` for the whole value), then the cast.
    fn variant_get_cast(&self, w: &mut SqlWriter, args: &[SqlExpr], target: &SqlType) -> Result<(), Refusal> {
        let key = arg(args, 1)?;
        let root = matches!(key, SqlExpr::StringLit { value, .. } if value == "$");
        w.append("CAST((").expr(self, arg(args, 0)?, 8)?;
        if root {
            w.append(" #>> '{}'");
        } else if !matches!(key, SqlExpr::IntLit { .. }) && is_numeric(key) {
            // a computed index, BIGINT: ->> takes an int or a text, never a bigint
            w.append(" ->> CAST(").expr(self, key, 0)?.append(" AS INTEGER)");
        } else {
            w.append(" ->> ").expr(self, key, 8)?;
        }
        let name = cast_type_name(self, target)?;
        w.append(") AS ").append(&name).append(")");
        Ok(())
    }

    /// A value to a list of scalars (Java's `listCast`).
    fn list_cast(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        element: &SqlType,
    ) -> Result<(), Refusal> {
        if empty_collection(value) {
            let carrier = self.carrier(element)?;
            w.append(&format!("CAST(NULL AS {carrier}[])"));
            return Ok(());
        }
        // the empty list literal typed: a list of no elements, never NULL
        if matches!(value, SqlExpr::ArrayLit { elements, .. } if elements.is_empty()) {
            let carrier = self.carrier(element)?;
            w.append(&format!("CAST(ARRAY[] AS {carrier}[])"));
            return Ok(());
        }
        if is_json(value) {
            // a Variant array: its elements held as the list holds them
            return self.decode(self.ex(value, 0), target, 0)(w);
        }
        let from = list_element(value);
        if let Some(from) = from
            && held_as_itself(from)
            && held_as_itself(element)
        {
            let carrier = self.carrier(element)?;
            w.append("CAST(").expr(self, value, 0)?.append(&format!(" AS {carrier}[])"));
            return Ok(());
        }
        if let Some(from) = from
            && !held_as_itself(from)
            && !held_as_itself(element)
        {
            // both held as jsonb
            w.expr(self, value, 0)?;
            return Ok(());
        }
        Err(Refusal::capability(format!(
            "a cast of {} to {} reached Postgres",
            type_fact_text(value.r#type()),
            sql_type_text(target)
        )))
    }
}

/// `unnest(list)`, the list held behind a scalar subquery (Postgres's planner estimates the function's rows from its
/// argument's constant sub-expressions; a scalar subquery reaches the estimate as a parameter, which it does not open).
fn unnest(list: P<'_>) -> P<'_> {
    pc(move |w| {
        w.append("unnest((SELECT ");
        list(w)?;
        w.append("))");
        Ok(())
    })
}

/// A value of type `t` as a list holds it: a list or a map (its entries, a jsonb[]) as jsonb, anything else as it is.
fn encode<'a>(value: P<'a>, t: Option<&SqlType>) -> P<'a> {
    if !rebuilt(t) {
        return value;
    }
    pc(move |w| {
        w.append("to_jsonb(");
        value(w)?;
        w.append(")");
        Ok(())
    })
}

/// A lambda body over a struct parameter: a field read off the parameter becomes a field read of the jsonb object,
/// typed as the struct declares it, its fact the field read's (`None`: the body itself).
fn struct_fields(body: &SqlExpr, param: &str, element: &SqlType) -> Result<Option<SqlExpr>, Refusal> {
    if let SqlExpr::Column(c) = body
        && c.table.as_deref() == Some(param)
    {
        let source = build::typed_column(None, param, element.clone(), true, Origin::Derived);
        return Ok(Some(build::struct_get_given(source, c.name.clone(), c.r#type.clone())));
    }
    carrier::map_children(body, &mut |k| struct_fields(k, param, element))
}

/// jsonb's text, compact as Pure and DuckDB print JSON: a string-aware replace drops the space jsonb puts after every
/// ',' and ':' outside a string.
fn compact_json(text: P<'_>) -> P<'_> {
    pc(move |w| {
        w.append("regexp_replace(");
        text(w)?;
        w.append(r#", '("(?:[^"\\]|\\.)*")|([,:]) ', '\1\2', 'g')"#);
        Ok(())
    })
}

/// A double as jsonb, a Float still: a whole finite one as the numeric of its value with one decimal place.
fn double_jsonb(x: P<'_>) -> P<'_> {
    pc(move |w| {
        w.append("(CASE WHEN ");
        x(w)?;
        w.append(" = trunc(");
        x(w)?;
        w.append(") AND abs(");
        x(w)?;
        w.append(") < CAST('Infinity' AS DOUBLE PRECISION) THEN to_jsonb(round(CAST(");
        x(w)?;
        w.append(" AS NUMERIC), 1)) ELSE to_jsonb(");
        x(w)?;
        w.append(") END)");
        Ok(())
    })
}

/// The power of two the exact remainder scales a finite non-zero double by: 60 less its logarithm's floor.
fn exponent(v: P<'_>) -> P<'_> {
    pc(move |w| {
        w.append("CASE WHEN abs(");
        v(w)?;
        w.append(") > 0 AND abs(");
        v(w)?;
        w.append(") < 'Infinity' THEN 60 - CAST(floor(ln(abs(");
        v(w)?;
        w.append(")) / ln(2)) AS INTEGER) END");
        Ok(())
    })
}

/// `|v|` times two to the `k` (two steps, so neither overflows), an integer, as a NUMERIC.
fn mantissa<'a>(v: P<'a>, k: P<'a>) -> P<'a> {
    pc(move |w| {
        w.append("CAST(CAST(abs(");
        v(w)?;
        w.append(") * power(CAST(2 AS DOUBLE PRECISION), div(");
        k(w)?;
        w.append(", 2)) * power(CAST(2 AS DOUBLE PRECISION), ");
        k(w)?;
        w.append(" - div(");
        k(w)?;
        w.append(", 2)) AS BIGINT) AS NUMERIC)");
        Ok(())
    })
}

/// An operand's integer `m` scaled from its own power `kv` to the common one `k`.
fn scaled<'a>(m: P<'a>, k: P<'a>, kv: P<'a>) -> P<'a> {
    pc(move |w| {
        m(w)?;
        w.append(" * power(CAST(2 AS NUMERIC), ");
        k(w)?;
        w.append(" - ");
        kv(w)?;
        w.append(")");
        Ok(())
    })
}

/// Sub-day parts floor the epoch in their unit, before 1970 too.
fn epoch_boundaries<'a>(from: P<'a>, to: P<'a>, scale: &'static str) -> P<'a> {
    pc(move |w| {
        w.append("floor(extract(epoch FROM ");
        to(w)?;
        w.append(&format!("){scale}) - floor(extract(epoch FROM "));
        from(w)?;
        w.append(&format!("){scale})"));
        Ok(())
    })
}

/// `v` padded on the left to `width` with `fill`, never cut (printf never cuts).
fn padded(v: P<'_>, width: i32, fill: char) -> P<'_> {
    pc(move |w| {
        w.append("lpad(");
        v(w)?;
        w.append(&format!(", greatest({width}, length("));
        v(w)?;
        w.append(&format!(")), '{fill}')"));
        Ok(())
    })
}

/// The Levenshtein distance of two texts, by characters: the edit-distance table a row at a time.
fn levenshtein<'a>(d: &'a Postgres, x: &'a SqlExpr, y: &'a SqlExpr) -> P<'a> {
    let more = "__lv.j < length(__lv.b)";
    let cell = "least(__lv.p[__lv.j + 2] + 1, __lv.c[__lv.j + 1] + 1, __lv.p[__lv.j + 1] + CASE WHEN \
                substr(__lv.a, __lv.i, 1) = substr(__lv.b, __lv.j + 1, 1) THEN 0 ELSE 1 END)";
    pc(move |w| {
        w.append(
            "(WITH RECURSIVE __lv(a, b, i, j, p, c) AS (SELECT __s.a, __s.b, 0, 0, CAST(ARRAY[] AS BIGINT[]), \
             CAST(ARRAY[0] AS BIGINT[]) FROM (SELECT ",
        )
        .expr(d, x, 0)?
        .append(" AS a, ")
        .expr(d, y, 0)?;
        w.append(&format!(
            " AS b) AS __s UNION ALL SELECT __lv.a, __lv.b, CASE WHEN {more} THEN __lv.i ELSE __lv.i + 1 END, CASE \
             WHEN {more} THEN __lv.j + 1 ELSE 0 END, CASE WHEN {more} THEN __lv.p ELSE __lv.c END, CASE WHEN {more} \
             THEN __lv.c || CASE WHEN __lv.i = 0 THEN CAST(__lv.j + 1 AS BIGINT) ELSE {cell} END ELSE \
             ARRAY[CAST(__lv.i + 1 AS BIGINT)] END FROM __lv WHERE NOT (__lv.j = length(__lv.b) AND __lv.i = \
             length(__lv.a))) SELECT c[j + 1] FROM __lv WHERE i = length(a) AND j = length(b))"
        ));
        Ok(())
    })
}

/// Jaro-Winkler similarity, exactly DuckDB's: a recursive walk along `t` matching `p`'s characters in their windows,
/// the transpositions counted, the score in DOUBLE PRECISION.
fn jaro_winkler<'a>(d: &'a Postgres, p: &'a SqlExpr, t: &'a SqlExpr) -> P<'a> {
    let m = "CAST(length(__jw.tm) AS DOUBLE PRECISION)";
    let jaro = format!(
        "(({m} / CAST(length(__jw.p) AS DOUBLE PRECISION) + {m} / CAST(length(__jw.t) AS DOUBLE PRECISION) + ({m} - \
         CAST(__x.n / 2 AS DOUBLE PRECISION)) / NULLIF({m}, 0)) / CAST(3 AS DOUBLE PRECISION))"
    );
    let prefix: Vec<String> = (1..=4)
        .map(|k| {
            format!(
                "CASE WHEN {k} <= least(length(__jw.p), length(__jw.t)) AND left(__jw.p, {k}) = left(__jw.t, {k}) \
                 THEN 1 ELSE 0 END"
            )
        })
        .collect();
    let prefix = prefix.join(" + ");
    let at = "__jw.j + 1";
    pc(move |w| {
        w.append(
            "(WITH RECURSIVE __jw(p, t, b, j, f, tm) AS (SELECT __s.p, __s.t, greatest(0, greatest(length(__s.p), \
             length(__s.t)) / 2 - 1), 0, repeat('0', length(__s.p)), CAST('' AS TEXT) FROM (SELECT ",
        )
        .expr(d, p, 0)?
        .append(" AS p, ")
        .expr(d, t, 0)?;
        w.append(&format!(
            " AS t) AS __s UNION ALL SELECT __jw.p, __jw.t, __jw.b, {at}, CASE WHEN __m.i IS NULL THEN __jw.f ELSE \
             overlay(__jw.f placing '1' from __m.i for 1) END, CASE WHEN __m.i IS NULL THEN __jw.tm ELSE __jw.tm || \
             substr(__jw.t, {at}, 1) END FROM __jw CROSS JOIN LATERAL (SELECT min(__g.i) AS i FROM \
             generate_series(greatest(1, {at} - __jw.b), {at} + __jw.b) AS __g(i) WHERE substr(__jw.f, __g.i, 1) = \
             '0' AND substr(__jw.p, __g.i, 1) = substr(__jw.t, {at}, 1)) AS __m WHERE __jw.j < length(__jw.t)) SELECT \
             CASE WHEN length(__jw.tm) = 0 THEN CAST(0 AS DOUBLE PRECISION) WHEN {jaro} > CAST(0.7 AS DOUBLE \
             PRECISION) THEN {jaro} + CAST({prefix} AS DOUBLE PRECISION) * CAST(0.1 AS DOUBLE PRECISION) * (CAST(1 AS \
             DOUBLE PRECISION) - {jaro}) ELSE {jaro} END FROM __jw CROSS JOIN LATERAL (SELECT count(*) AS n FROM \
             (SELECT substr(__jw.p, __g.i, 1) AS ch, row_number() OVER (ORDER BY __g.i) AS k FROM generate_series(1, \
             length(__jw.p)) AS __g(i) WHERE substr(__jw.f, __g.i, 1) = '1') AS __o WHERE __o.ch <> substr(__jw.tm, \
             CAST(__o.k AS INTEGER), 1)) AS __x WHERE __jw.j = length(__jw.t) AND __jw.p IS NOT NULL)"
        ));
        Ok(())
    })
}

// ==================================================================
// The renderer
// ==================================================================

impl AnsiSqlRenderer for Postgres {
    fn lexicon(&self) -> &Lexicon {
        &LEXICON
    }

    fn ansi_state(&self) -> &AnsiState {
        &self.ansi
    }

    fn simple_name(&self) -> &'static str {
        "Postgres"
    }

    /// The engine's H2 text in the flavour DuckDB shares, its column types Postgres's (the 8-byte double is DOUBLE
    /// PRECISION, SMALLINT the smallest integer) and a bare column name in lower case, as Postgres folds a bare name.
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        Ok(raw_sql::translated(
            statement,
            &|types| raw_sql::replace_word(&raw_sql::double_precision(&types), "TINYINT", "SMALLINT"),
            &|bare| bare.to_lowercase(),
        ))
    }

    /// The portable carrier strategies (FULL OUTER native), the star exclusion spelled out, whole-partition ordered
    /// sets correlated, the substring clamp, string counts as INTEGER, the Float aggregates delivered as DOUBLE,
    /// QUALIFY wrapped by outputs, constant keys typed, and the root's numeric types stated.
    fn passes(&self) -> Vec<Box<dyn SqlRewriter>> {
        vec![
            Box::new(crate::carrier::CarrierStrategies::new(CAPS)),
            Box::new(crate::shared_passes::StarExceptToColumns),
            Box::new(crate::shared_passes::WholePartitionOrderedSets),
            Box::new(crate::passes::SubstringClamp),
            Box::new(crate::shared_passes::StringCountsAsInt),
            Box::new(crate::shared_passes::AggregatesDeliverDouble::new(DOUBLE_AGGREGATES)),
            Box::new(crate::shared_passes::QualifyToSubselect::by_outputs()),
            Box::new(crate::passes::ConstantKeysAsExpressions::default()),
            Box::new(crate::shared_passes::RootNumericTypes),
        ]
    }

    /// Postgres also reads a nested value as jsonb: json is cast, an array or a composite converted.
    fn reads_stored(t: &ColumnType) -> bool {
        ansi::reads_as_text(t) || jsonb_read(t) != JsonbRead::None
    }

    fn stored_read(&self, w: &mut SqlWriter, column: &Column, stored: &ColumnType) -> Result<(), Refusal> {
        match jsonb_read(stored) {
            JsonbRead::Cast => {
                w.append("CAST(").expr(self, &SqlExpr::Column(column.clone()), 0)?.append(" AS JSONB)");
                Ok(())
            }
            JsonbRead::Convert => {
                w.append("to_jsonb(").expr(self, &SqlExpr::Column(column.clone()), 0)?.append(")");
                Ok(())
            }
            JsonbRead::None => ansi::stored_read(self, w, column, stored),
        }
    }

    /// Postgres 12+ evaluates a MATERIALIZED CTE once.
    fn cte_as(&self, c: &Cte) -> String {
        if c.materialized { " AS MATERIALIZED (" } else { " AS (" }.to_owned()
    }

    /// Alias-less projections label explicitly from the declared output (Postgres labels an expression `?column?`).
    fn implicit_label(&self, p: &Projection) -> Result<Option<String>, Refusal> {
        match &p.out {
            Some(out) => Ok(Some(self.alias_ident(&out.name))),
            None => ansi::implicit_label(self, p),
        }
    }

    /// A NULL under a LIST slot typed as the list; error() under a slot raised as the slot's type; the empty collection
    /// in a scalar or struct slot typed as the slot.
    fn projection(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        if let (SqlExpr::NullLit { .. }, Some(out)) = (&p.expr, &p.out)
            && let SqlType::Array { element } = &out.r#type
        {
            let carrier = self.carrier(element)?;
            w.append(&format!("CAST(NULL AS {carrier}[])"));
            return self.label(w, p);
        }
        if let Some(out) = &p.out
            && let SqlExpr::Call { r#fn: SqlFn::Error, args, .. } = &p.expr
        {
            self.raise_in(args, Some(&out.r#type))(w)?;
            return self.label(w, p);
        }
        if let Some(out) = &p.out
            && matches!(p.expr.r#type(), TypeFact::Bottom)
            && matches!(out.r#type, SqlType::Scalar(_) | SqlType::Decimal { .. } | SqlType::Struct { .. })
        {
            w.append("CAST(").expr(self, &p.expr, 0)?;
            let ty = if matches!(out.r#type, SqlType::Struct { .. }) {
                self.carrier(&out.r#type)?
            } else {
                cast_type_name(self, &out.r#type)?
            };
            w.append(" AS ").append(&ty).append(")");
            return self.label(w, p);
        }
        ansi::projection(self, w, p)
    }

    /// A name the query itself writes (an alias, a derived label) quoted, so it keeps the case it is written in; a
    /// quote-bearing declaration is already its own spelling.
    fn ident(&self, name: &str) -> String {
        if pre_quoted(name) {
            return name.to_owned();
        }
        self.delimited(name)
    }

    /// A STORE's name as Postgres folds it (decision D8): bare where it may be, delimited where it must be; a name
    /// declared quoted keeps its case.
    fn physical_name(&self, name: &str) -> String {
        if pre_quoted(name) {
            return name.to_owned();
        }
        let folded = name.to_lowercase();
        if bare(&folded) && !self.lexicon().is_reserved(&folded) { folded } else { self.delimited(&folded) }
    }

    /// A DDL column the store declared bare is a [`AnsiSqlRenderer::physical_name`].
    fn ddl_identifier(&self, name: &str, declared_quoted: bool) -> String {
        if declared_quoted { self.delimited(name) } else { self.physical_name(name) }
    }

    /// A stored table, each part a stored name.
    fn table_name(&self, name: &str, quoting: NameQuoting) -> String {
        match name.find('.') {
            Some(dot) if dot > 0 => format!(
                "{}.{}",
                self.stored_name(&name[..dot], quoting.schema),
                self.stored_name(&name[dot + 1..], quoting.table)
            ),
            _ => self.stored_name(name, quoting.table),
        }
    }

    /// A stored column the store declared bare is a physical name; a derived one and one declared quoted keep their
    /// spelling.
    fn column_name(&self, c: &Column) -> Result<String, Refusal> {
        if c.origin == Some(Origin::Physical) {
            return Ok(self.physical_name(&c.name));
        }
        Ok(ansi::column_name(self, c))
    }

    fn row_order_column(&self) -> Result<String, Refusal> {
        Err(Refusal::capability(
            "row order reached Postgres, which has no stable row-order column (ctid moves on update)",
        ))
    }

    fn star_except_keyword(&self) -> Result<String, Refusal> {
        Err(Refusal::capability(
            "SELECT * EXCLUDE reached Postgres, which has no star exclusion; the column list must be expanded upstream",
        ))
    }

    /// A list literal typed, and DuckDB's ordered list aggregate as `array_agg`.
    fn expr(&self, w: &mut SqlWriter, e: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        match e {
            SqlExpr::ArrayLit { elements, r#type } => self.array_literal(w, elements, r#type),
            // DuckDB's list(value ORDER BY key), as the LIST aggregate: NULL over no row, a list value held as jsonb
            SqlExpr::OrderedListAgg { value, order_by, .. } => {
                w.append("array_agg(");
                encode(self.ex(value, 0), type_of(value))(w)?;
                w.append(" ORDER BY ").expr(self, order_by, 0)?.append(")");
                Ok(())
            }
            other => ansi::expr(self, w, other, parent_prec),
        }
    }

    /// A date-time finer than a microsecond is a TIMESTAMP of its first six digits, as its literal is.
    fn hole_type(&self, kind: ValueKind) -> Result<TypeSpelling, Refusal> {
        if kind == ValueKind::DateTimeNanos {
            return Ok(TypeSpelling { name: "TIMESTAMP".to_owned(), digits: Digits::None, fraction: Some(6) });
        }
        ansi::hole_type(kind)
    }

    /// Postgres keeps microseconds and ROUNDS finer digits: finer digits are TRUNCATED, as DuckDB's TIMESTAMP does.
    fn timestamp_lit(&self, iso: &str) -> String {
        // by UTF-16 unit, a digit as Java's Character.isDigit says
        let units: Vec<u16> = iso.encode_utf16().collect();
        if let Some(dot) = units.iter().rposition(|&u| u == u16::from(b'.')) {
            let digits = units[dot + 1..].iter().take_while(|&&u| lite_javacompat::character::is_digit(u)).count();
            let end = dot + 1 + digits;
            if digits > 6 {
                let kept = String::from_utf16_lossy(&units[..dot + 7]);
                let rest = String::from_utf16_lossy(&units[end..]);
                return format!("TIMESTAMP '{kept}{rest}'");
            }
        }
        format!("TIMESTAMP '{iso}'")
    }

    /// Postgres text cannot hold NUL.
    fn string_lit(&self, value: &str) -> Result<String, Refusal> {
        if value.contains('\0') {
            return Err(Refusal::capability("a string holding NUL reached Postgres, whose text type cannot hold it"));
        }
        Ok(ansi::string_lit(value))
    }

    /// A list, a map, a Variant and a value out of a variant each have their own casts; a Float's text keeps its
    /// `.0`; a text literal is cast as a text first.
    fn variant_aware_cast(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        if let SqlType::Array { element } = target {
            return self.list_cast(w, value, target, element);
        }
        // a Variant as a map: its members as entries, in jsonb's own key order
        if matches!(target, SqlType::Map { .. }) && is_json(value) {
            w.append("(CASE WHEN ").expr(self, value, 0)?.append(" IS NULL OR jsonb_typeof(").expr(self, value, 0)?;
            w.append(
                ") = 'null' THEN NULL ELSE ARRAY(SELECT jsonb_build_object('key', to_jsonb(__mk), 'value', __mv) FROM \
                 jsonb_each(",
            )
            .expr(self, value, 0)?;
            w.append(") WITH ORDINALITY AS __mj(__mk, __mv, __mo) ORDER BY __mo) END)");
            return Ok(());
        }
        // a map to another map's key and value types: its entries are jsonb, so it is itself
        if matches!(target, SqlType::Map { .. }) {
            if matches!(value, SqlExpr::NullLit { .. }) {
                w.append("CAST(NULL AS JSONB[])");
                return Ok(());
            }
            let map_call = matches!(
                value,
                SqlExpr::Call {
                    r#fn: SqlFn::MapEmpty | SqlFn::MapFromLists | SqlFn::MapFromEntries | SqlFn::MapConcat,
                    ..
                }
            );
            if !matches!(type_of(value), Some(SqlType::Map { .. })) && !map_call {
                return Err(Refusal::capability(format!(
                    "a cast of {} to {} reached Postgres",
                    type_fact_text(value.r#type()),
                    sql_type_text(target)
                )));
            }
            w.expr(self, value, 0)?;
            return Ok(());
        }
        // a VARIANT is jsonb: text parses into it; any other value converts
        if *target == json_type() {
            if matches!(value, SqlExpr::NullLit { .. }) {
                w.append("CAST(NULL AS JSONB)");
                return Ok(());
            }
            if is_text(value) {
                w.append("CAST(").expr(self, value, 0)?.append(" AS JSONB)");
            } else {
                w.append("to_jsonb(").expr(self, value, 0)?.append(")");
            }
            return Ok(());
        }
        let marker = matches!(target, SqlType::Scalar(Scalar::TemporalText | Scalar::DecimalText));
        if !marker {
            if let SqlExpr::Call { r#fn: SqlFn::VariantGet, args, .. } = value {
                return self.variant_get_cast(w, args, target);
            }
            // a whole value to a scalar reads its text; to TEXT it is its JSON text, as DuckDB's cast
            if is_json(value) && *target != SqlType::Scalar(Scalar::Varchar) {
                w.append("CAST((").expr(self, value, 8)?.append(" #>> '{}') AS ");
                let name = cast_type_name(self, target)?;
                w.append(&name).append(")");
                return Ok(());
            }
            if is_json(value) {
                return compact_json(pc(move |w| {
                    w.append("CAST(").expr(self, value, 0)?.append(" AS VARCHAR)");
                    Ok(())
                }))(w);
            }
        }
        // DuckDB's Float text keeps an integral value's '.0' (41.0, where Postgres writes 41)
        let float = is_double(value) || matches!(value, SqlExpr::FloatLit { .. });
        if *target == SqlType::Scalar(Scalar::Varchar) && float {
            w.append("regexp_replace(");
            ansi::variant_aware_cast(self, w, value, target, conform)?;
            w.append(r", '^(-?[0-9]+)$', '\1.0')");
            return Ok(());
        }
        if matches!(value, SqlExpr::StringLit { .. })
            && !matches!(target, SqlType::Scalar(Scalar::Varchar | Scalar::TemporalText | Scalar::DecimalText))
        {
            return ansi::variant_aware_cast(self, w, &typed_text(value), target, false);
        }
        ansi::variant_aware_cast(self, w, value, target, conform)
    }

    /// VARIANT navigation over jsonb: a key or a 0-based index; the JSON path `$` is the value itself.
    fn variant_get(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let key = arg(args, 1)?;
        if matches!(key, SqlExpr::StringLit { value, .. } if value == "$") {
            w.expr(self, arg(args, 0)?, 0)?;
            return Ok(());
        }
        if !matches!(key, SqlExpr::IntLit { .. }) && is_numeric(key) {
            // a computed index: Postgres's -> takes an int or a text, never a bigint
            w.append("(").expr(self, &args[0], 8)?.append(" -> CAST(").expr(self, key, 0)?.append(" AS INTEGER))");
            return Ok(());
        }
        w.append("(").expr(self, &args[0], 8)?.append(" -> ").expr(self, key, 8)?.append(")");
        Ok(())
    }

    fn variant_construct(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() != 1 {
            return Err(Refusal::capability(format!("toVariant of {} arguments reached Postgres", a.len())));
        }
        let x = &a[0];
        if matches!(x, SqlExpr::StringLit { .. }) {
            // a quoted literal is of no type yet, which to_jsonb refuses
            return self.as_jsonb(self.ex(x, 0), &SqlType::Scalar(Scalar::Varchar))?(w);
        }
        if is_double(x) {
            return double_jsonb(self.ex(x, 0))(w);
        }
        if matches!(list_element(x), Some(SqlType::Scalar(Scalar::Double))) {
            // a list of doubles, each element so (in order; a null list null, an empty one [])
            w.append("(CASE WHEN ").expr(self, x, 0)?.append(" IS NULL THEN NULL ELSE COALESCE((SELECT jsonb_agg(");
            double_jsonb(text("__ve"))(w)?;
            w.append(" ORDER BY __vo) FROM unnest(").expr(self, x, 0)?;
            w.append(") WITH ORDINALITY AS __v(__ve, __vo)), CAST('[]' AS JSONB)) END)");
            return Ok(());
        }
        w.append("to_jsonb(").expr(self, x, 0)?.append(")");
        Ok(())
    }

    fn struct_lit(&self, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
        w.append("jsonb_build_object(").join(fields, ", ", |w, f| {
            let name = self.string_lit(&f.name)?;
            w.append(&name).append(", ").expr(self, &f.value, 0)?;
            Ok(())
        })?;
        w.append(")");
        Ok(())
    }

    /// A field of a jsonb object, read as its type: the read's own, else the one the struct declares for it.
    fn struct_get(&self, w: &mut SqlWriter, source: &SqlExpr, field: &str, r#type: &TypeFact) -> Result<(), Refusal> {
        let mut t = fact_type(r#type).cloned();
        if t.is_none()
            && let Some(SqlType::Struct { fields }) = type_of(source)
        {
            for f in fields {
                if f.name == field {
                    t = Some(f.r#type.clone());
                }
            }
        }
        let Some(t) = t else {
            return Err(Refusal::capability(format!("a field '{field}' of unknown type reached Postgres")));
        };
        let read = pc(move |w| {
            w.append("(").expr(self, source, 8)?;
            let lit = self.string_lit(field)?;
            w.append(" -> ").append(&lit).append(")");
            Ok(())
        });
        self.decode(read, &t, 0)(w)
    }

    /// Lists of scalars are native arrays: each list function over its elements.
    fn list_call(&self, w: &mut SqlWriter, f: SqlFn, args: &[SqlExpr]) -> Result<(), Refusal> {
        let xs0 = arg(args, 0)?;
        let first = element_of(xs0, f.as_str())?.clone();
        let xs = self.ex(xs0, 0);
        let x = self.param("__x")?;
        let xp = text(x.clone());
        match f {
            SqlFn::ListFilter | SqlFn::ListTransform => {
                let l = Postgres::body_over(lambda_arg(arg(args, 1)?)?, &first)?;
                let (p0, p1) = self.lambda_params(&l)?;
                if f == SqlFn::ListFilter {
                    // a kept element is held again as the list holds it
                    let select = encode(text(p0.clone()), Some(&first));
                    let body = self.ex_owned(*l.body, 0);
                    let order = text(p1.clone());
                    return self.over_elements(xs, first, p0, p1, select, Some(body), order)(w);
                }
                Err(Refusal::illegal_state("LIST_TRANSFORM is rendered with its result type (call)"))
            }
            // NULL || xs is xs, as DuckDB's list_concat
            SqlFn::ListConcat => {
                w.append("(").join(args, " || ", |w, e| self.list_of(e, f.as_str())?(w))?;
                w.append(")");
                Ok(())
            }
            // 1-based, negative from the end, out of range NULL (DuckDB's list_extract)
            SqlFn::ListGet => {
                let i = arg(args, 1)?;
                let xs2 = xs.clone();
                let at = pc(move |w| {
                    w.append("(CASE WHEN ").expr(self, i, 0)?.append(" < 0 THEN (");
                    xs2(w)?;
                    w.append(")[cardinality(");
                    xs2(w)?;
                    w.append(") + 1 + ").expr(self, i, 6)?.append("] ELSE (");
                    xs2(w)?;
                    w.append(")[").expr(self, i, 0)?.append("] END)");
                    Ok(())
                });
                // an inner list is held as jsonb: read back as a list
                if rebuilt(Some(&first)) { self.decode(at, &first, 0)(w) } else { at(w) }
            }
            SqlFn::ListPosition => {
                w.append("array_position(");
                xs(w)?;
                w.append(", ").expr(self, arg(args, 1)?, 0)?.append(")");
                Ok(())
            }
            // DuckDB's list_distinct drops NULLs; first occurrence order
            SqlFn::ListDistinct => {
                w.append("(CASE WHEN ");
                xs(w)?;
                w.append(" IS NULL THEN NULL ELSE ARRAY(SELECT ").append(&x).append(" FROM ");
                unnest(xs.clone())(w)?;
                w.append(&format!(" WITH ORDINALITY AS __u({x}, __o) WHERE "));
                w.append(&format!("{x} IS NOT NULL GROUP BY {x} ORDER BY min(__o)) END)"));
                Ok(())
            }
            SqlFn::ListAppend => {
                w.append("array_append(");
                xs(w)?;
                w.append(", ");
                encode(self.ex(arg(args, 1)?, 0), Some(&first))(w)?;
                w.append(")");
                Ok(())
            }
            // reorderings move elements as the list holds them (an inner list stays jsonb)
            SqlFn::ListSort | SqlFn::ListSortDesc => {
                let direction = if f == SqlFn::ListSort { "" } else { " DESC" };
                let order = self.sort_key_of(xp.clone(), &first, direction);
                self.over_elements(xs, held_type(xs0)?, x.clone(), "__o".to_owned(), xp, None, order)(w)
            }
            SqlFn::ListReverse => {
                let order = text("__o DESC");
                self.over_elements(xs, held_type(xs0)?, x.clone(), "__o".to_owned(), xp, None, order)(w)
            }
            SqlFn::ListTail => {
                w.append("(");
                xs(w)?;
                w.append(")[2:]");
                Ok(())
            }
            SqlFn::ListInit => {
                w.append("(");
                xs(w)?;
                w.append(")[:cardinality(");
                xs(w)?;
                w.append(") - 1]");
                Ok(())
            }
            // DuckDB's array_slice: 1-based, both ends inclusive
            SqlFn::ListSlice => {
                w.append("(");
                xs(w)?;
                w.append(")[").expr(self, arg(args, 1)?, 0)?.append(":").expr(self, arg(args, 2)?, 0)?.append("]");
                Ok(())
            }
            SqlFn::ListSum => self.over_reduce(xs, format!("sum({x})"))(w),
            // a Boolean list's extremes: false < true, and Postgres has no max(boolean)
            SqlFn::ListMin => {
                let f = if first == SqlType::Scalar(Scalar::Boolean) { "bool_and(" } else { "min(" };
                self.over_reduce(xs, format!("{f}{x})"))(w)
            }
            SqlFn::ListMax => {
                let f = if first == SqlType::Scalar(Scalar::Boolean) { "bool_or(" } else { "max(" };
                self.over_reduce(xs, format!("{f}{x})"))(w)
            }
            SqlFn::ListAvg => self.over_reduce(xs, format!("CAST(avg({x}) AS DOUBLE PRECISION)"))(w),
            SqlFn::ListMedian => self.over_reduce(xs, format!("percentile_cont(0.5) WITHIN GROUP (ORDER BY {x})"))(w),
            SqlFn::ListMode => self.over_reduce(xs, format!("mode() WITHIN GROUP (ORDER BY {x})"))(w),
            SqlFn::ListBoolAnd => self.over_reduce(xs, format!("bool_and({x})"))(w),
            SqlFn::ListBoolOr => self.over_reduce(xs, format!("bool_or({x})"))(w),
            other => Err(Refusal::illegal_state(format!("not a list call: {other}"))),
        }
    }

    /// Every `SqlFn`, decided here (the dialect program's Phase 1: an exhaustive switch).
    fn call(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact, parent_prec: i32) -> Result<(), Refusal> {
        use SqlFn as F;
        // the empty collection as a list: a reduction, an element or a rearrangement of it is the empty collection
        // itself, NULL, as DuckDB answers over a NULL list
        if empty_call(f, a, t) {
            w.append("NULL");
            return Ok(());
        }
        let plain = |w: &mut SqlWriter, name: &str| -> Result<(), Refusal> {
            w.function(self, name, a)?;
            Ok(())
        };
        let cast_double = |w: &mut SqlWriter, name: &str| -> Result<(), Refusal> {
            w.append(name).append("(CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS DOUBLE PRECISION))");
            Ok(())
        };
        match f {
            // ---- the ANSI operators
            F::And
            | F::Or
            | F::Equal
            | F::NotEqual
            | F::Less
            | F::LessEqual
            | F::Greater
            | F::GreaterEqual
            | F::Plus
            | F::Minus
            | F::Times => ansi::infix(self, w, f, a, parent_prec),
            // ---- the shared spellings
            F::Not => ansi::not_prefix(self, w, a, parent_prec),
            F::Negate => ansi::negation(self, w, a),
            F::IsNull => ansi::is_null_test(self, w, a),
            F::IsNotNull => ansi::is_not_null_test(self, w, a),
            F::In => ansi::in_list(self, w, a, parent_prec),
            F::IsDistinctFrom | F::NullSafeNotEqual => ansi::distinct_from(self, w, a),
            F::NullSafeEqual => ansi::not_distinct_from(self, w, a),
            F::Xor => ansi::xor_as_and_or(self, w, a, parent_prec),
            // NULL-skipping flat concat
            F::Concat => ansi::concat_flat(self, w, a),
            // joinStrings' interleaved concat, never flattened
            F::ConcatJoin => plain(w, "concat"),
            // 64-bit parse
            F::ParseInt => ansi::cast_as(self, w, &typed_text(arg(a, 0)?), "BIGINT"),
            // parseDate(text): the ISO text as a timestamp
            F::ParseDate => ansi::cast_as(self, w, &typed_text(arg(a, 0)?), "TIMESTAMP"),
            F::Pi => {
                w.append("pi()");
                Ok(())
            }
            F::Ceiling => ansi::bigint_of(self, w, "ceil", a),
            F::Floor => ansi::bigint_of(self, w, "floor", a),
            F::Sign => ansi::bigint_of(self, w, "sign", a),
            F::Lpad => ansi::pad_with_space_to_integer(self, w, "lpad", a),
            F::Rpad => ansi::pad_with_space_to_integer(self, w, "rpad", a),
            F::UcFirst => ansi::first_char_cased(self, w, "upper", a, parent_prec),
            F::LcFirst => ansi::first_char_cased(self, w, "lower", a, parent_prec),
            F::Today => {
                w.append("current_date");
                Ok(())
            }
            F::DateTruncDay => ansi::cast_as(self, w, arg(a, 0)?, "DATE"),
            F::BoolToText => ansi::cast_as(self, w, arg(a, 0)?, "VARCHAR"),
            F::FloatCanon => ansi::float_canon_by_text(self, w, a, parent_prec),
            F::FloatPrint => ansi::float_print_by_text(self, w, a, parent_prec),
            F::CurrentUserFn => {
                w.append("current_user");
                Ok(())
            }
            // ---- the hooks: this dialect's own
            F::BitAnd | F::BitOr | F::BitXor | F::BitShiftLeft | F::BitShiftRight => self.bit_op(w, f, a),
            F::Round => self.round_half_even(w, a),
            F::Hash => self.hash_signed(w, a),
            // ---- this database's plain spellings: name(args)
            F::Abs => plain(w, "abs"),
            F::AsciiCode => plain(w, "ascii"),
            F::Atan => plain(w, "atan"),
            F::Atan2 => plain(w, "atan2"),
            F::Cbrt => plain(w, "cbrt"),
            // chr takes a 32-bit code; a computed Integer is a BIGINT here
            F::Chr => {
                w.append("chr(CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS INTEGER))");
                Ok(())
            }
            F::Coalesce => self.coalesce_arm(w, a),
            F::Cos => plain(w, "cos"),
            F::Cosh => plain(w, "cosh"),
            F::Cot => plain(w, "cot"),
            F::Degrees => plain(w, "degrees"),
            F::FloorRaw => plain(w, "floor"),
            F::Greatest => plain(w, "greatest"),
            F::Least => plain(w, "least"),
            F::Left => plain(w, "left"),
            F::Lower => plain(w, "lower"),
            F::Ltrim => plain(w, "ltrim"),
            F::Md5 => plain(w, "md5"),
            F::Radians => plain(w, "radians"),
            F::RepeatStr => plain(w, "repeat"),
            F::Replace => plain(w, "replace"),
            F::Translate => plain(w, "translate"),
            F::ReverseString => plain(w, "reverse"),
            F::Right => plain(w, "right"),
            F::Rtrim => plain(w, "rtrim"),
            F::Sin => plain(w, "sin"),
            F::Sinh => plain(w, "sinh"),
            F::SplitPart => plain(w, "split_part"),
            F::StartsWith => plain(w, "starts_with"),
            F::Strpos => plain(w, "strpos"),
            F::Substring => plain(w, "substr"),
            F::Tan => plain(w, "tan"),
            F::Tanh => plain(w, "tanh"),
            F::Timezone => plain(w, "timezone"),
            // a date plus a time of day is the timestamp at it
            F::DateAtTime => ansi::op(w, parent_prec, &|w| {
                w.expr(self, arg(a, 0)?, 5)?.append(" + ").expr(self, arg(a, 1)?, 6)?;
                Ok(())
            }),
            F::Trim => plain(w, "trim"),
            F::Upper => plain(w, "upper"),
            // a Float, as DuckDB's answers; each of these has a numeric overload on Postgres
            F::Sqrt => cast_double(w, "sqrt"),
            F::Exp => cast_double(w, "exp"),
            F::Ln => cast_double(w, "ln"),
            F::Log10 => cast_double(w, "log10"),
            // ---- arithmetic
            // Pure's pow is a Float, as DuckDB's power answers; Postgres's power over numeric is numeric
            F::Pow => {
                w.append("power(CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS DOUBLE PRECISION), CAST(");
                w.expr(self, arg(a, 1)?, 0)?.append(" AS DOUBLE PRECISION))");
                Ok(())
            }
            // Pure's divide is a Float division: both operands DOUBLE PRECISION
            F::Divide => {
                w.append("(CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS DOUBLE PRECISION) / CAST(");
                w.expr(self, arg(a, 1)?, 0)?.append(" AS DOUBLE PRECISION))");
                Ok(())
            }
            // Postgres has no % or mod() over double precision; integers and decimals take the base's MOD forms
            F::Mod | F::Rem => self.mod_arm(w, f, a),
            // ~x on BIGINT
            F::BitNot => {
                w.append("(~ CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS BIGINT))");
                Ok(())
            }
            // DuckDB's // truncates toward zero; so does div() (numeric)
            F::IntDivide => {
                w.append("CAST(div(").expr(self, arg(a, 0)?, 0)?.append(", ").expr(self, arg(a, 1)?, 0)?;
                w.append(") AS BIGINT)");
                Ok(())
            }
            // Pure's divide-with-scale is HALF_UP
            F::RoundHalfUp => self.round_half_up(w, a),
            // the engine's out-of-domain answer is NaN; Postgres raises
            F::Acos | F::Asin => self.acos(w, f, a),
            // ---- strings
            // the engine coerces a non-text argument
            F::Length => {
                if matches!(arg(a, 0)?, SqlExpr::StringLit { .. }) {
                    w.append("length(").list(self, a)?.append(")");
                } else {
                    w.append("length(CAST(").expr(self, &a[0], 0)?.append(" AS VARCHAR))");
                }
                Ok(())
            }
            F::EndsWith => {
                w.append("(right(").expr(self, arg(a, 0)?, 0)?.append(", length(").expr(self, arg(a, 1)?, 0)?;
                w.append(")) = ").expr(self, &a[1], 0)?.append(")");
                Ok(())
            }
            // MATCHES is the PARTIAL test, Postgres's ~
            F::Matches => {
                w.append("(").expr(self, arg(a, 0)?, 7)?.append(" ~ ");
                self.pattern(arg(a, 1)?, "", "")?(w)?;
                w.append(")");
                Ok(())
            }
            // full match: ~ is partial on Postgres, so the pattern anchors (after its options)
            F::RegexpFullMatch => {
                w.append("(").expr(self, arg(a, 0)?, 7)?.append(" ~ ");
                self.pattern(arg(a, 1)?, "^(?:", ")$")?(w)?;
                w.append(")");
                Ok(())
            }
            F::RegexpExtract => self.regexp_extract(w, a),
            F::RegexpReplace => self.regexp_replace(w, a),
            // Postgres's base64 wraps lines at 76 characters
            F::EncodeBase64 => {
                w.append("replace(encode(convert_to(").expr(self, arg(a, 0)?, 0)?;
                w.append(", 'UTF8'), 'base64'), chr(10), '')");
                Ok(())
            }
            F::DecodeBase64 => {
                w.append("convert_from(decode(").expr(self, arg(a, 0)?, 0)?.append(", 'base64'), 'UTF8')");
                Ok(())
            }
            // sha256 is bytea here; the reference is lowercase hex text
            F::Sha256 => {
                w.append("encode(sha256(convert_to(").expr(self, arg(a, 0)?, 0)?.append(", 'UTF8')), 'hex')");
                Ok(())
            }
            F::Guid => {
                w.append("CAST(gen_random_uuid() AS VARCHAR)");
                Ok(())
            }
            F::Random => {
                w.append("random()");
                Ok(())
            }
            // ---- temporal
            F::Now => {
                w.append("(now() AT TIME ZONE 'UTC')");
                Ok(())
            }
            F::Strftime => self.strftime(w, a),
            F::Dayname => {
                w.append("to_char(");
                self.naive(arg(a, 0)?)(w)?;
                w.append(", 'FMDay')");
                Ok(())
            }
            F::Monthname => {
                w.append("to_char(");
                self.naive(arg(a, 0)?)(w)?;
                w.append(", 'FMMonth')");
                Ok(())
            }
            F::Extract => self.extract_arm(w, a),
            F::DateTrunc => self.date_trunc(a)?(w),
            // Postgres's make_date/make_timestamp take INTEGER, never BIGINT
            F::MakeDate => {
                w.append("make_date(").join(a, ", ", |w, x| {
                    w.append("CAST(").expr(self, x, 0)?.append(" AS INTEGER)");
                    Ok(())
                })?;
                w.append(")");
                Ok(())
            }
            F::MakeTimestamp => self.make_timestamp(w, a),
            // (unitFn, amount, date): amount × a one-unit interval
            F::AddInterval | F::AddIntervalTemporal => ansi::op(w, parent_prec, &|w| {
                w.expr(self, arg(a, 2)?, 5)?.append(" + ").expr(self, arg(a, 1)?, 6)?.append(" * INTERVAL '1 ");
                w.append(interval_unit(part(arg(a, 0)?)?)?).append("'");
                Ok(())
            }),
            F::DateDiff => self.date_diff(a)?(w),
            // date_bin bins fixed-length intervals; months and years count whole calendar months from 1970-01
            F::TimeBucket => self.time_bucket(w, a),
            // DuckDB's epoch(ts) is DOUBLE seconds; epoch_ms truncates toward zero
            F::EpochSeconds => {
                w.append("CAST(extract(epoch FROM ").expr(self, arg(a, 0)?, 0)?.append(") AS DOUBLE PRECISION)");
                Ok(())
            }
            F::EpochNs | F::FromEpochNs => Err(Refusal::capability(
                "a nanosecond epoch: Postgres keeps microseconds (the Cypher island's nanosecond leg is DuckDB's)",
            )),
            F::EpochMs => {
                w.append("CAST(trunc(extract(epoch FROM ").expr(self, arg(a, 0)?, 0)?.append(") * 1000) AS BIGINT)");
                Ok(())
            }
            // epoch arithmetic on a naive timestamp: no session zone involved
            F::FromEpochSeconds => ansi::op(w, parent_prec, &|w| {
                w.append("TIMESTAMP '1970-01-01 00:00:00' + ").expr(self, arg(a, 0)?, 6)?;
                w.append(" * INTERVAL '1 second'");
                Ok(())
            }),
            F::FromEpochMs => ansi::op(w, parent_prec, &|w| {
                w.append("TIMESTAMP '1970-01-01 00:00:00' + CAST(").expr(self, arg(a, 0)?, 0)?;
                w.append(" AS BIGINT) * INTERVAL '1 millisecond'");
                Ok(())
            }),
            // error() outside a CASE branch: the raise, VARCHAR-typed
            F::Error => {
                let unnestable = matches!(fact_type(t), Some(SqlType::Array { .. } | SqlType::Map { .. }));
                self.raise(a, None, unnestable)(w)
            }
            // ---- walls
            F::Strptime => {
                Err(wall(f, "to_timestamp(text, fmt) is lenient and zone-bound; not yet probed against the reference"))
            }
            F::Format => self.format(a)?(w),
            // SHA-1 is pgcrypto's digest()
            F::Sha1 => {
                w.append("encode(digest(convert_to(").expr(self, arg(a, 0)?, 0)?.append(", 'UTF8'), 'sha1'), 'hex')");
                Ok(())
            }
            F::Levenshtein => levenshtein(self, arg(a, 0)?, arg(a, 1)?)(w),
            F::JaroWinkler => jaro_winkler(self, arg(a, 0)?, arg(a, 1)?)(w),
            // ---- variant (jsonb)
            F::ToVariant => self.variant_construct(w, a),
            F::VariantGet => self.variant_get(w, a),
            // in DuckDB's json_type vocabulary, which the lowering compares against
            F::JsonType => self.json_type_arm(w, a),
            F::JsonArrayLength => {
                w.append("jsonb_array_length(").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            F::JsonPretty => {
                w.append("jsonb_pretty(").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            // a Variant array's elements, each a Variant (idempotent, as DuckDB's cast)
            F::VariantElements => {
                let x = arg(a, 0)?;
                if list_element(x).is_some() {
                    w.expr(self, x, 0)?;
                    return Ok(());
                }
                self.decode(self.ex(x, 0), &SqlType::Array { element: Box::new(json_type()) }, 0)(w)
            }
            F::JsonMergePatch => Err(wall(f, "variant over jsonb is leg P4")),
            // ---- lists: a list of scalars is a native array
            F::ListFilter
            | F::ListGet
            | F::ListPosition
            | F::ListDistinct
            | F::ListAppend
            | F::ListSum
            | F::ListMin
            | F::ListMax
            | F::ListAvg
            | F::ListMedian
            | F::ListMode
            | F::ListSort
            | F::ListSortDesc
            | F::ListTail
            | F::ListInit
            | F::ListSlice
            | F::ListBoolAnd
            | F::ListBoolOr
            | F::ListReverse => self.list_call(w, f, a),
            F::ListConcat => self.list_concat(w, a, t, parent_prec),
            // each result held as the RESULT list holds it (its type is the call's)
            F::ListTransform => self.list_transform(w, a, t),
            // the list-making calls whose first argument is no list
            F::RangeFn => self.range(w, a),
            F::RepeatValue => {
                w.append("array_fill(").expr(self, arg(a, 0)?, 0)?.append(", ARRAY[CAST(").expr(self, arg(a, 1)?, 0)?;
                w.append(" AS INTEGER)])");
                Ok(())
            }
            F::ListExists => self.list_exists(w, a),
            F::ListForAll => self.list_for_all(w, a),
            F::Unnest => self.unnest_projection(w, a),
            F::AllDistinct => self.all_distinct(w, a),
            F::ListLength => {
                w.append("cardinality(");
                self.list_of(arg(a, 0)?, f.as_str())?(w)?;
                w.append(")");
                Ok(())
            }
            F::Split => self.split(w, a),
            // DuckDB's regexp_extract_all(s, p[, g]): group g of every match
            F::RegexpExtractAll => self.regexp_extract_all(w, a),
            F::MapFromLists
            | F::MapFromEntries
            | F::MapEmpty
            | F::MapExtract
            | F::MapConcat
            | F::MapKeys
            | F::MapValues => self.maps(w, f, a),
            // a struct with one field set (struct_insert): the jsonb object, the field replaced or added
            F::StructInsert => self.struct_insert_arm(w, a),
            F::PureSplitPart => self.split_part_call(w, a),
            F::ListReduce => self.list_reduce(w, a, t),
            F::ListFlatten | F::ListZip | F::ListProduct | F::Typeof => {
                Err(wall(f, "collections over the jsonb carrier are leg P4"))
            }
        }
    }

    /// Postgres's FUNC_MAX_ARGS: a call takes at most 100 arguments.
    fn max_function_arguments(&self) -> usize {
        100
    }

    /// Pure's splitPart: the non-empty tokens, 1-based, NULL past either end, as an array index is.
    fn split_part_call(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("(array_remove(string_to_array(").expr(self, arg(a, 0)?, 0)?.append(", ").expr(self, arg(a, 1)?, 0)?;
        w.append("), ''))[").expr(self, arg(a, 2)?, 0)?.append("]");
        Ok(())
    }

    /// exists([]) is false.
    fn list_exists(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list_predicate(w, args, "bool_or", "FALSE")
    }

    /// forAll([]) is true.
    fn list_for_all(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list_predicate(w, args, "bool_and", "TRUE")
    }

    /// No duplicates iff the distinct count is the count; an empty or NULL list is distinct.
    fn all_distinct(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let xs = self.list_of(arg(args, 0)?, "isDistinct")?;
        let x = self.param("__x")?;
        w.append(&format!("coalesce((SELECT count(*) = count(DISTINCT {x}) FROM "));
        unnest(xs)(w)?;
        w.append(&format!(" AS __u({x})), TRUE)"));
        Ok(())
    }

    /// DuckDB's list_contains: NULL for a NULL list; found by IS NOT DISTINCT FROM, as array_position.
    fn membership(&self, w: &mut SqlWriter, needle: &SqlExpr, collection: &SqlExpr) -> Result<(), Refusal> {
        let element = element_of(collection, "membership")?;
        w.append("(CASE WHEN ").expr(self, collection, 0)?.append(" IS NULL THEN NULL ELSE array_position(");
        w.expr(self, collection, 0)?.append(", ");
        encode(self.ex(needle, 0), Some(element))(w)?;
        w.append(") IS NOT NULL END)");
        Ok(())
    }

    /// A named aggregate over a list's elements, in list order.
    fn reduce_collection(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        collection: &SqlExpr,
        extras: &[SqlExpr],
    ) -> Result<(), Refusal> {
        let element = element_of(collection, reducer.as_str())?;
        let x = build::typed_column(None, "__x", element.clone(), true, Origin::Derived);
        let mut args = vec![x];
        args.extend(extras.iter().cloned());
        let order = if reducer == AggFn::StringAgg {
            vec![build::sort_key(build::derived(None, "__o"), true, None, None)]
        } else {
            Vec::new()
        };
        let agg = build::reducer(reducer, args, false, order);
        w.append("(SELECT ");
        // avg and the moments over a list are a Float too, cast over their exact result as the grouped ones are
        if DOUBLE_AGGREGATES.contains(&reducer) {
            w.append("CAST(");
            self.reducer(w, &agg)?;
            w.append(" AS DOUBLE PRECISION)");
        } else {
            self.reducer(w, &agg)?;
        }
        w.append(" FROM ");
        unnest(self.ex(collection, 0))(w)?;
        let (xn, on) = (self.param("__x")?, self.param("__o")?);
        w.append(&format!(" WITH ORDINALITY AS __u({xn}, {on}))"));
        Ok(())
    }

    /// A list exploded to rows in the select list: unnest keeps the list's order.
    fn unnest_projection(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let x = arg(args, 0)?;
        if empty_collection(x) {
            // the empty collection exploded: no row, so no value carries its column's type (TEXT)
            w.append("unnest(CAST(NULL AS TEXT[]))");
            return Ok(());
        }
        unnest(self.list_of(x, "UNNEST")?)(w)
    }

    /// fold(xs, {element, accumulator | body}, init): a correlated recursive walk, the accumulator typed as the fold's
    /// result (a scalar, or a list of scalars).
    fn fold_call(&self, w: &mut SqlWriter, fold: &SqlExpr) -> Result<(), Refusal> {
        let SqlExpr::FoldCall { source, lambda, init, r#type, .. } = fold else {
            return Err(Refusal::illegal_state("a fold's call is no fold"));
        };
        let held = |t: &SqlType| {
            matches!(t, SqlType::Scalar(_) | SqlType::Decimal { .. })
                || matches!(t, SqlType::Array { element } if held_as_itself(element))
        };
        let Some(t) = fact_type(r#type).filter(|t| held(t)) else {
            return Err(Refusal::capability(format!(
                "a fold into a {} reached Postgres: a list accumulator of anything but scalars is the jsonb carrier",
                type_fact_text(r#type)
            )));
        };
        let xs = self.list_of(source, "fold")?;
        let ty = cast_type_name(self, t)?;
        let elem = self.param_at(&lambda.params, 0)?;
        let acc = self.param_at(&lambda.params, 1)?;
        w.append("(WITH RECURSIVE __f(__i, __acc) AS (SELECT 0, CAST(").expr(self, init, 0)?;
        w.append(&format!(" AS {ty})")).append(" UNION ALL SELECT __f.__i + 1, CAST(").expr(self, &lambda.body, 0)?;
        w.append(&format!(" AS {ty})")).append(" FROM __f CROSS JOIN LATERAL (SELECT (");
        xs(w)?;
        w.append(&format!(
            ")[__f.__i + 1] AS {elem}, __f.__acc AS {acc}) AS __b WHERE __f.__i < coalesce(cardinality("
        ));
        xs(w)?;
        w.append("), 0))").append(" SELECT __acc FROM __f ORDER BY __i DESC LIMIT 1)");
        Ok(())
    }

    /// The base CASE, with a branch that IS error() raised in the CASE's own type.
    fn case_expr(
        &self,
        w: &mut SqlWriter,
        whens: &[When],
        otherwise: Option<&SqlExpr>,
        r#type: &TypeFact,
    ) -> Result<(), Refusal> {
        let slot = fact_type(r#type);
        w.append("CASE");
        for wh in whens {
            w.append(" WHEN ").expr(self, &wh.condition, 0)?.append(" THEN ");
            self.branch(&wh.then, slot)(w)?;
        }
        if let Some(o) = otherwise {
            w.append(" ELSE ");
            self.branch(o, slot)(w)?;
        }
        w.append(" END");
        Ok(())
    }

    /// Pure's hashCode, a signed 64-bit integer: the first 64 bits of the MD5 of the values' text as a JSON array,
    /// read signed through bit(64).
    fn hash_signed(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("CAST(CAST('x' || left(md5(CAST(json_build_array(");
        w.join(a, ", ", |w, x| {
            if is_text(x) || matches!(x, SqlExpr::NullLit { .. }) {
                w.expr(self, x, 0)?;
            } else {
                w.expr(self, &build::cast(x.clone(), SqlType::Scalar(Scalar::Varchar)), 0)?;
            }
            Ok(())
        })?;
        w.append(") AS TEXT)), 16) AS BIT(64)) AS BIGINT)");
        Ok(())
    }

    /// Pure round is half-EVEN: Postgres's round(double precision) is rint; a decimal scaled, its exact half stepped to
    /// the even neighbour.
    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 1 {
            w.append("round(CAST(").expr(self, &a[0], 0)?.append(" AS DOUBLE PRECISION))");
            return Ok(());
        }
        let scale = pc(move |w| {
            w.append("CAST(").expr(self, arg(a, 1)?, 0)?.append(" AS INTEGER)");
            Ok(())
        });
        let x = arg(a, 0)?;
        if is_double(x) {
            // a Float to a scale: scaled, rounded half-even, scaled back, in double precision
            let p = pc(move |w| {
                w.append("power(CAST(10 AS DOUBLE PRECISION), ");
                scale(w)?;
                w.append(")");
                Ok(())
            });
            w.append("(round(CAST(").expr(self, x, 0)?.append(" AS DOUBLE PRECISION) * ");
            p(w)?;
            w.append(") / ");
            p(w)?;
            w.append(")");
            return Ok(());
        }
        // an exact decimal: numeric rounds half AWAY from zero, so an exact .5 steps to its even neighbour
        let p = pc(move |w| {
            w.append("power(CAST(10 AS NUMERIC), ");
            scale(w)?;
            w.append(")");
            Ok(())
        });
        let p2 = p.clone();
        let v = pc(move |w| {
            w.append("(CAST(").expr(self, x, 0)?.append(" AS NUMERIC) * ");
            p2(w)?;
            w.append(")");
            Ok(())
        });
        w.append("(CASE WHEN ");
        v(w)?;
        w.append(" - floor(");
        v(w)?;
        w.append(") = 0.5 THEN (CASE WHEN mod(floor(");
        v(w)?;
        w.append("), 2) = 0 THEN floor(");
        v(w)?;
        w.append(") ELSE floor(");
        v(w)?;
        w.append(") + 1 END) ELSE round(");
        v(w)?;
        w.append(") END / ");
        p(w)?;
        w.append(")");
        Ok(())
    }

    /// BIGINT bit operators: an INTEGER shift is masked to 32 bits, so operands widen first; xor is `#`.
    fn bit_op(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let (op, shift) = match f {
            SqlFn::BitAnd => ("&", false),
            SqlFn::BitOr => ("|", false),
            SqlFn::BitXor => ("#", false),
            SqlFn::BitShiftLeft => ("<<", true),
            SqlFn::BitShiftRight => (">>", true),
            other => return Err(Refusal::illegal_state(format!("not a bit op: {other}"))),
        };
        w.append("(CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS BIGINT) ").append(op).append(" CAST(");
        w.expr(self, arg(a, 1)?, 0)?.append(if shift { " AS INTEGER))" } else { " AS BIGINT))" });
        Ok(())
    }

    /// Every aggregate, decided here.
    fn reducer(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        use AggFn as A;
        match r.r#fn {
            // ANY_VALUE is Postgres 16+; ordered aggregates keep the plain spelling (Postgres's default null placement
            // IS the reference's NULL-largest)
            A::Sum => ansi::aggregate(self, w, "SUM", r),
            A::Count => ansi::aggregate(self, w, "COUNT", r),
            A::Avg => ansi::aggregate(self, w, "AVG", r),
            A::Min => self.extreme(w, "MIN", "bool_and", " ASC", r),
            A::Max => self.extreme(w, "MAX", "bool_or", " DESC", r),
            A::AnyValue => ansi::aggregate(self, w, "ANY_VALUE", r),
            A::StddevSamp => ansi::aggregate(self, w, "STDDEV_SAMP", r),
            A::StddevPop => ansi::aggregate(self, w, "STDDEV_POP", r),
            A::VarSamp => ansi::aggregate(self, w, "VAR_SAMP", r),
            A::VarPop => ansi::aggregate(self, w, "VAR_POP", r),
            A::StringAgg => ansi::aggregate(self, w, "STRING_AGG", r),
            A::Corr => ansi::aggregate(self, w, "CORR", r),
            A::CovarSamp => ansi::aggregate(self, w, "COVAR_SAMP", r),
            A::CovarPop => ansi::aggregate(self, w, "COVAR_POP", r),
            A::BoolAnd => ansi::aggregate(self, w, "BOOL_AND", r),
            A::BoolOr => ansi::aggregate(self, w, "BOOL_OR", r),
            A::Variance => ansi::aggregate(self, w, "VARIANCE", r),
            A::Stddev => ansi::aggregate(self, w, "STDDEV", r),
            // DuckDB's median interpolates numbers; a median of anything else walls
            A::Median => self.median(w, r),
            A::Mode => self.mode(w, r),
            // the reducer's single order key IS the within-group order
            A::QuantileCont => self.percentile(w, "percentile_cont", r),
            A::QuantileDisc => self.percentile(w, "percentile_disc", r),
            // DuckDB's list(x): the values in order, NULLs kept, NULL over no row
            A::List => self.list_of_values(w, r),
            // no arg_max/arg_min: the value at the first row in the key's order, rows of a NULL key ignored
            A::ArgMax => self.arg_extreme(w, " DESC", r),
            A::ArgMin => self.arg_extreme(w, " ASC", r),
            A::Wavg | A::HashList | A::IsDistinctMark | A::UniqueValueOnly => Err(ansi::lowering_marker(r)),
            // a window function is a ranking or value function, never an aggregate
            A::RowNumber
            | A::Rank
            | A::DenseRank
            | A::PercentRank
            | A::CumeDist
            | A::Ntile
            | A::Lag
            | A::Lead
            | A::FirstValue
            | A::LastValue
            | A::NthValue => Err(ansi::window_function(r)),
        }
    }

    /// Ordered-set aggregates (percentile_*, mode) are not window functions here; a windowed avg or moment is numeric
    /// here too, and the pass sees only grouped ones.
    fn window_call(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        if let SqlAgg::Reducer(r) = f {
            if matches!(r.r#fn, AggFn::Median | AggFn::Mode | AggFn::QuantileCont | AggFn::QuantileDisc) {
                return Err(Refusal::capability(format!(
                    "{} over a window reached Postgres, where ordered-set aggregates take no OVER clause",
                    r.r#fn
                )));
            }
            if DOUBLE_AGGREGATES.contains(&r.r#fn) {
                w.append("CAST(");
                ansi::window_call(self, w, f, partition_by, order_by, frame)?;
                w.append(" AS DOUBLE PRECISION)");
                return Ok(());
            }
        }
        ansi::window_call(self, w, f, partition_by, order_by, frame)
    }

    /// Interval frame offsets in the SQL standard's quoted singular form; weeks are 7-day multiples.
    fn bound(&self, b: &Bound) -> Result<String, Refusal> {
        match b {
            Bound::IntervalPreceding { n, unit } => Ok(format!("{} PRECEDING", frame_interval(*n, unit)?)),
            Bound::IntervalFollowing { n, unit } => Ok(format!("{} FOLLOWING", frame_interval(*n, unit)?)),
            other => Ok(ansi::bound(self, other)),
        }
    }

    /// JSON constructors as json (json keeps key order).
    fn json_object(&self, w: &mut SqlWriter, kv: &[SqlExpr]) -> Result<(), Refusal> {
        w.function(self, "json_build_object", kv)?;
        Ok(())
    }

    fn json_array(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.function(self, "json_build_array", elements)?;
        Ok(())
    }

    /// json_agg keeps NULL elements; zero rows aggregate to NULL, so the empty array is coalesced in.
    fn json_array_agg(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        order_keys: &[JsonArrayAggKey],
    ) -> Result<(), Refusal> {
        w.append("coalesce(json_agg(").expr(self, value, 0)?;
        if !order_keys.is_empty() {
            w.append(" ORDER BY ").join(order_keys, ", ", |w, k| {
                w.expr(self, &k.expr, 0)?.append(if k.desc { " DESC" } else { " ASC" }).append(" NULLS LAST");
                Ok(())
            })?;
        }
        w.append("), '[]')");
        Ok(())
    }

    /// to_char codes; literal text is always double-quoted (a bare letter would read as a code).
    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal> {
        let mut out = String::new();
        for p in parts {
            match p {
                DateFmt::Text { s } => {
                    out.push('"');
                    out.push_str(&s.replace('\\', "\\\\").replace('"', "\\\""));
                    out.push('"');
                }
                DateFmt::Part(part) => out.push_str(match part {
                    DateFmtPart::Year4 => "YYYY",
                    DateFmtPart::Month2 => "MM",
                    DateFmtPart::Day2 => "DD",
                    DateFmtPart::Hour2 => "HH24",
                    DateFmtPart::Min2 => "MI",
                    DateFmtPart::Sec2 => "SS",
                    DateFmtPart::SubsecMicro => "US",
                    DateFmtPart::SubsecNano => "US\"000\"",
                    // DuckDB's %g: the milliseconds, three digits
                    DateFmtPart::SubsecMin => "MS",
                    DateFmtPart::MonthAbbrev => "Mon",
                    DateFmtPart::MonthName => "FMMonth",
                    DateFmtPart::WeekdayName => "FMDay",
                    DateFmtPart::Hour12 => "HH12",
                    DateFmtPart::Hour12Nopad => "FMHH12",
                    DateFmtPart::Ampm => "AM",
                }),
            }
        }
        Ok(out)
    }

    /// Every scalar type's name, a refusal where it has none.
    fn scalar_type_name(&self, s: Scalar) -> Result<String, Refusal> {
        Ok(match s {
            Scalar::Boolean => "BOOLEAN",
            Scalar::Integer => "INTEGER",
            Scalar::Bigint => "BIGINT",
            Scalar::Hugeint => "NUMERIC(38)",
            Scalar::Double => "DOUBLE PRECISION",
            Scalar::Varchar | Scalar::TemporalText => "VARCHAR",
            Scalar::Date => "DATE",
            Scalar::Timestamp => "TIMESTAMP",
            Scalar::Timestamptz => "TIMESTAMPTZ",
            Scalar::Json => "JSONB",
            Scalar::Time => "TIME",
            Scalar::DecimalText => {
                return Err(Refusal::illegal_state(format!("{s} cast reached a dialect without {s} support")));
            }
        }
        .to_owned())
    }

    /// No STRUCT type here: a struct cast is refused.
    fn struct_type_name(&self, fields: &[StructField]) -> Result<String, Refusal> {
        let _ = fields;
        Err(Refusal::capability("a STRUCT type reached a dialect without struct support"))
    }

    /// A map is its entries, a jsonb[] of `{"key", "value"}`.
    fn map_type_name(&self, key: &str, value: &str) -> String {
        let _ = (key, value);
        "JSONB[]".to_owned()
    }

    /// Postgres has no TINYINT, BIT or bare DOUBLE; FLOAT is DOUBLE PRECISION as on H2.
    fn ddl_type(&self, t: &ColumnType) -> Result<String, Refusal> {
        Ok(match t {
            ColumnType::Plain { kind } => match kind {
                ColumnKind::Bigint => "BIGINT",
                ColumnKind::Smallint | ColumnKind::Tinyint => "SMALLINT",
                ColumnKind::Integer => "INTEGER",
                ColumnKind::Float | ColumnKind::Double => "DOUBLE PRECISION",
                ColumnKind::Real => "REAL",
                ColumnKind::Bit => "BOOLEAN",
                ColumnKind::Timestamp => "TIMESTAMP",
                ColumnKind::Date => "DATE",
                ColumnKind::Time => "TIME",
                // a nanosecond timestamp column: Postgres's timestamps keep microseconds
                ColumnKind::TimestampNs => {
                    return Err(Refusal::capability("a TIMESTAMP_NS column: Postgres keeps microseconds"));
                }
                ColumnKind::Json => "JSONB",
                ColumnKind::Varchar => "VARCHAR",
                ColumnKind::Other | ColumnKind::Distinct | ColumnKind::Array | ColumnKind::Object => {
                    return Err(Refusal::illegal_state(format!("no DDL spelling for declared column type {kind}")));
                }
            }
            .to_owned(),
            ColumnType::Sized { kind, size } => format!("{kind}({size})"),
            ColumnType::Scaled { kind, precision, scale } => format!("{kind}({precision}, {scale})"),
        })
    }

    /// JSON_TYPE answers DuckDB's names (a CASE over jsonb_typeof).
    fn json_kind_name(&self, kind: JsonKind) -> Result<String, Refusal> {
        Ok(match kind {
            JsonKind::Object => "OBJECT",
            JsonKind::Array => "ARRAY",
            JsonKind::Null => "NULL",
            JsonKind::String => "VARCHAR",
            JsonKind::Integer => "BIGINT",
            JsonKind::UnsignedInteger => "UBIGINT",
            JsonKind::Float => "DOUBLE",
            JsonKind::Boolean => "BOOLEAN",
        }
        .to_owned())
    }

    /// Refused, as TYPEOF is here (its type names are Postgres's).
    fn typeof_name(&self, scalar: Scalar) -> Result<String, Refusal> {
        let _ = scalar;
        Err(Refusal::capability("typeof reached Postgres: its type names are Postgres's"))
    }

    /// Measured on Postgres 16: the standard spellings of the store writes, each statement atomic on an error.
    fn insert_select(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        ansi::standard_insert_select(self, dml)
    }

    fn update(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        ansi::standard_update(self, dml)
    }

    fn delete_where(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        ansi::standard_delete_where(self, dml)
    }

    fn merge(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        ansi::standard_merge(self, dml)
    }

    /// Postgres 16 runs the standard spelling: a temporary table of the session, its columns named, each delimited.
    fn temp_table_as(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        ansi::create_temporary_table_as(self, dml, true, "CREATE TEMPORARY TABLE ", "")
    }
}

impl Postgres {
    /// A projection's label after its value (the label arms of Java's `projection`): its alias, else the implicit
    /// label, if any.
    fn label(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        let label = match &p.alias {
            Some(alias) => Some(self.alias_ident(alias)),
            None => self.implicit_label(p)?,
        };
        if let Some(label) = label {
            w.append(" AS ").append(&label);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

// ==================================================================
// Postgres's `SqlDialect`: the renders as its renderer writes them, and what Postgres.java overrides of the interface
// ==================================================================

impl crate::dialect::SqlDialect for Postgres {
    fn dialect_name(&self) -> &'static str {
        AnsiSqlRenderer::simple_name(self)
    }

    fn render(&self, query: SqlQuery) -> Result<String, Refusal> {
        AnsiSqlRenderer::render(self, query)
    }

    fn render_inline(&self, query: SqlQuery) -> Result<String, Refusal> {
        ansi::render_inline(self, query)
    }

    fn render_statement(&self, query: SqlQuery) -> Result<RenderedStatement, Refusal> {
        AnsiSqlRenderer::render_statement(self, query)
    }

    fn render_ddl(&self, ddl: &SqlDdl) -> Result<String, Refusal> {
        AnsiSqlRenderer::render_ddl(self, ddl)
    }

    fn render_dml(&self, dml: &SqlDml) -> Result<String, Refusal> {
        AnsiSqlRenderer::render_dml(self, dml)
    }

    fn render_dml_statement(&self, dml: &SqlDml) -> Result<RenderedStatement, Refusal> {
        AnsiSqlRenderer::render_dml_statement(self, dml)
    }

    fn raw(&self, statement: &str, written_for: Option<&str>) -> Result<String, Refusal> {
        ansi::raw(self, statement, written_for)
    }

    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        AnsiSqlRenderer::from_engine_text(self, statement)
    }

    fn stored_name(&self, name: &str, declared_quoted: bool) -> String {
        AnsiSqlRenderer::stored_name(self, name, declared_quoted)
    }

    fn physical_name(&self, name: &str) -> String {
        AnsiSqlRenderer::physical_name(self, name)
    }

    fn insert_column_name(&self, name: &str, declared_quoted: bool) -> String {
        AnsiSqlRenderer::insert_column_name(self, name, declared_quoted)
    }

    fn reads_stored(&self, t: &ColumnType) -> bool {
        <Postgres as AnsiSqlRenderer>::reads_stored(t)
    }

    fn table_name_spelled(&self, name: &str, quoting: NameQuoting) -> String {
        AnsiSqlRenderer::table_name(self, name, quoting)
    }

    /// The platform's naive-UTC temporal contract: the session zone pinned.
    fn session_setup(&self) -> Vec<String> {
        vec!["SET TimeZone='UTC'".to_owned()]
    }

    /// Postgres DDL is transactional: an effect segment is one transaction (DuckDB's form).
    fn script(&self, statements: &[String]) -> String {
        format!("BEGIN TRANSACTION;\n{};\nCOMMIT;", statements.join(";\n"))
    }

    fn script_abort(&self) -> Option<String> {
        Some("ROLLBACK".to_owned())
    }

    /// Written beside this dialect ([`PostgresHostOperations`]).
    fn host_operations(&self) -> Box<dyn HostOperations> {
        Box::new(PostgresHostOperations)
    }

    /// The probe plan's facts (2026-10-20): REPEATABLE READ is its snapshot.
    fn keys_snapshot(&self) -> Result<KeysSnapshot, Refusal> {
        Ok(KeysSnapshot::RepeatableRead)
    }

    /// "target lists can have at most 1664 entries" (MaxTupleAttributeNumber).
    fn result_column_limit(&self) -> Result<Option<i32>, Refusal> {
        Ok(Some(1664))
    }

    /// No native PIVOT: static pivots are emulated by the carrier pass.
    fn needs_static_pivot(&self) -> bool {
        true
    }

    /// A Postgres column, from Postgres's OWN catalog ([`CATALOG_RULES`]).
    fn catalog_type(&self, column: &CatalogColumn) -> Result<CatalogType, Refusal> {
        CATALOG_RULES.type_of(column)
    }

    /// A catalog name a bare declaration reaches: one that is its own fold.
    fn bare_name_reaches(&self, catalog_name: &str) -> Result<bool, Refusal> {
        Ok(catalog_name == catalog_name.to_lowercase())
    }

    /// A map is carried as its entries, a jsonb[]: an ARRAY at the wire.
    fn delivers_own(&self, label: &SqlType, wire: &str) -> bool {
        matches!(label, SqlType::Map { .. }) && wire == "ARRAY"
    }
}
