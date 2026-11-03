// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `H2` (`core/src/main/java/com/legend/sql/dialect/H2.java`), the H2 execution dialect, and `H2Modern`
//! (`H2Modern.java`, `extends H2`), its 2.3+ profile: the base renderer plus executable H2 2.1.214 SQL -- native
//! QUALIFY, no LATERAL (the collection carrier rides JSON), no typed-JSON navigation on 2.1 (H2Modern spells it
//! natively), case-sensitive names (a derived label quoted always, a physical name as the DDL spelled it) -- and, as
//! its [`crate::SqlDialect`], its session facts and the server versions it is written for.
//!
//! H2Modern is this struct with `modern` set: Java's subclass overrides ten methods, and each is a branch on it here
//! (`server_versions`, `for_server`, `variant_get`, `struct_lit`, `struct_get`, `call`'s LIST_LENGTH arm,
//! `variant_aware_cast`; its `scalarTypeName`, `formatText` and `reducer` are H2's by delegation). Every other answer,
//! the refusals naming "H2" in their text included, is H2's.

use crate::ansi::{self, part};
use crate::catalog::{self, TypeCode, Types};
use crate::dialect::KeysSnapshot;
use crate::h2_passes::h2_passes;
use crate::java::{self, sql_type_text, type_fact_text};
use crate::refusal::{Refusal, arg};
use crate::renderer::{AnsiSqlRenderer, AnsiState, Lexicon, PlanParam};
use crate::rewriter::SqlRewriter;
use crate::statement::{Digits, RenderedStatement, TypeSpelling, enum_map};
use crate::writer::SqlWriter;
use lite_sql::{
    AggFn, Bound, Column, ColumnKind, ColumnType, DateFmt, DateFmtPart, DatePart, JsonArrayAggKey, JsonKind,
    NameQuoting, NullOrder, Origin, Projection, Reducer, Scalar, SortKey, SqlDdl, SqlDml, SqlExpr, SqlFn, SqlQuery,
    SqlSelect, SqlSource, SqlType, StructField, StructLitField, TypeFact, ValueKind,
};
use std::sync::LazyLock;

/// The H2 dialect, or (with `modern`) H2Modern: one per render, as Java's `new H2()` and `new H2Modern()`.
#[derive(Debug, Default)]
pub struct H2 {
    ansi: AnsiState,
    /// Java's `H2Modern`: the 2.3+ profile, chosen for a server H2's [`SERVERS`] do not name.
    modern: bool,
}

impl H2 {
    /// Java's `new H2()`.
    pub fn new() -> H2 {
        H2::default()
    }

    /// Java's `new H2Modern()`.
    pub fn modern() -> H2 {
        H2 { ansi: AnsiState::default(), modern: true }
    }

    /// Whether this is H2Modern.
    pub fn is_modern(&self) -> bool {
        self.modern
    }
}

/// Java's `H2.CAPS`: no native lists, no correlated explosion (the only probed backend without it), JSON
/// constructors only.
pub const CAPS: crate::carrier::Caps = crate::carrier::Caps {
    native_lists: false,
    correlated_explode: false,
    json_carrier: false,
    native_full_outer: false,
};

/// Java's `H2.SERVERS`: the server versions this dialect writes for, as prefixes of the version a session reports --
/// 2.1 and 2.2, the engine-parity target (the product's H2 is 2.1.214).
pub const SERVERS: &[&str] = &["2.1", "2.2"];

/// H2 2.1.214's reserved words, the engine's own list (h2Extension2_1_214.pure:180-192): Java's `ENGINE_LEXICON`, the
/// golden text channel's set, in its order.
pub const ENGINE_RESERVED: &[&str] = &[
    "all",
    "and",
    "array",
    "as",
    "between",
    "case",
    "check",
    "constraint",
    "cross",
    "current_catalog",
    "current_date",
    "current_schema",
    "current_time",
    "current_timestamp",
    "current_user",
    "distinct",
    "except",
    "exists",
    "false",
    "fetch",
    "for",
    "foreign",
    "from",
    "full",
    "group",
    "having",
    "if",
    "in",
    "inner",
    "intersect",
    "interval",
    "is",
    "join",
    "left",
    "like",
    "limit",
    "localtime",
    "localtimestamp",
    "minus",
    "natural",
    "not",
    "null",
    "offset",
    "on",
    "or",
    "order",
    "primary",
    "qualify",
    "row",
    "rownum",
    "select",
    "table",
    "true",
    "union",
    "unique",
    "unknown",
    "using",
    "values",
    "where",
    "window",
    "with",
    "_rowid_",
    "both",
    "groups",
    "ilike",
    "leading",
    "over",
    "partition",
    "range",
    "regexp",
    "rows",
    "top",
    "trailing",
];

/// Stock H2 2.1.214's execution lexicon (Java's `H2.LEXICON`): the engine's list plus the word the stock parser
/// rejects in alias position that the engine's laxer fork omits (`AS right`).
static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| {
    let mut words = ENGINE_RESERVED.to_vec();
    words.push("right");
    Lexicon::new('"', &words)
});

/// H2's catalog names a column's type by its own name (`CHARACTER VARYING`, `DOUBLE PRECISION`) and keeps the declared
/// one beside it: its JDBC driver reads the code by the declared name where there is one and reports a DECIMAL by that
/// name (Java's `H2.CATALOG_GRID_TYPES`; measured 2026-10-16 against `DatabaseMetaData.getColumns` on H2 2.1.214).
static CATALOG_GRID_TYPES: Types = Types {
    type_name: "CASE WHEN declared_data_type = 'DECIMAL' THEN 'DECIMAL' ELSE data_type END",
    type_key: "COALESCE(declared_data_type, data_type)",
    codes: &[
        ("INTEGER", TypeCode::Integer),
        ("CHARACTER VARYING", TypeCode::Varchar),
        ("BIGINT", TypeCode::Bigint),
        ("SMALLINT", TypeCode::Smallint),
        ("TINYINT", TypeCode::Tinyint),
        ("DOUBLE PRECISION", TypeCode::Double),
        ("FLOAT", TypeCode::Float),
        ("REAL", TypeCode::Real),
        ("DECIMAL", TypeCode::Decimal),
        ("NUMERIC", TypeCode::Numeric),
        ("DATE", TypeCode::Date),
        ("TIMESTAMP", TypeCode::Timestamp),
        ("BOOLEAN", TypeCode::Boolean),
        ("CHARACTER", TypeCode::Char),
        ("TIME", TypeCode::Time),
        ("TIMESTAMP WITH TIME ZONE", TypeCode::TimestampWithTimezone),
        ("BINARY VARYING", TypeCode::Varbinary),
        ("BINARY", TypeCode::Binary),
    ],
};

/// A date part as H2's `extract` and `DATEDIFF` name it (and, lower-cased, its `date_trunc`).
fn date_part(p: DatePart) -> Result<&'static str, Refusal> {
    Ok(match p {
        DatePart::Year => "YEAR",
        DatePart::Quarter => "QUARTER",
        DatePart::Month => "MONTH",
        DatePart::Week => "WEEK",
        DatePart::Day => "DAY",
        DatePart::Hour => "HOUR",
        DatePart::Minute => "MINUTE",
        DatePart::Second => "SECOND",
        DatePart::Millisecond => "MILLISECOND",
        DatePart::Microsecond => "MICROSECOND",
        DatePart::Nanosecond => {
            return Err(Refusal::capability(
                "a nanosecond part has no measured H2 spelling (the Cypher island's nanosecond leg is DuckDB's)",
            ));
        }
        DatePart::DayOfYear => "DOY",
        DatePart::IsoDayOfWeek => "ISODOW",
    })
}

/// A DurationUnit's part as H2's `dateadd` unit keyword (the engine's `mapToDBUnitType`).
fn date_unit(unit: DatePart) -> Result<&'static str, Refusal> {
    Ok(match unit {
        DatePart::Year => "YEAR",
        DatePart::Month => "MONTH",
        DatePart::Week => "WEEK",
        DatePart::Day => "DAY",
        DatePart::Hour => "HOUR",
        DatePart::Minute => "MINUTE",
        DatePart::Second => "SECOND",
        DatePart::Millisecond => "MILLISECOND",
        DatePart::Microsecond => "MICROSECOND",
        DatePart::Quarter | DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
            return Err(Refusal::capability(format!(
                "interval unit '{unit}' reached a dialect without a dateadd unit"
            )));
        }
    })
}

/// H2's window-frame interval: the SQL standard's quoted singular form (`INTERVAL '3' DAY`); weeks are 7-day
/// multiples.
fn h2_interval(n: i64, unit: &str) -> String {
    let u = unit.to_uppercase();
    if u == "WEEKS" || u == "WEEK" {
        return format!("INTERVAL '{}' DAY", n.wrapping_mul(7));
    }
    let singular = u.strip_suffix('S').unwrap_or(&u);
    format!("INTERVAL '{n}' {singular}")
}

/// A value whose node shape is boolean (Java's `booleanShaped`): a literal, EXISTS, membership, a comparison, logic, a
/// null test.
fn boolean_shaped(e: &SqlExpr) -> bool {
    match e {
        SqlExpr::BoolLit { .. } | SqlExpr::Exists { .. } | SqlExpr::Membership { .. } => true,
        SqlExpr::Group { inner, .. } => boolean_shaped(inner),
        SqlExpr::Call { r#fn, .. } => matches!(
            r#fn,
            SqlFn::And
                | SqlFn::Or
                | SqlFn::Not
                | SqlFn::Xor
                | SqlFn::Equal
                | SqlFn::NotEqual
                | SqlFn::Less
                | SqlFn::LessEqual
                | SqlFn::Greater
                | SqlFn::GreaterEqual
                | SqlFn::IsNull
                | SqlFn::IsNotNull
                | SqlFn::In
                | SqlFn::IsDistinctFrom
                | SqlFn::NullSafeEqual
                | SqlFn::NullSafeNotEqual
                | SqlFn::StartsWith
                | SqlFn::EndsWith
        ),
        _ => false,
    }
}

/// `[A-Za-z_][A-Za-z0-9_$]*`, the whole part (Java's `String.matches`).
fn bare_part(part: &str) -> bool {
    let mut bytes = part.bytes();
    match bytes.next() {
        Some(b) if b.is_ascii_alphabetic() || b == b'_' => {
            bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
        }
        _ => false,
    }
}

/// A string's UTF-16 length (Java's `String.length()`).
fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

impl H2 {
    /// A stored name as the DDL spelled it: pre-quoted as it is, else bare unless it is no plain word or a reserved one.
    fn exec_part(&self, part: &str) -> String {
        if part.len() > 1 && part.starts_with('"') && part.ends_with('"') {
            return part.to_owned();
        }
        if !bare_part(part) || self.lexicon().is_reserved(&part.to_lowercase()) {
            return format!("\"{part}\"");
        }
        part.to_owned()
    }

    /// `writer.function(name, args)`: this database's plain spelling of a call.
    fn plain(&self, w: &mut SqlWriter, name: &str, args: &[SqlExpr]) -> Result<(), Refusal> {
        w.function(self, name, args)?;
        Ok(())
    }

    /// `RAWTOHEX(HASH('<algorithm>', x))`: a string's digest in lower-case hex.
    fn digest(&self, w: &mut SqlWriter, algorithm: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("RAWTOHEX(HASH('").append(algorithm).append("', ").expr(self, arg(a, 0)?, 0)?.append("))");
        Ok(())
    }

    /// format(template, args...) over a literal template: H2 has no printf, so the template is a concatenation.
    fn format_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if let Some(SqlExpr::StringLit { value, .. }) = a.first() {
            return self.format_as_concat(w, value, &a[1..]);
        }
        self.plain(w, "printf", a)
    }

    /// bool = text: a parseable literal becomes the boolean it names; anything else falls through.
    fn equal_arm(&self, w: &mut SqlWriter, a: &[SqlExpr], parent_prec: i32) -> Result<(), Refusal> {
        if a.len() == 2 {
            for i in 0..2 {
                if boolean_shaped(&a[i])
                    && let SqlExpr::StringLit { value, .. } = &a[1 - i]
                {
                    let b = match value.to_lowercase().as_str() {
                        "true" | "t" | "1" | "yes" | "y" => Some(true),
                        "false" | "f" | "0" | "no" | "n" => Some(false),
                        _ => None,
                    };
                    if let Some(b) = b {
                        w.append("(").expr(self, &a[i], 4)?.append(" = ").append(if b { "TRUE" } else { "FALSE" });
                        w.append(")");
                        return Ok(());
                    }
                }
            }
        }
        ansi::infix(self, w, SqlFn::Equal, a, parent_prec)
    }

    /// date_diff: `DATEDIFF(UNIT, a, b)`.
    fn date_diff_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("DATEDIFF(").append(date_part(part(arg(a, 0)?)?)?).append(", ").expr(self, arg(a, 1)?, 0)?;
        w.append(", ").expr(self, arg(a, 2)?, 0)?.append(")");
        Ok(())
    }

    /// split_part over a literal one-character separator and a literal positive index: a separator-count guard and
    /// a `REGEXP_REPLACE` with the separator `\Q...\E`-quoted.
    fn split_part_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if let SqlExpr::StringLit { value: sep, .. } = arg(a, 1)?
            && utf16_len(sep) == 1
            && let SqlExpr::IntLit { value: ix, .. } = arg(a, 2)?
            && *ix >= 1
        {
            let sep_lit = self.string_lit(sep)?;
            let q = format!("\\Q{sep}\\E");
            let cls = format!("[^{q}]*");
            let pat = self.string_lit(&format!("^(?:{cls}{q}){{{}}}({cls}).*$", ix - 1))?;
            w.append("CASE WHEN (CHAR_LENGTH(").expr(self, &a[0], 0)?.append(") - CHAR_LENGTH(REPLACE(");
            w.expr(self, &a[0], 0)?.append(", ").append(&sep_lit).append(", ''))) < ").append_long(ix - 1);
            w.append(" THEN '' ELSE REGEXP_REPLACE(").expr(self, &a[0], 0)?.append(", ").append(&pat);
            w.append(", '$1') END");
            return Ok(());
        }
        self.plain(w, "split_part", a)
    }

    /// regexp_extract(s, p[, g]): `COALESCE(REGEXP_SUBSTR(s, p, 1, 1, NULL, g), '')`.
    fn regexp_extract_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 2 || a.len() == 3 {
            w.append("COALESCE(REGEXP_SUBSTR(").expr(self, &a[0], 0)?.append(", ").expr(self, &a[1], 0)?;
            w.append(", 1, 1, NULL, ");
            if a.len() == 3 {
                w.expr(self, &a[2], 0)?;
            } else {
                w.append("0");
            }
            w.append("), '')");
            return Ok(());
        }
        self.plain(w, "regexp_extract", a)
    }

    /// The REGEXP_FULL_MATCH arm: `REGEXP_LIKE` anchored `^(?:p)$`.
    fn regexp_full_match(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("REGEXP_LIKE(").expr(self, arg(a, 0)?, 0)?.append(", ");
        match arg(a, 1)? {
            SqlExpr::StringLit { value, .. } => {
                let lit = self.string_lit(&format!("^(?:{value})$"))?;
                w.append(&lit);
            }
            other => {
                w.append("CONCAT('^(?:', ").expr(self, other, 0)?.append(", ')$')");
            }
        }
        w.append(")");
        Ok(())
    }

    /// The ERROR arm: `SIGNAL('45000', ...)`, the message in the U+001F provenance sentinel.
    fn error_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("SIGNAL('45000', CHAR(31) || ");
        if a.len() > 1 {
            w.expr(self, &a[1], 0)?.append(" || CHAR(30) || ");
        }
        w.append("(").expr(self, arg(a, 0)?, 0)?.append(") || CHAR(31))");
        Ok(())
    }

    /// `format` with a literal template as `||` concatenation, each `%d` / `%s` slot cast to VARCHAR.
    fn format_as_concat(&self, w: &mut SqlWriter, template: &str, args: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("(");
        let mut text = String::new();
        let mut slot = 0;
        let mut first = true;
        let mut chars = template.chars();
        while let Some(ch) = chars.next() {
            if ch != '%' {
                text.push(ch);
                continue;
            }
            let Some(spec) = chars.next() else {
                return Err(Refusal::capability("format template ends in a bare '%' — no H2 spelling"));
            };
            if spec == '%' {
                text.push('%');
                continue;
            }
            if spec != 'd' && spec != 's' {
                return Err(Refusal::capability(format!(
                    "format specifier '%{spec}' reached H2, which has no printf — only %d / %s concatenate"
                )));
            }
            if slot >= args.len() {
                return Err(Refusal::capability("format template has more slots than arguments"));
            }
            if !text.is_empty() {
                w.append(if first { "" } else { " || " }).append("'").append(&text.replace('\'', "''")).append("'");
                first = false;
                text.clear();
            }
            w.append(if first { "" } else { " || " })
                .append("CAST(")
                .expr(self, &args[slot], 0)?
                .append(" AS VARCHAR)");
            slot += 1;
            first = false;
        }
        if !text.is_empty() || first {
            w.append(if first { "" } else { " || " }).append("'").append(&text.replace('\'', "''")).append("'");
        }
        w.append(")");
        Ok(())
    }

    /// An aggregate by its name; an ordered one pins the reference NULL placement too.
    fn h2_aggregate(&self, w: &mut SqlWriter, name: &str, r: &Reducer) -> Result<(), Refusal> {
        if r.order_by.is_empty() {
            return ansi::aggregate(self, w, name, r);
        }
        w.append(name).append("(").append(if r.distinct { "DISTINCT " } else { "" });
        if r.args.is_empty() {
            w.append("*");
        } else {
            w.list(self, &r.args)?;
        }
        w.append(" ORDER BY ").join(&r.order_by, ", ", |w, k| {
            w.expr(self, &k.expr, 0)?.append(if k.ascending { " ASC" } else { " DESC" });
            w.append(match k.null_order {
                None if k.ascending => " NULLS LAST",
                None => " NULLS FIRST",
                Some(NullOrder::NullsFirst) => " NULLS FIRST",
                Some(NullOrder::NullsLast) => " NULLS LAST",
            });
            Ok(())
        })?;
        w.append(")");
        Ok(())
    }

    /// Pure's first() over a group: H2 2.1.214 has no ANY_VALUE, so a comparable scalar takes MIN; a JSON or array
    /// carrier is refused.
    fn any_value(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        let a = arg(&r.args, 0)?;
        if let TypeFact::Typed { r#type: SqlType::Scalar(sc), .. } = a.r#type()
            && *sc != Scalar::Json
        {
            w.append("MIN(").expr(self, a, 0)?.append(")");
            return Ok(());
        }
        let what = match a.r#type() {
            TypeFact::Typed { r#type, .. } => sql_type_text(r#type),
            other => type_fact_text(other),
        };
        Err(Refusal::capability(format!(
            "first() over a group of {what} values has no H2 spelling: this H2 has no ANY_VALUE and MIN needs a \
             comparable scalar"
        )))
    }

    /// `name(level) WITHIN GROUP (ORDER BY value[ DESC])`.
    fn percentile(&self, w: &mut SqlWriter, name: &str, r: &Reducer) -> Result<(), Refusal> {
        let desc = r.order_by.first().is_some_and(|k| !k.ascending);
        w.append(name).append("(").expr(self, arg(&r.args, 1)?, 0)?.append(") WITHIN GROUP (ORDER BY ");
        w.expr(self, arg(&r.args, 0)?, 0)?.append(if desc { " DESC" } else { "" }).append(")");
        Ok(())
    }

    /// A list of the group's values: `JSON_ARRAYAGG(x[ ORDER BY keys] NULL ON NULL)`.
    fn list_of_values(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        w.append("JSON_ARRAYAGG(").expr(self, arg(&r.args, 0)?, 0)?;
        if !r.order_by.is_empty() {
            w.append(" ORDER BY ").join(&r.order_by, ", ", |w, k: &SortKey| {
                w.expr(self, &k.expr, 0)?.append(if k.ascending { " ASC" } else { " DESC" });
                w.append(match k.null_order {
                    None => "",
                    Some(NullOrder::NullsFirst) => " NULLS FIRST",
                    Some(NullOrder::NullsLast) => " NULLS LAST",
                });
                Ok(())
            })?;
        }
        w.append(" NULL ON NULL)");
        Ok(())
    }

    /// A cast TO JSON is the parse intent: `JSON '...'` for a literal, `(x FORMAT JSON)` for dynamic text, a typed
    /// NULL SQL's NULL.
    fn json_parse_cast(&self, w: &mut SqlWriter, value: &SqlExpr) -> Result<(), Refusal> {
        match value {
            SqlExpr::StringLit { value, .. } => {
                let lit = self.string_lit(value)?;
                w.append("JSON ").append(&lit);
            }
            SqlExpr::NullLit { .. } => {
                w.append("CAST(NULL AS JSON)");
            }
            other => {
                w.append("(").expr(self, other, 0)?.append(" FORMAT JSON)");
            }
        }
        Ok(())
    }

    /// translate, where H2's means the same: both literals, `to` at least as long as `from`; refused otherwise.
    fn translate(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if let SqlExpr::StringLit { value: from, .. } = arg(a, 1)?
            && let SqlExpr::StringLit { value: to, .. } = arg(a, 2)?
            && to.chars().count() >= from.chars().count()
        {
            return self.plain(w, "translate", a);
        }
        Err(Refusal::capability(
            "TRANSLATE reached H2 with a to shorter than its from, or either not a literal: H2's TRANSLATE keeps the \
             characters translate removes",
        ))
    }

    /// A parameter's cast type: its type's, a date-time's to the nanosecond.
    fn parameter_type_name(&self, t: &SqlType) -> Result<String, Refusal> {
        if *t == SqlType::Scalar(Scalar::Timestamp) {
            return Ok("TIMESTAMP(9)".to_owned());
        }
        ansi::cast_type_name(self, t)
    }

    /// H2Modern's LIST_LENGTH: `CARDINALITY` counts a JSON array's elements on 2.3+ (an Array-cast wrapper unwrapped).
    fn list_length(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let mut x = &a[0];
        if let SqlExpr::Cast { value, target: SqlType::Array { .. }, .. } = x {
            x = value;
        }
        w.append("CARDINALITY(").expr(self, x, 0)?.append(")");
        Ok(())
    }
}

impl AnsiSqlRenderer for H2 {
    fn lexicon(&self) -> &Lexicon {
        &LEXICON
    }

    fn ansi_state(&self) -> &AnsiState {
        &self.ansi
    }

    fn simple_name(&self) -> &'static str {
        if self.modern { "H2Modern" } else { "H2" }
    }

    /// The engine's H2 text is H2's own.
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        Ok(statement.to_owned())
    }

    /// The carrier strategies, the lateral rewrites (H2 2.1 has no LATERAL), AVG delivered as DOUBLE, and the
    /// references spelled by their sources last.
    fn passes(&self) -> Vec<Box<dyn SqlRewriter>> {
        h2_passes(self.supports_qualify())
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
            Scalar::Json => "JSON",
            // to the nanosecond: H2's TIME keeps no fraction of a second by default
            Scalar::Time => "TIME(9)",
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

    fn supports_qualify(&self) -> bool {
        true
    }

    /// Native QUALIFY.
    fn append_qualify(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        self.nl(w, depth);
        let Some(q) = &s.qualify else {
            return Err(Refusal::null_pointer("appendQualify without a qualify clause"));
        };
        w.append("QUALIFY ").expr(self, q, 0)?;
        Ok(())
    }

    /// A schema-qualified name spells each part as its DDL did: a declared-quoted part delimited, a bare one by the
    /// reference rule.
    fn table_name(&self, name: &str, quoting: NameQuoting) -> String {
        match name.find('.') {
            Some(dot) if dot > 0 => {
                let (schema, table) = (&name[..dot], &name[dot + 1..]);
                let s = if quoting.schema { self.delimited(schema) } else { self.exec_part(schema) };
                let t = if quoting.table { self.delimited(table) } else { self.exec_part(table) };
                format!("{s}.{t}")
            }
            _ => ansi::table_name(self, name, quoting),
        }
    }

    /// EXCEPT names are derived labels of the framed source: the alias rule.
    fn star_except_name(&self, name: &str) -> String {
        self.alias_ident(name)
    }

    /// H2 keeps the SQL standard's `information_schema`.
    fn catalog(&self, w: &mut SqlWriter, catalog: &SqlSource) -> Result<(), Refusal> {
        catalog::write(self, w, catalog, &CATALOG_GRID_TYPES)
    }

    /// The standard spelling; a program's staged rows a local, transactional table dropped on commit.
    fn temp_table_as(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        if matches!(dml, SqlDml::CreateTempTableAs { staged: true, .. }) {
            return ansi::create_temporary_table_as(
                self,
                dml,
                false,
                "CREATE LOCAL TEMPORARY TABLE ",
                " ON COMMIT DROP TRANSACTIONAL",
            );
        }
        ansi::create_temporary_table_as(self, dml, false, "CREATE TEMPORARY TABLE ", "")
    }

    /// Aliases and every derived label quote unconditionally (a pre-quoted one as it is).
    fn alias_ident(&self, name: &str) -> String {
        if name.len() > 1 && name.starts_with('"') && name.ends_with('"') {
            return name.to_owned();
        }
        format!("\"{}\"", name.replace('"', "\"\""))
    }

    /// An alias-less projection labels explicitly from its declared output; stars and output-less projections pass
    /// labels through.
    fn projection(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        match (&p.alias, &p.out) {
            (None, Some(out)) => {
                w.expr(self, &p.expr, 0)?.append(" AS ");
                let label = self.alias_ident(&out.name);
                w.append(&label);
                Ok(())
            }
            _ => ansi::projection(self, w, p),
        }
    }

    /// The origin-driven reference rule: DERIVED spells as its definition, PHYSICAL as the DDL spelled it; an
    /// origin-less reference is refused.
    fn column_name(&self, c: &Column) -> Result<String, Refusal> {
        let Some(origin) = c.origin else {
            return Err(Refusal::capability(format!(
                "origin-less column reference reached the case-sensitive H2 renderer: {}{} — stamp the construction \
                 site (Column.derived/physical or an OutputCol door)",
                c.table.as_ref().map_or(String::new(), |t| format!("{t}.")),
                c.name
            )));
        };
        Ok(match origin {
            Origin::Derived => self.alias_ident(&c.name),
            // declared quoted: delimited, its case kept
            Origin::PhysicalQuoted => self.delimited(&c.name),
            Origin::Physical => self.exec_part(&c.name),
            Origin::PerKey => {
                return Err(Refusal::illegal_state(format!(
                    "a per-key name '{}' spelled without its key's position: the expression writer writes it inside \
                     its per-key region",
                    c.name
                )));
            }
        })
    }

    /// A stored name as this dialect's queries reference a physical column or table part.
    fn physical_name(&self, name: &str) -> String {
        self.exec_part(name)
    }

    /// A store column's declared type as H2 spells it in DDL.
    fn ddl_type(&self, t: &ColumnType) -> Result<String, Refusal> {
        Ok(match t {
            ColumnType::Plain { kind } => match kind {
                ColumnKind::Bigint => "BIGINT",
                ColumnKind::Smallint => "SMALLINT",
                ColumnKind::Tinyint => "TINYINT",
                ColumnKind::Integer => "INTEGER",
                ColumnKind::Float => "FLOAT",
                ColumnKind::Double => "DOUBLE",
                ColumnKind::Real => "REAL",
                ColumnKind::Bit => "BIT",
                ColumnKind::Timestamp => "TIMESTAMP",
                ColumnKind::Date => "DATE",
                // a fraction of a second kept, as the cast's
                ColumnKind::Time => "TIME(9)",
                ColumnKind::TimestampNs => "TIMESTAMP(9)",
                ColumnKind::Json => "JSON",
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

    /// Every placeholder written typed, as H2 types a parameter when it prepares the statement: a value-typed one cast
    /// to a type hole, any other to its type (a date-time to the nanosecond).
    fn placeholder(&self, w: &mut SqlWriter, p: &PlanParam<'_>) -> Result<(), Refusal> {
        if let Some(by_value) = p.by_value {
            let mut types = Vec::new();
            for kind in &by_value.kinds {
                types.push((*kind, self.hole_type(*kind)?));
            }
            let bind = ansi::scalar_bind(p)?;
            w.append("CAST(").bind(bind).append(" AS ").type_hole(enum_map(types), by_value.absent)?.append(")");
            return Ok(());
        }
        let TypeFact::Typed { r#type, .. } = p.r#type else {
            return Err(Refusal::capability(format!(
                "plan parameter '{}' has no type and is not typed by its value, and H2 types a parameter when it \
                 prepares the statement: not bound on H2",
                p.name
            )));
        };
        let bind = ansi::scalar_bind(p)?;
        w.append("CAST(").bind(bind).append(" AS ");
        let t = self.parameter_type_name(r#type)?;
        w.append(&t).append(")");
        Ok(())
    }

    /// The type H2 gives a literal of a value of `kind`, spelled with the value's own digits where it takes them.
    fn hole_type(&self, kind: ValueKind) -> Result<TypeSpelling, Refusal> {
        Ok(match kind {
            ValueKind::Integer => TypeSpelling::new("BIGINT", Digits::None),
            ValueKind::Decimal => TypeSpelling::new("NUMERIC", Digits::PrecisionAndScale),
            ValueKind::Floating => TypeSpelling::new("DECFLOAT", Digits::Precision),
            ValueKind::Date => TypeSpelling::new("DATE", Digits::None),
            ValueKind::DateTime | ValueKind::DateTimeNanos => TypeSpelling::new("TIMESTAMP(9)", Digits::None),
        })
    }

    fn call(
        &self,
        w: &mut SqlWriter,
        f: SqlFn,
        a: &[SqlExpr],
        _type: &TypeFact,
        parent_prec: i32,
    ) -> Result<(), Refusal> {
        use SqlFn as F;
        match f {
            // H2Modern: CARDINALITY counts a JSON array's elements on 2.3+
            F::ListLength if self.modern && a.len() == 1 => self.list_length(w, a),
            F::Format => self.format_arm(w, a),
            F::Equal => self.equal_arm(w, a, parent_prec),
            F::DateDiff => self.date_diff_arm(w, a),
            F::SplitPart => self.split_part_arm(w, a),
            F::RegexpExtract => self.regexp_extract_arm(w, a),
            // ---- this dialect's own spellings
            // integer division: `//` is a line comment on H2; its `/` over integers is integer division
            F::IntDivide => {
                w.append("CAST(").expr(self, arg(a, 0)?, 6)?.append(" / ").expr(self, arg(a, 1)?, 6)?;
                w.append(" AS BIGINT)");
                Ok(())
            }
            // date arithmetic: dateadd(UNIT, n, d)
            F::AddInterval | F::AddIntervalTemporal => {
                w.append("dateadd(").append(date_unit(part(arg(a, 0)?)?)?).append(", ").expr(self, arg(a, 1)?, 0)?;
                w.append(", ").expr(self, arg(a, 2)?, 0)?.append(")");
                Ok(())
            }
            // the SQL standard's extract(UNIT FROM x)
            F::Extract => {
                w.append("extract(").append(date_part(part(arg(a, 0)?)?)?).append(" FROM ").expr(
                    self,
                    arg(a, 1)?,
                    0,
                )?;
                w.append(")");
                Ok(())
            }
            // no starts_with: LEFT/CHAR_LENGTH equality
            F::StartsWith => {
                w.append("(LEFT(").expr(self, arg(a, 0)?, 0)?.append(", CHAR_LENGTH(").expr(self, arg(a, 1)?, 0)?;
                w.append(")) = ").expr(self, arg(a, 1)?, 0)?.append(")");
                Ok(())
            }
            // strpos: LOCATE with its arguments swapped
            F::Strpos => {
                w.append("LOCATE(").expr(self, arg(a, 1)?, 0)?.append(", ").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            // ends_with: the RIGHT/CHAR_LENGTH twin
            F::EndsWith => {
                w.append("(RIGHT(").expr(self, arg(a, 0)?, 0)?.append(", CHAR_LENGTH(").expr(self, arg(a, 1)?, 0)?;
                w.append(")) = ").expr(self, arg(a, 1)?, 0)?.append(")");
                Ok(())
            }
            // boolean text: H2's CAST prints 'TRUE', the reference 'true'
            F::BoolToText => {
                w.append("CASE WHEN ").expr(self, arg(a, 0)?, 0)?.append(" THEN 'true' WHEN NOT ");
                w.expr(self, arg(a, 0)?, 0)?.append(" THEN 'false' END");
                Ok(())
            }
            F::RegexpFullMatch => self.regexp_full_match(w, a),
            F::Error => self.error_arm(w, a),
            // epoch seconds with the fraction, and its millisecond scaling
            F::EpochSeconds => {
                w.append("EXTRACT(EPOCH FROM ").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            F::EpochNs | F::FromEpochNs => Err(Refusal::capability(
                "a nanosecond epoch: H2 keeps microseconds (the Cypher island's nanosecond leg is DuckDB's)",
            )),
            F::EpochMs => {
                w.append("CAST(EXTRACT(EPOCH FROM ").expr(self, arg(a, 0)?, 0)?.append(") * 1000 AS BIGINT)");
                Ok(())
            }
            // ---- the ANSI operators
            F::And
            | F::Or
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
            F::Divide => ansi::divide_as_doubles(self, w, a),
            F::Mod => ansi::mod_floored(self, w, a),
            F::Rem => ansi::mod_truncated(self, w, a),
            F::Negate => ansi::negation(self, w, a),
            F::IsNull => ansi::is_null_test(self, w, a),
            F::IsNotNull => ansi::is_not_null_test(self, w, a),
            F::In => ansi::in_list(self, w, a, parent_prec),
            F::ParseInt => ansi::cast_as(self, w, arg(a, 0)?, "BIGINT"),
            F::Concat => ansi::concat_flat(self, w, a),
            F::ConcatJoin => self.plain(w, "concat", a),
            F::Pi => {
                w.append("pi()");
                Ok(())
            }
            F::Ceiling => ansi::bigint_of(self, w, "ceil", a),
            F::Floor => ansi::bigint_of(self, w, "floor", a),
            F::Sign => ansi::bigint_of(self, w, "sign", a),
            F::Xor => ansi::xor_as_and_or(self, w, a, parent_prec),
            F::Lpad => ansi::pad_with_space(self, w, "lpad", a),
            F::Rpad => ansi::pad_with_space(self, w, "rpad", a),
            F::UcFirst => ansi::first_char_cased(self, w, "upper", a, parent_prec),
            F::LcFirst => ansi::first_char_cased(self, w, "lower", a, parent_prec),
            F::Guid => {
                w.append("CAST(uuid() AS VARCHAR)");
                Ok(())
            }
            F::Random => {
                w.append("RAND()");
                Ok(())
            }
            F::Today => {
                w.append("current_date");
                Ok(())
            }
            F::Now => {
                w.append("now()");
                Ok(())
            }
            F::DateTruncDay => ansi::cast_as(self, w, arg(a, 0)?, "DATE"),
            F::DateTrunc => {
                let spelled = self.string_lit(&date_part(part(arg(a, 0)?)?)?.to_lowercase())?;
                ansi::date_trunc_date_grained(self, w, a, &spelled)
            }
            F::CurrentUserFn => {
                w.append("current_user");
                Ok(())
            }
            F::IsDistinctFrom | F::NullSafeNotEqual => ansi::distinct_from(self, w, a),
            F::NullSafeEqual => ansi::not_distinct_from(self, w, a),
            F::ParseDate => ansi::cast_as(self, w, arg(a, 0)?, "TIMESTAMP"),
            // Pure's divide-with-scale is HALF_UP: plain ROUND
            F::RoundHalfUp => self.plain(w, "ROUND", a),
            F::FloatCanon => ansi::float_canon_by_text(self, w, a, parent_prec),
            F::FloatPrint => ansi::float_print_by_text(self, w, a, parent_prec),
            // ---- the hooks: this dialect's own (or the base's refusal)
            F::ListFilter
            | F::ListTransform
            | F::ListConcat
            | F::ListGet
            | F::ListPosition
            | F::ListZip
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
            | F::RangeFn
            | F::ListProduct
            | F::ListReduce
            | F::ListSlice
            | F::RepeatValue
            | F::ListBoolAnd
            | F::ListBoolOr
            | F::ListReverse
            | F::Typeof => self.list_call(w, f, a),
            F::ListExists => self.list_exists(w, a),
            F::ListForAll => self.list_for_all(w, a),
            F::StructInsert => self.struct_insert(w, a),
            F::Unnest => self.unnest_projection(w, a),
            F::Round => self.round_half_even(w, a),
            F::BitAnd | F::BitOr | F::BitXor | F::BitShiftLeft | F::BitShiftRight => self.bit_op(w, f, a),
            F::PureSplitPart => self.split_part_call(w, a),
            F::Hash => self.hash_signed(w, a),
            F::AllDistinct => self.all_distinct(w, a),
            F::ToVariant => self.variant_construct(w, a),
            F::VariantElements => self.variant_elements(w, a),
            F::VariantGet => self.variant_get(w, a),
            // ---- this database's plain spellings: name(args)
            F::Abs => self.plain(w, "abs", a),
            F::Coalesce => self.plain(w, "coalesce", a),
            F::Length => self.plain(w, "length", a),
            F::Upper => self.plain(w, "upper", a),
            F::Lower => self.plain(w, "lower", a),
            F::Greatest => self.plain(w, "greatest", a),
            F::Least => self.plain(w, "least", a),
            F::ListFlatten => self.plain(w, "flatten", a),
            F::Sqrt => self.plain(w, "sqrt", a),
            F::Cbrt => self.plain(w, "cbrt", a),
            F::Exp => self.plain(w, "exp", a),
            F::Ln => self.plain(w, "ln", a),
            F::Log10 => self.plain(w, "log10", a),
            F::Pow => self.plain(w, "power", a),
            F::Sin => self.plain(w, "sin", a),
            F::Cos => self.plain(w, "cos", a),
            F::Tan => self.plain(w, "tan", a),
            F::Asin => self.plain(w, "asin", a),
            F::Acos => self.plain(w, "acos", a),
            F::Atan => self.plain(w, "atan", a),
            F::Atan2 => self.plain(w, "atan2", a),
            F::Sinh => self.plain(w, "sinh", a),
            F::Cosh => self.plain(w, "cosh", a),
            F::Tanh => self.plain(w, "tanh", a),
            F::RegexpExtractAll => self.plain(w, "regexp_extract_all", a),
            F::RegexpReplace => self.plain(w, "regexp_replace", a),
            F::MapFromLists => self.plain(w, "map", a),
            F::MapFromEntries => self.plain(w, "map_from_entries", a),
            F::MapExtract => self.plain(w, "map_extract", a),
            F::MapKeys => self.plain(w, "map_keys", a),
            F::MapValues => self.plain(w, "map_values", a),
            F::MapConcat => self.plain(w, "map_concat", a),
            F::Substring => self.plain(w, "substr", a),
            F::Matches => self.plain(w, "regexp_like", a),
            F::Left => self.plain(w, "left", a),
            F::Right => self.plain(w, "right", a),
            F::Trim => self.plain(w, "trim", a),
            F::Ltrim => self.plain(w, "ltrim", a),
            F::Rtrim => self.plain(w, "rtrim", a),
            F::Replace => self.plain(w, "replace", a),
            F::Translate => self.translate(w, a),
            F::Split => self.plain(w, "string_split", a),
            F::ReverseString => self.plain(w, "reverse", a),
            F::AsciiCode => self.plain(w, "ascii", a),
            F::Chr => self.plain(w, "chr", a),
            F::Levenshtein => self.plain(w, "levenshtein", a),
            // H2 has no md5/sha1/sha256: its HASH digests, RAWTOHEX spells the digest in lower-case hex
            F::Md5 => self.digest(w, "MD5", a),
            F::Sha1 => self.digest(w, "SHA-1", a),
            F::Sha256 => self.digest(w, "SHA-256", a),
            F::MakeDate => self.plain(w, "make_date", a),
            F::Timezone => self.plain(w, "timezone", a),
            // H2 adds a date and a time to a timestamp
            F::DateAtTime => ansi::op(w, parent_prec, &|w| {
                w.expr(self, arg(a, 0)?, 5)?.append(" + ").expr(self, arg(a, 1)?, 6)?;
                Ok(())
            }),
            F::FromEpochSeconds => self.plain(w, "to_timestamp", a),
            F::Dayname => self.plain(w, "dayname", a),
            F::Monthname => self.plain(w, "monthname", a),
            F::Cot => self.plain(w, "cot", a),
            F::Radians => self.plain(w, "radians", a),
            F::Degrees => self.plain(w, "degrees", a),
            F::RepeatStr => self.plain(w, "repeat", a),
            F::JaroWinkler => self.plain(w, "jaro_winkler_similarity", a),
            F::ListLength => self.plain(w, "len", a),
            F::Strftime => self.plain(w, "formatdatetime", a),
            F::Strptime => self.plain(w, "parsedatetime", a),
            F::FloorRaw => self.plain(w, "floor", a),
            F::JsonType => self.plain(w, "json_type", a),
            F::JsonArrayLength => self.plain(w, "json_array_length", a),
            F::JsonPretty => self.plain(w, "json_pretty", a),
            // refused by name: H2 cannot run DuckDB's spelling
            F::JsonMergePatch => Err(Refusal::capability("a JSON merge reached H2, which has no json_merge_patch()")),
            F::BitNot => Err(Refusal::capability("bitNot reached H2, which has no xor()")),
            F::MapEmpty => Err(Refusal::capability("an empty map reached H2, which has no map type")),
            F::EncodeBase64 => Err(Refusal::capability("encodeBase64 reached H2, which has no to_base64()")),
            F::MakeTimestamp => {
                Err(Refusal::capability("a date-time constructor reached H2, which has no make_timestamp()"))
            }
            F::FromEpochMs => {
                Err(Refusal::capability("a date from epoch milliseconds reached H2, which has no epoch_ms()"))
            }
            F::DecodeBase64 => Err(Refusal::capability("decodeBase64 reached H2, which has no from_base64()")),
            F::TimeBucket => Err(Refusal::capability("a time bucket reached H2, which has no time_bucket()")),
        }
    }

    /// Window-frame interval bounds in the SQL standard's quoted singular form.
    fn bound(&self, b: &Bound) -> Result<String, Refusal> {
        Ok(match b {
            Bound::IntervalPreceding { n, unit } => format!("{} PRECEDING", h2_interval(*n, unit)),
            Bound::IntervalFollowing { n, unit } => format!("{} FOLLOWING", h2_interval(*n, unit)),
            other => ansi::bound(self, other),
        })
    }

    /// Collection values ride the JSON carrier: `JSON_ARRAY(e1, ..., eN NULL ON NULL)`.
    fn array_lit(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        if elements.is_empty() {
            w.append("JSON_ARRAY()");
            return Ok(());
        }
        w.append("JSON_ARRAY(").list(self, elements)?.append(" NULL ON NULL)");
        Ok(())
    }

    /// Scalar to variant JSON: `CAST(x AS JSON)`.
    fn variant_construct(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS JSON)");
        Ok(())
    }

    /// `SELECT * EXCEPT (...)`.
    fn star_except_keyword(&self) -> Result<String, Refusal> {
        Ok("EXCEPT".to_owned())
    }

    /// H2's row-order pseudo-column.
    fn row_order_column(&self) -> Result<String, Refusal> {
        Ok("_ROWID_".to_owned())
    }

    /// FORMATDATETIME's java.time patterns; literal text quoted always.
    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal> {
        let mut out = String::new();
        for p in parts {
            match p {
                DateFmt::Text { s } => {
                    out.push('\'');
                    out.push_str(&s.replace('\'', "''"));
                    out.push('\'');
                }
                DateFmt::Part(part) => out.push_str(match part {
                    DateFmtPart::Year4 => "yyyy",
                    DateFmtPart::Month2 => "MM",
                    DateFmtPart::Day2 => "dd",
                    DateFmtPart::Hour2 => "HH",
                    DateFmtPart::Min2 => "mm",
                    DateFmtPart::Sec2 => "ss",
                    DateFmtPart::SubsecMicro => "SSSSSS",
                    // nine-digit fractional seconds: java.time nanos
                    DateFmtPart::SubsecNano => "nnnnnnnnn",
                    // java.time has no trim-zeros token
                    DateFmtPart::SubsecMin => {
                        return Err(Refusal::capability(
                            "minimal-fraction date format reached a dialect without a trim-zeros token",
                        ));
                    }
                    DateFmtPart::MonthAbbrev => "MMM",
                    DateFmtPart::MonthName => "MMMM",
                    DateFmtPart::WeekdayName => "EEEE",
                    DateFmtPart::Hour12 => "hh",
                    DateFmtPart::Hour12Nopad => "h",
                    DateFmtPart::Ampm => "a",
                }),
            }
        }
        Ok(out)
    }

    /// H2's aggregates: the inverse-distribution percentiles WITHIN GROUP, a list of values on the JSON carrier, an
    /// ordered aggregate's NULL placement explicit.
    fn reducer(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        match r.r#fn {
            AggFn::Sum => self.h2_aggregate(w, "SUM", r),
            AggFn::Count => self.h2_aggregate(w, "COUNT", r),
            AggFn::Avg => self.h2_aggregate(w, "AVG", r),
            AggFn::Min => self.h2_aggregate(w, "MIN", r),
            AggFn::Max => self.h2_aggregate(w, "MAX", r),
            // Pure's first() over a group is ANY non-empty value: a comparable scalar takes MIN
            AggFn::AnyValue => {
                if r.args.len() == 1 && !r.distinct && r.order_by.is_empty() {
                    self.any_value(w, r)
                } else {
                    self.h2_aggregate(w, "ANY_VALUE", r)
                }
            }
            AggFn::StddevSamp => self.h2_aggregate(w, "STDDEV_SAMP", r),
            AggFn::StddevPop => self.h2_aggregate(w, "STDDEV_POP", r),
            AggFn::VarSamp => self.h2_aggregate(w, "VAR_SAMP", r),
            AggFn::VarPop => self.h2_aggregate(w, "VAR_POP", r),
            AggFn::Median => self.h2_aggregate(w, "MEDIAN", r),
            AggFn::Mode => self.h2_aggregate(w, "MODE", r),
            AggFn::StringAgg => self.h2_aggregate(w, "STRING_AGG", r),
            AggFn::List => {
                if r.args.len() == 1 && !r.distinct {
                    self.list_of_values(w, r)
                } else {
                    self.h2_aggregate(w, "LIST", r)
                }
            }
            // the reducer's single order key is the within-group order (a descending percentile); none = ascending
            AggFn::QuantileCont => {
                if within_group_shaped(r) {
                    self.percentile(w, "PERCENTILE_CONT", r)
                } else {
                    self.h2_aggregate(w, "QUANTILE_CONT", r)
                }
            }
            AggFn::QuantileDisc => {
                if within_group_shaped(r) {
                    self.percentile(w, "PERCENTILE_DISC", r)
                } else {
                    self.h2_aggregate(w, "QUANTILE_DISC", r)
                }
            }
            AggFn::Corr => self.h2_aggregate(w, "CORR", r),
            AggFn::CovarSamp => self.h2_aggregate(w, "COVAR_SAMP", r),
            AggFn::CovarPop => self.h2_aggregate(w, "COVAR_POP", r),
            AggFn::ArgMax => self.h2_aggregate(w, "ARG_MAX", r),
            AggFn::ArgMin => self.h2_aggregate(w, "ARG_MIN", r),
            AggFn::BoolAnd => self.h2_aggregate(w, "BOOL_AND", r),
            AggFn::BoolOr => self.h2_aggregate(w, "BOOL_OR", r),
            AggFn::Variance => self.h2_aggregate(w, "VARIANCE", r),
            AggFn::Stddev => self.h2_aggregate(w, "STDDEV", r),
            AggFn::Wavg | AggFn::HashList | AggFn::IsDistinctMark | AggFn::UniqueValueOnly => {
                Err(ansi::lowering_marker(r))
            }
            // a window function is a ranking or value function, never an aggregate (window_call writes it)
            AggFn::RowNumber
            | AggFn::Rank
            | AggFn::DenseRank
            | AggFn::PercentRank
            | AggFn::CumeDist
            | AggFn::Ntile
            | AggFn::Lag
            | AggFn::Lead
            | AggFn::FirstValue
            | AggFn::LastValue
            | AggFn::NthValue => Err(ansi::window_function(r)),
        }
    }

    /// Banker's ROUND: an exact .5 fraction steers to the even integer of the scaled value, anything else is plain
    /// ROUND.
    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let p = |w: &mut SqlWriter| -> Result<(), Refusal> {
            if a.len() == 1 {
                w.append("1");
            } else {
                w.append("POWER(10, ").expr(self, arg(a, 1)?, 0)?.append(")");
            }
            Ok(())
        };
        let scaled = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("(").expr(self, arg(a, 0)?, 6)?.append(" * ").piece(&p)?.append(")");
            Ok(())
        };
        w.append("(CASE WHEN ").piece(&scaled)?.append(" - FLOOR(").piece(&scaled)?.append(") = 0.5");
        w.append(" THEN (CASE WHEN MOD(CAST(FLOOR(").piece(&scaled)?.append(") AS BIGINT), 2) = 0 THEN FLOOR(");
        w.piece(&scaled)?.append(") ELSE FLOOR(").piece(&scaled)?.append(") + 1 END)").append(" ELSE ROUND(");
        w.piece(&scaled)?.append(") END / ").piece(&p)?.append(")");
        Ok(())
    }

    /// The SQL standard's constructors: `JSON_ARRAY(...)`.
    fn json_array(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("JSON_ARRAY(").list(self, elements)?.append(")");
        Ok(())
    }

    /// `JSON_OBJECT('k': v, ...)`: the list alternates key and value (an odd last one is left out).
    fn json_object(&self, w: &mut SqlWriter, kv: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("JSON_OBJECT(");
        let mut i = 0;
        while i + 1 < kv.len() {
            if i > 0 {
                w.append(", ");
            }
            w.expr(self, &kv[i], 0)?.append(": ").expr(self, &kv[i + 1], 0)?;
            i += 2;
        }
        w.push(')');
        Ok(())
    }

    /// `COALESCE(JSON_ARRAYAGG(v ORDER BY ...), JSON '[]')`: the empty array over zero rows.
    fn json_array_agg(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        order_keys: &[JsonArrayAggKey],
    ) -> Result<(), Refusal> {
        w.append("COALESCE(JSON_ARRAYAGG(").expr(self, value, 0)?;
        if !order_keys.is_empty() {
            w.append(" ORDER BY ").join(order_keys, ", ", |w, k| {
                w.expr(self, &k.expr, 0)?.append(if k.desc { " DESC" } else { " ASC" }).append(" NULLS LAST");
                Ok(())
            })?;
        }
        w.append("), JSON '[]')");
        Ok(())
    }

    /// A cast to JSON parses; a boolean cast to text spells the reference's `true`/`false`. H2Modern first: a scalar
    /// cast over a navigation extracts the text and strips its JSON quoting.
    fn variant_aware_cast(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        if self.modern {
            let navigation = matches!(value, SqlExpr::Call { r#fn: SqlFn::VariantGet | SqlFn::VariantElements, .. })
                || matches!(value, SqlExpr::StructGet { .. });
            if navigation && matches!(target, SqlType::Scalar(sc) if *sc != Scalar::Json) {
                w.append("CAST(TRIM(BOTH '\"' FROM CAST(").expr(self, value, 0)?.append(" AS VARCHAR)) AS ");
                let t = ansi::cast_type_name(self, target)?;
                w.append(&t).append(")");
                return Ok(());
            }
        }
        if *target == SqlType::Scalar(Scalar::Json) {
            return self.json_parse_cast(w, value);
        }
        // a value whose type fact is BOOLEAN takes the reference spelling too, not only a boolean-shaped one
        let boolean_fact = matches!(value.r#type(), TypeFact::Typed { r#type: SqlType::Scalar(Scalar::Boolean), .. });
        if *target == SqlType::Scalar(Scalar::Varchar) && (boolean_fact || boolean_shaped(value)) {
            w.append("CASE WHEN ").expr(self, value, 0)?.append(" THEN 'true' WHEN NOT ").expr(self, value, 0)?;
            w.append(" THEN 'false' END");
            return Ok(());
        }
        ansi::variant_aware_cast(self, w, value, target, conform)
    }

    /// Pure's splitPart with the engine's meaning: the part-th maximal run of non-separator characters,
    /// `REGEXP_SUBSTR(s, '[^...]+', 1, part)`, each separator character written `\x{h...}`.
    fn split_part_call(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let sep = match arg(a, 1)? {
            SqlExpr::StringLit { value, .. } if !value.is_empty() => value,
            _ => return Err(Refusal::capability("splitPart on H2 needs a literal, non-empty separator")),
        };
        let mut cls = String::from("[^");
        for c in sep.chars() {
            cls.push_str(&format!("\\x{{{:x}}}", u32::from(c)));
        }
        cls.push_str("]+");
        let lit = self.string_lit(&cls)?;
        w.append("REGEXP_SUBSTR(").expr(self, arg(a, 0)?, 0)?.append(", ").append(&lit).append(", 1, ");
        w.expr(self, arg(a, 2)?, 0)?.append(")");
        Ok(())
    }

    /// Its JSON_TYPE answers DuckDB's names.
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

    /// A DATE is `DATE`; no other type's name is compared.
    fn typeof_name(&self, scalar: Scalar) -> Result<String, Refusal> {
        if scalar == Scalar::Date {
            return Ok("DATE".to_owned());
        }
        Err(Refusal::capability(format!("a TYPEOF compared with {scalar}'s name reached H2")))
    }

    /// H2Modern: `(x)."key"` for a literal key, `(x)[i+1]` for a literal index (one-based); a dynamic key is refused.
    fn variant_get(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        if self.modern {
            match arg(args, 1)? {
                SqlExpr::StringLit { value: key, .. } => {
                    w.append("(")
                        .expr(self, &args[0], 7)?
                        .append(").\"")
                        .append(&key.replace('"', "\"\""))
                        .append("\"");
                    return Ok(());
                }
                SqlExpr::IntLit { value: ix, .. } => {
                    w.append("(").expr(self, &args[0], 7)?.append(")[").append_long(ix.wrapping_add(1)).append("]");
                    return Ok(());
                }
                _ => {}
            }
        }
        ansi::variant_get(self, w, args)
    }

    /// H2Modern: a struct literal is a JSON object (its canonical field order).
    fn struct_lit(&self, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
        if !self.modern {
            return ansi::struct_lit(self, w, fields);
        }
        w.append("JSON_OBJECT(").join(fields, ", ", |w, f| {
            let name = self.string_lit(&f.name)?;
            w.append(&name).append(": ").expr(self, &f.value, 0)?;
            Ok(())
        })?;
        w.append(")");
        Ok(())
    }

    /// H2Modern: a field read is navigation, `(x)."field"`.
    fn struct_get(&self, w: &mut SqlWriter, source: &SqlExpr, field: &str, _type: &TypeFact) -> Result<(), Refusal> {
        if !self.modern {
            return ansi::struct_get(self, w, source, field);
        }
        w.append("(").expr(self, source, 7)?.append(").\"").append(&field.replace('"', "\"\"")).append("\"");
        Ok(())
    }

    /// Measured on H2 2.x: the standard spellings of the store writes.
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
}

#[cfg(test)]
mod tests;

/// A percentile H2 writes WITHIN GROUP: a value and a level, at most one order key.
fn within_group_shaped(r: &Reducer) -> bool {
    r.args.len() == 2 && !r.distinct && r.order_by.len() <= 1
}

// ==================================================================
// H2's `SqlDialect`: the renders as its renderer writes them, and what H2.java and H2Modern.java override of the
// interface
// ==================================================================

impl crate::dialect::SqlDialect for H2 {
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
        <H2 as AnsiSqlRenderer>::reads_stored(t)
    }

    fn table_name_spelled(&self, name: &str, quoting: NameQuoting) -> String {
        AnsiSqlRenderer::table_name(self, name, quoting)
    }

    /// H2 2.3+ has typed-JSON navigation, which H2Modern spells natively; 2.1 and 2.2 (the engine-parity target,
    /// [`SERVERS`]) keep H2's walls. H2Modern is already the 2.3+ profile.
    fn for_server(&self, version: &str) -> Option<Box<dyn crate::dialect::SqlDialect>> {
        if self.modern || SERVERS.iter().any(|s| version.starts_with(s)) {
            return None;
        }
        Some(Box::new(H2::modern()))
    }

    /// [`SERVERS`]; H2Modern refuses: it is chosen for a server at run time, never planned with.
    fn server_versions(&self) -> Result<Option<Vec<String>>, Refusal> {
        if self.modern {
            return Err(Refusal::capability(format!(
                "H2Modern: a plan is written for H2's servers [{}], and H2Modern is chosen for any other at run time",
                SERVERS.join(", ")
            )));
        }
        Ok(Some(SERVERS.iter().map(|s| (*s).to_owned()).collect()))
    }

    /// An embedded engine.
    fn in_process(&self) -> bool {
        true
    }

    /// Its staged table is dropped on commit: a `DROP TABLE` would commit the program's transaction.
    fn drops_staged_tables_with_their_transaction(&self) -> bool {
        true
    }

    /// Measured 2026-10-20 (2.1.214): REPEATABLE READ accepted.
    fn keys_snapshot(&self) -> Result<KeysSnapshot, Refusal> {
        Ok(KeysSnapshot::RepeatableRead)
    }

    /// Measured 2026-10-20 (2.1.214): 16,384 columns.
    fn result_column_limit(&self) -> Result<Option<i32>, Refusal> {
        Ok(Some(16384))
    }

    /// No native dynamic PIVOT: the two-phase staticization pre-pass pins the key values.
    fn needs_static_pivot(&self) -> bool {
        true
    }

    /// H2 names the failing statement of a script (`SQL statement: <text>`; a later failure quotes the script from
    /// that statement on): the index of the statement the quote starts with.
    fn failing_statement(&self, message: &str, statements: &[String]) -> Option<usize> {
        const MARK: &str = "SQL statement: ";
        let at = message.find(MARK)?;
        let quoted = java::strip(&message[at + MARK.len()..]);
        statements.iter().position(|s| quoted.starts_with(java::strip(s)))
    }
}
