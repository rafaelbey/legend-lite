// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `DuckDb` (`core/src/main/java/com/legend/sql/dialect/DuckDb.java`): the base renderer plus DuckDB's own
//! capabilities and idioms -- native QUALIFY, native PIVOT, ASOF joins, list lambdas, bracket array literals, the
//! `->`/`->>` JSON operators. Its catalog decisions, host operations and session facts are not rendering and are not
//! here.

use crate::ansi::{self, part};
use crate::catalog::{self, TypeCode, Types};
use crate::duckdb_spellings as spellings;
use crate::passes::duckdb_passes;
use crate::raw_sql;
use crate::refusal::{Refusal, arg, tail};
use crate::renderer::{AnsiSqlRenderer, AnsiState, Lexicon};
use crate::rewriter::SqlRewriter;
use crate::statement::{Digits, TypeSpelling};
use crate::writer::SqlWriter;
use lite_sql::{
    AggFn, Column, ColumnKind, ColumnType, Cte, DateFmt, DateFmtPart, DatePart, JoinKind, JsonKind, Lambda, Origin,
    Projection, Reducer, Scalar, SqlDml, SqlExpr, SqlFn, SqlSelect, SqlSource, SqlType, StructField, StructLitField,
    TypeFact, ValueKind,
};
use std::borrow::Cow;
use std::sync::LazyLock;

/// The DuckDB dialect: one per dump entry, as Java's `new DuckDb()`.
#[derive(Debug, Default)]
pub struct DuckDb {
    ansi: AnsiState,
    /// Java's `Boolean.getBoolean("legend.exec.engineScanOrder")`, read at each render: the engine corpus runner's
    /// opt-in to `StableScanOrder`.
    pub engine_scan_order: bool,
}

/// Java's `DuckDb.CAPS`: every collection capability.
pub const CAPS: crate::carrier::Caps =
    crate::carrier::Caps { native_lists: true, correlated_explode: true, json_carrier: true, native_full_outer: true };

impl DuckDb {
    pub fn new() -> DuckDb {
        DuckDb::default()
    }
}

/// Every keyword DuckDB will not take as a table or column name (its `reserved` and `type_function` categories of
/// `duckdb_keywords()`), and a few more DML words besides: Java's `DuckDb.LEXICON`.
static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| {
    Lexicon::new(
        '"',
        &[
            "all",
            "analyse",
            "analyze",
            "and",
            "anti",
            "any",
            "array",
            "as",
            "asc",
            "asof",
            "asymmetric",
            "at",
            "authorization",
            "between",
            "binary",
            "both",
            "by",
            "case",
            "cast",
            "check",
            "collate",
            "collation",
            "column",
            "columns",
            "concurrently",
            "constraint",
            "create",
            "cross",
            "default",
            "deferrable",
            "delete",
            "desc",
            "describe",
            "distinct",
            "do",
            "drop",
            "else",
            "end",
            "except",
            "exists",
            "false",
            "fetch",
            "for",
            "foreign",
            "freeze",
            "from",
            "full",
            "generated",
            "glob",
            "group",
            "having",
            "ilike",
            "in",
            "initially",
            "inner",
            "insert",
            "intersect",
            "into",
            "is",
            "isnull",
            "join",
            "lambda",
            "lateral",
            "leading",
            "left",
            "like",
            "limit",
            "map",
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
            "pivot",
            "pivot_longer",
            "pivot_wider",
            "placing",
            "positional",
            "primary",
            "qualify",
            "references",
            "returning",
            "right",
            "select",
            "semi",
            "show",
            "similar",
            "some",
            "struct",
            "summarize",
            "symmetric",
            "table",
            "tablesample",
            "then",
            "to",
            "trailing",
            "true",
            "try_cast",
            "union",
            "unique",
            "unpack",
            "unpivot",
            "update",
            "using",
            "values",
            "variadic",
            "verbose",
            "when",
            "where",
            "window",
            "with",
        ],
    )
});

/// DuckDB's catalog names a column's type as its JDBC driver does; each name's `java.sql.Types` code is the
/// driver's (REAL is FLOAT there).
static CATALOG_GRID_TYPES: Types = Types {
    type_name: "data_type",
    type_key: "CASE WHEN data_type LIKE 'DECIMAL%' THEN 'DECIMAL' ELSE data_type END",
    codes: &[
        ("INTEGER", TypeCode::Integer),
        ("VARCHAR", TypeCode::Varchar),
        ("BIGINT", TypeCode::Bigint),
        ("SMALLINT", TypeCode::Smallint),
        ("TINYINT", TypeCode::Tinyint),
        ("DOUBLE", TypeCode::Double),
        ("FLOAT", TypeCode::Float),
        ("DECIMAL", TypeCode::Decimal),
        ("DATE", TypeCode::Date),
        ("TIMESTAMP", TypeCode::Timestamp),
        ("BOOLEAN", TypeCode::Boolean),
        ("TIME", TypeCode::Time),
        ("TIMESTAMP WITH TIME ZONE", TypeCode::TimestampWithTimezone),
        ("BLOB", TypeCode::Blob),
    ],
};

/// Into `out`, `e`'s conjuncts under AND.
fn conjuncts<'e>(e: &'e SqlExpr, out: &mut Vec<&'e SqlExpr>) {
    match e {
        SqlExpr::Call { r#fn: SqlFn::And, args, .. } => args.iter().for_each(|a| conjuncts(a, out)),
        other => out.push(other),
    }
}

/// The comparisons that are never TRUE for a NULL operand.
fn is_comparison(f: SqlFn) -> bool {
    matches!(f, SqlFn::Equal | SqlFn::NotEqual | SqlFn::Less | SqlFn::LessEqual | SqlFn::Greater | SqlFn::GreaterEqual)
}

/// A stored Variant column: navigation reads it as it is, a whole value used as-is is read as JSON.
fn stored_variant(e: &SqlExpr) -> bool {
    match e {
        SqlExpr::Column(c) => stored_variant_column(c),
        _ => false,
    }
}

fn stored_variant_column(c: &Column) -> bool {
    matches!(c.origin, Some(Origin::Physical | Origin::PhysicalQuoted))
        && matches!(c.r#type, TypeFact::Typed { r#type: SqlType::Scalar(Scalar::Json), .. })
}

/// `Path.of(URI.create(url)).toString()` for a `file:` URL on a POSIX file system: the URI's path, its escapes
/// decoded, repeated and trailing slashes dropped (Java's `UnixPath` normalizes so).
fn file_url_path(url: &str) -> Result<String, Refusal> {
    let rest = &url["file:".len()..];
    if rest.contains('#') {
        return Err(Refusal::illegal_argument("URI has a fragment component"));
    }
    if rest.contains('?') {
        return Err(Refusal::illegal_argument("URI has a query component"));
    }
    let path = match rest.strip_prefix("//") {
        Some(after) => {
            let end = after.find('/').unwrap_or(after.len());
            if end > 0 {
                return Err(Refusal::illegal_argument("URI has an authority component"));
            }
            &after[end..]
        }
        None => rest,
    };
    if !path.starts_with('/') {
        return Err(Refusal::illegal_argument("URI is not hierarchical"));
    }
    let mut bytes = Vec::with_capacity(path.len());
    let raw = path.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%'
            && i + 2 < raw.len() + 1
            && let Some(v) =
                raw.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok())
        {
            bytes.push(v);
            i += 3;
            continue;
        }
        bytes.push(raw[i]);
        i += 1;
    }
    let decoded = String::from_utf8_lossy(&bytes);
    let mut out = String::with_capacity(decoded.len());
    for c in decoded.chars() {
        if c == '/' && out.ends_with('/') {
            continue;
        }
        out.push(c);
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    Ok(out)
}

impl DuckDb {
    /// `strftime`, its format split after `%n` when more of it follows (DuckDB 1.5.5 writes nine fraction digits with
    /// a wrong length when the format goes on).
    fn strftime(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        if args.len() == 2
            && let SqlExpr::FormatLit { parts, .. } = &args[1]
            && let Some(nano) = parts.iter().position(|p| *p == DateFmt::Part(DateFmtPart::SubsecNano))
            && nano < parts.len() - 1
        {
            let tail = &parts[nano + 1..];
            let format = |parts: &[DateFmt]| SqlExpr::FormatLit { parts: parts.to_vec(), r#type: TypeFact::Unknown };
            w.append("(");
            w.function(self, "strftime", &[args[0].clone(), format(&parts[..=nano])])?;
            w.append(" || ");
            if tail.iter().all(|p| matches!(p, DateFmt::Text { .. })) {
                w.expr(self, &format(tail), 0)?;
            } else {
                w.function(self, "strftime", &[args[0].clone(), format(tail)])?;
            }
            w.append(")");
            return Ok(());
        }
        w.function(self, "strftime", args)?;
        Ok(())
    }

    /// The `ACOS` and `ASIN` arms: NaN outside the domain, as the engine's H2.
    fn domain_guarded(&self, w: &mut SqlWriter, f: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("(CASE WHEN (").expr(self, arg(a, 0)?, 0)?.append(") BETWEEN -1 AND 1 THEN ").append(f).append("(");
        w.expr(self, arg(a, 0)?, 0)?.append(") ELSE 'NaN'::DOUBLE END)");
        Ok(())
    }

    /// len(DOUBLE): a non-literal argument stringified first, as the engine's implicit varchar coercion.
    fn length_arm(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        if matches!(arg(args, 0)?, SqlExpr::StringLit { .. }) {
            w.function(self, "length", args)?;
        } else {
            w.append("length(CAST(").expr(self, &args[0], 0)?.append(" AS VARCHAR))");
        }
        Ok(())
    }

    /// date_trunc('day', ts) cast back to the TIMESTAMP the engine's H2 keeps; the coarser parts stay DATE.
    fn date_trunc_arm(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let p = part(arg(args, 0)?)?;
        let spelled = self.string_lit(spellings::date_part(p));
        if part(arg(args, 0)?)? == DatePart::Day {
            w.append("CAST(");
            ansi::date_trunc_date_grained(self, w, args, &spelled)?;
            w.append(" AS TIMESTAMP)");
            return Ok(());
        }
        ansi::date_trunc_date_grained(self, w, args, &spelled)
    }

    fn list_predicate(
        &self,
        w: &mut SqlWriter,
        args: &[SqlExpr],
        agg: &str,
        empty_default: bool,
    ) -> Result<(), Refusal> {
        w.append("coalesce(").append(agg).append("(").function(self, "list_transform", args)?.append("), ");
        let default = self.bool_lit(empty_default);
        w.append(&default).append(")");
        Ok(())
    }

    /// The operand of a navigation: a stored Variant column as it is.
    fn navigated(&self, w: &mut SqlWriter, e: &SqlExpr) -> Result<(), Refusal> {
        match e {
            SqlExpr::Column(c) if stored_variant_column(c) => {
                let text = ansi::column_ref(self, c);
                w.append(&text);
                Ok(())
            }
            _ => self.expr(w, e, 7),
        }
    }

    /// A NULL-valued field spells its declared slot type; a literal-built value under a BIGINT slot does too.
    fn struct_field_value(&self, w: &mut SqlWriter, f: &StructLitField) -> Result<(), Refusal> {
        if let Some(declared) = &f.declared
            && (matches!(f.value.r#type(), TypeFact::Bottom)
                || *declared == SqlType::Scalar(Scalar::Bigint)
                    && !matches!(f.value, SqlExpr::Column(_))
                    && !matches!(f.value, SqlExpr::Cast { .. }))
        {
            w.append("CAST(").expr(self, &f.value, 0)?.append(" AS ");
            let t = ansi::cast_type_name(self, declared)?;
            w.append(&t).append(")");
            return Ok(());
        }
        self.expr(w, &f.value, 0)
    }
}

impl AnsiSqlRenderer for DuckDb {
    fn lexicon(&self) -> &Lexicon {
        &LEXICON
    }

    fn ansi_state(&self) -> &AnsiState {
        &self.ansi
    }

    fn simple_name(&self) -> &'static str {
        "DuckDb"
    }

    /// The engine's H2 text translated for DuckDB: keyword column names quoted, H2's type kinds mapped,
    /// `CURRENT_TIMESTAMP()` without parens.
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        Ok(raw_sql::translated(statement, &|types| types, &|bare| bare.to_owned()))
    }

    fn passes(&self) -> Vec<Box<dyn SqlRewriter>> {
        duckdb_passes(self.engine_scan_order)
    }

    /// An outer join's ON without the IS NOT NULL tests a comparison beside them already makes: DuckDB plans the bare
    /// equality as a hash join.
    fn join_condition<'e>(&self, kind: JoinKind, on: &'e SqlExpr) -> Cow<'e, SqlExpr> {
        if !matches!(kind, JoinKind::Left | JoinKind::Right | JoinKind::Full) {
            return Cow::Borrowed(on);
        }
        let mut all = Vec::new();
        conjuncts(on, &mut all);
        let mut compared: Vec<&SqlExpr> = Vec::new();
        for c in &all {
            if let SqlExpr::Call { r#fn, args, .. } = c
                && is_comparison(*r#fn)
            {
                compared.extend(args.iter());
            }
        }
        let kept: Vec<&SqlExpr> = all
            .iter()
            .copied()
            .filter(|c| {
                !matches!(c, SqlExpr::Call { r#fn: SqlFn::IsNotNull, args, .. }
                    if args.len() == 1 && compared.contains(&&args[0]))
            })
            .collect();
        if kept.len() == all.len() {
            return Cow::Borrowed(on);
        }
        let mut out = kept[0].clone();
        for k in &kept[1..] {
            out = crate::build::call(SqlFn::And, vec![out, (*k).clone()]);
        }
        Cow::Owned(out)
    }

    fn scalar_type_name(&self, s: Scalar) -> Result<String, Refusal> {
        Ok(match s {
            Scalar::Boolean => "BOOLEAN",
            Scalar::Integer => "INTEGER",
            Scalar::Bigint => "BIGINT",
            Scalar::Hugeint => "HUGEINT",
            Scalar::Double => "DOUBLE",
            Scalar::Varchar | Scalar::TemporalText => "VARCHAR",
            Scalar::Date => "DATE",
            Scalar::Timestamp => "TIMESTAMP",
            Scalar::Timestamptz => "TIMESTAMPTZ",
            Scalar::Json => "JSON",
            Scalar::Time => "TIME",
            Scalar::DecimalText => {
                return Err(Refusal::illegal_state(format!("{s} cast reached a dialect without {s} support")));
            }
        }
        .to_owned())
    }

    /// A struct type: `STRUCT(a T, ...)`.
    fn struct_type_name(&self, fields: &[StructField]) -> Result<String, Refusal> {
        let fs: Vec<String> = ansi::struct_fields(self, fields)?.into_iter().map(|(n, t)| format!("{n} {t}")).collect();
        Ok(format!("STRUCT({})", fs.join(", ")))
    }

    /// DuckDB's strftime codes.
    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal> {
        Ok(ansi::strftime_text(parts))
    }

    /// DuckDB's aggregates, each by its name in the shared `name(args)` form.
    fn reducer(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        let name = match r.r#fn {
            AggFn::Sum => "SUM",
            AggFn::Count => "COUNT",
            AggFn::Avg => "AVG",
            AggFn::Min => "MIN",
            AggFn::Max => "MAX",
            AggFn::AnyValue => "ANY_VALUE",
            AggFn::StddevSamp => "STDDEV_SAMP",
            AggFn::StddevPop => "STDDEV_POP",
            AggFn::VarSamp => "VAR_SAMP",
            AggFn::VarPop => "VAR_POP",
            AggFn::Median => "MEDIAN",
            AggFn::Mode => "MODE",
            AggFn::StringAgg => "STRING_AGG",
            AggFn::List => "LIST",
            AggFn::QuantileCont => "QUANTILE_CONT",
            AggFn::QuantileDisc => "QUANTILE_DISC",
            AggFn::Corr => "CORR",
            AggFn::CovarSamp => "COVAR_SAMP",
            AggFn::CovarPop => "COVAR_POP",
            AggFn::ArgMax => "ARG_MAX",
            AggFn::ArgMin => "ARG_MIN",
            AggFn::BoolAnd => "BOOL_AND",
            AggFn::BoolOr => "BOOL_OR",
            AggFn::Variance => "VARIANCE",
            AggFn::Stddev => "STDDEV",
            AggFn::Wavg | AggFn::HashList | AggFn::IsDistinctMark | AggFn::UniqueValueOnly => {
                return Err(ansi::lowering_marker(r));
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
            | AggFn::NthValue => return Err(ansi::window_function(r)),
        };
        ansi::aggregate(self, w, name, r)
    }

    /// DuckDB's documented evaluate-once: `AS MATERIALIZED`.
    fn cte_as(&self, c: &Cte) -> String {
        if c.materialized { " AS MATERIALIZED (" } else { " AS (" }.to_owned()
    }

    /// A date-time finer than a microsecond is a `TIMESTAMP_NS`, as its literal is.
    fn hole_type(&self, kind: ValueKind) -> Result<TypeSpelling, Refusal> {
        if kind == ValueKind::DateTimeNanos {
            return Ok(TypeSpelling::new("TIMESTAMP_NS", Digits::None));
        }
        ansi::hole_type(kind)
    }

    /// A literal with nonzero sub-microsecond digits binds as `TIMESTAMP_NS` (DuckDB's bare TIMESTAMP truncates).
    fn timestamp_lit(&self, iso: &str) -> String {
        if let Some(dot) = iso.rfind('.')
            && iso.len() - dot - 1 > 6
            && let Some(rest) = iso.get(dot + 7..)
        {
            let sub_micro = &rest[..rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len())];
            if !sub_micro.is_empty() && !sub_micro.bytes().all(|b| b == b'0') {
                return format!("TIMESTAMP_NS '{iso}'");
            }
        }
        format!("TIMESTAMP '{iso}'")
    }

    /// DuckDB DDL: a store FLOAT is DOUBLE, a BIT column is BOOLEAN.
    fn ddl_type(&self, t: &ColumnType) -> Result<String, Refusal> {
        Ok(match t {
            ColumnType::Plain { kind } => match kind {
                ColumnKind::Bigint => "BIGINT",
                ColumnKind::Smallint => "SMALLINT",
                ColumnKind::Tinyint => "TINYINT",
                ColumnKind::Integer => "INTEGER",
                ColumnKind::Float | ColumnKind::Double => "DOUBLE",
                ColumnKind::Real => "REAL",
                ColumnKind::Bit => "BOOLEAN",
                ColumnKind::Timestamp => "TIMESTAMP",
                ColumnKind::Date => "DATE",
                ColumnKind::Time => "TIME",
                ColumnKind::TimestampNs => "TIMESTAMP_NS",
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

    fn call(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], parent_prec: i32) -> Result<(), Refusal> {
        use SqlFn as F;
        match f {
            // ---- DuckDB's own spellings
            // the engine's H2 returns NaN for an out-of-domain acos/asin; DuckDB throws
            F::Acos => self.domain_guarded(w, "acos", a),
            F::Asin => self.domain_guarded(w, "asin", a),
            // now(): DuckDB's TIMESTAMPTZ cast to the engine's plain TIMESTAMP (the session is pinned UTC)
            F::Now => {
                w.append("CAST(now() AS TIMESTAMP)");
                Ok(())
            }
            F::BitNot => spellings::bit_not(self, w, a),
            F::MapEmpty => spellings::map_empty(w),
            F::JsonMergePatch => spellings::json_merge_patch(self, w, a),
            F::EncodeBase64 => spellings::encode_base64(self, w, a),
            F::DecodeBase64 => spellings::decode_base64(self, w, a),
            F::MakeTimestamp => spellings::make_timestamp(self, w, a),
            F::TimeBucket => spellings::time_bucket(self, w, a),
            F::FromEpochMs => spellings::from_epoch_ms(self, w, a),
            F::IntDivide => spellings::int_divide(self, w, a),
            // (unit, amount, date): d + to_years(n)
            F::AddInterval | F::AddIntervalTemporal => ansi::op(w, parent_prec, &|w| {
                w.expr(self, arg(a, 2)?, 5)?.append(" + ");
                let unit = spellings::interval_function(part(arg(a, 0)?)?)?;
                w.append(unit).append("(").expr(self, arg(a, 1)?, 0)?.append(")");
                Ok(())
            }),
            // ---- the ANSI operators and the shared spellings
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
            F::Lpad => ansi::pad_with_space_to_integer(self, w, "lpad", a),
            F::Rpad => ansi::pad_with_space_to_integer(self, w, "rpad", a),
            F::UcFirst => ansi::first_char_cased(self, w, "upper", a, parent_prec),
            F::LcFirst => ansi::first_char_cased(self, w, "lower", a, parent_prec),
            F::Guid => {
                w.append("CAST(uuid() AS VARCHAR)");
                Ok(())
            }
            F::Random => {
                w.append("random()");
                Ok(())
            }
            F::Today => {
                w.append("current_date");
                Ok(())
            }
            F::DateTruncDay => ansi::cast_as(self, w, arg(a, 0)?, "DATE"),
            F::DateTrunc => self.date_trunc_arm(w, a),
            F::CurrentUserFn => {
                w.append("current_user");
                Ok(())
            }
            F::IsDistinctFrom | F::NullSafeNotEqual => ansi::distinct_from(self, w, a),
            F::NullSafeEqual => ansi::not_distinct_from(self, w, a),
            F::ParseDate => ansi::cast_as(self, w, arg(a, 0)?, "TIMESTAMP"),
            F::RoundHalfUp => self.plain(w, "ROUND", a),
            F::Error => ansi::raise_envelope(self, w, a, "error"),
            F::BoolToText => ansi::cast_as(self, w, arg(a, 0)?, "VARCHAR"),
            F::FloatCanon => ansi::float_canon_by_text(self, w, a, parent_prec),
            F::FloatPrint => ansi::float_print_by_text(self, w, a, parent_prec),
            // ---- the hooks
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
            F::Length => self.length_arm(w, a),
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
            F::Atan => self.plain(w, "atan", a),
            F::Atan2 => self.plain(w, "atan2", a),
            F::Sinh => self.plain(w, "sinh", a),
            F::Cosh => self.plain(w, "cosh", a),
            F::Tanh => self.plain(w, "tanh", a),
            F::RegexpExtract => self.plain(w, "regexp_extract", a),
            F::RegexpExtractAll => self.plain(w, "regexp_extract_all", a),
            F::RegexpReplace => self.plain(w, "regexp_replace", a),
            F::RegexpFullMatch => self.plain(w, "regexp_full_match", a),
            F::MapFromLists => self.plain(w, "map", a),
            F::MapFromEntries => self.plain(w, "map_from_entries", a),
            F::MapExtract => self.plain(w, "map_extract", a),
            F::MapKeys => self.plain(w, "map_keys", a),
            F::MapValues => self.plain(w, "map_values", a),
            F::MapConcat => self.plain(w, "map_concat", a),
            F::Substring => self.plain(w, "substr", a),
            F::Strpos => self.plain(w, "strpos", a),
            F::StartsWith => self.plain(w, "starts_with", a),
            F::EndsWith => self.plain(w, "ends_with", a),
            F::Matches => self.plain(w, "regexp_matches", a),
            F::Left => self.plain(w, "left", a),
            F::Right => self.plain(w, "right", a),
            F::Trim => self.plain(w, "trim", a),
            F::Ltrim => self.plain(w, "ltrim", a),
            F::Rtrim => self.plain(w, "rtrim", a),
            F::Replace => self.plain(w, "replace", a),
            F::Translate => self.plain(w, "translate", a),
            F::Split => self.plain(w, "string_split", a),
            F::SplitPart => self.plain(w, "split_part", a),
            F::ReverseString => self.plain(w, "reverse", a),
            F::AsciiCode => self.plain(w, "ascii", a),
            // chr takes a 32-bit code; a computed Integer is a BIGINT here
            F::Chr => {
                w.append("chr(CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS INTEGER))");
                Ok(())
            }
            F::Levenshtein => self.plain(w, "levenshtein", a),
            F::Format => self.plain(w, "printf", a),
            F::Md5 => self.plain(w, "md5", a),
            F::Sha1 => self.plain(w, "sha1", a),
            F::Sha256 => self.plain(w, "sha256", a),
            F::Extract => {
                let p = self.string_lit(spellings::date_part(part(arg(a, 0)?)?));
                w.append("date_part(").append(&p).append(", ").list(self, tail(a, 1)?)?.append(")");
                Ok(())
            }
            F::MakeDate => self.plain(w, "make_date", a),
            F::DateDiff => {
                let p = self.string_lit(spellings::date_part(part(arg(a, 0)?)?));
                w.append("date_diff(").append(&p).append(", ").list(self, tail(a, 1)?)?.append(")");
                Ok(())
            }
            F::Timezone => self.plain(w, "timezone", a),
            // a date plus a time of day is the timestamp at it
            F::DateAtTime => ansi::op(w, parent_prec, &|w| {
                w.expr(self, arg(a, 0)?, 5)?.append(" + ").expr(self, arg(a, 1)?, 6)?;
                Ok(())
            }),
            F::EpochSeconds => self.plain(w, "epoch", a),
            F::EpochMs => self.plain(w, "epoch_ms", a),
            F::EpochNs => self.plain(w, "epoch_ns", a),
            F::FromEpochNs => self.plain(w, "make_timestamp_ns", a),
            F::FromEpochSeconds => self.plain(w, "to_timestamp", a),
            F::Dayname => self.plain(w, "dayname", a),
            F::Monthname => self.plain(w, "monthname", a),
            F::Cot => self.plain(w, "cot", a),
            F::Radians => self.plain(w, "radians", a),
            F::Degrees => self.plain(w, "degrees", a),
            F::RepeatStr => self.plain(w, "repeat", a),
            F::JaroWinkler => self.plain(w, "jaro_winkler_similarity", a),
            F::ListLength => self.plain(w, "len", a),
            F::Strftime => self.strftime(w, a),
            F::Strptime => self.plain(w, "strptime", a),
            F::FloorRaw => self.plain(w, "floor", a),
            F::JsonType => self.plain(w, "json_type", a),
            F::JsonArrayLength => self.plain(w, "json_array_length", a),
            F::JsonPretty => self.plain(w, "json_pretty", a),
        }
    }

    /// DuckDB DDL identifier identity: an unquoted identifier spelled folded (as the standard reports it), a
    /// declared-quoted one keeps its case.
    fn ddl_identifier(&self, name: &str, declared_quoted: bool) -> String {
        if declared_quoted { format!("\"{name}\"") } else { self.ident(&name.to_uppercase()) }
    }

    /// DuckDB's native list carrier: `list_aggregate(list, 'name', extras...)`.
    fn reduce_collection(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        collection: &SqlExpr,
        extras: &[SqlExpr],
    ) -> Result<(), Refusal> {
        w.append("list_aggregate(").expr(self, collection, 0)?.append(", '");
        w.append(&reducer.as_str().to_ascii_lowercase()).append("'");
        for x in extras {
            w.append(", ").expr(self, x, 0)?;
        }
        w.append(")");
        Ok(())
    }

    /// DuckDB's native membership.
    fn membership(&self, w: &mut SqlWriter, needle: &SqlExpr, collection: &SqlExpr) -> Result<(), Refusal> {
        w.append("list_contains(").expr(self, collection, 0)?.append(", ").expr(self, needle, 0)?.append(")");
        Ok(())
    }

    fn supports_qualify(&self) -> bool {
        true
    }

    fn append_qualify(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        let Some(q) = &s.qualify else {
            return Err(Refusal::null_pointer("appendQualify without a qualify clause"));
        };
        self.nl(w, depth);
        w.append("QUALIFY ").expr(self, q, 0)?;
        Ok(())
    }

    fn as_of_join_clause(&self) -> Result<String, Refusal> {
        Ok("ASOF LEFT JOIN".to_owned())
    }

    /// Native PIVOT; its ON columns quote unconditionally, its arguments arrive unqualified.
    fn pivot_source(&self, w: &mut SqlWriter, pivot: &SqlSource, depth: usize) -> Result<(), Refusal> {
        let SqlSource::Pivot { source, on, r#in, usings, alias, .. } = pivot else {
            return Err(Refusal::illegal_state("a PIVOT of a source that is no pivot"));
        };
        w.append("(PIVOT ");
        self.source(w, source, depth)?;
        w.append(" ON ").join(on, ", ", |w, e| match e {
            SqlExpr::Column(c) => {
                let text = self.delimited(&c.name);
                w.append(&text);
                Ok(())
            }
            _ => self.expr(w, e, 0),
        })?;
        if !r#in.is_empty() {
            w.append(" IN (").list(self, r#in)?.append(")");
        }
        // real pure names pivot columns value__|__agg; DuckDB joins value + '_' + alias
        w.append(" USING ").join(usings, ", ", |w, u| {
            self.reducer(w, &u.agg)?;
            let name = self.ident(&format!("_|__{}", u.alias));
            w.append(" AS ").append(&name);
            Ok(())
        })?;
        let a = self.ident(alias);
        w.append(") AS ").append(&a);
        Ok(())
    }

    fn lambda(&self, w: &mut SqlWriter, l: &Lambda) -> Result<(), Refusal> {
        let params = ansi::lambda_params(l);
        w.append(&params).append(" -> ").expr(self, &l.body, 0)?;
        Ok(())
    }

    /// DuckDB keeps the SQL standard's `information_schema`.
    fn catalog(&self, w: &mut SqlWriter, catalog: &SqlSource) -> Result<(), Refusal> {
        catalog::write(self, w, catalog, &CATALOG_GRID_TYPES)
    }

    /// DuckDB runs the standard spelling.
    fn temp_table_as(&self, dml: &SqlDml) -> Result<SqlWriter, Refusal> {
        ansi::create_temporary_table_as(self, dml, false, "CREATE TEMPORARY TABLE ", "")
    }

    /// `data:application/json,[...]` inlines the payload as a JSON array unnested one row per element; `file:` reads
    /// objects.
    fn source_url(&self, url: &str) -> Result<String, Refusal> {
        if url.starts_with("data:") {
            let Some(comma) = url.find(',') else {
                return Err(Refusal::illegal_state(format!("invalid data: URI (no comma): {url}")));
            };
            let content = &url[comma + 1..];
            return Ok(format!("SELECT unnest(CAST({} AS JSON[])) AS data", self.string_lit(content)));
        }
        if url.starts_with("file:") {
            let path = file_url_path(url)?;
            return Ok(format!("SELECT json AS data FROM read_json_objects({})", self.string_lit(&path)));
        }
        Err(Refusal::illegal_state(format!("unsupported sourceUrl scheme: {url}")))
    }

    /// Pure semantics ride the expansion: exists([]) = false, forAll([]) = true.
    fn list_exists(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list_predicate(w, args, "list_bool_or", false)
    }

    fn list_for_all(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list_predicate(w, args, "list_bool_and", true)
    }

    /// len(list_distinct(x)) = len(x); NULL (empty) coalesces to true.
    fn all_distinct(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("coalesce(len(list_distinct(").expr(self, arg(args, 0)?, 0)?.append(")) = len(");
        w.expr(self, arg(args, 0)?, 0)?.append("), TRUE)");
        Ok(())
    }

    fn list_call(&self, w: &mut SqlWriter, f: SqlFn, args: &[SqlExpr]) -> Result<(), Refusal> {
        use SqlFn as F;
        let aggregate = |w: &mut SqlWriter, name: &str| -> Result<(), Refusal> {
            w.append("list_aggregate(").expr(self, arg(args, 0)?, 0)?.append(", '").append(name).append("')");
            Ok(())
        };
        match f {
            F::ListFilter => self.plain(w, "list_filter", args),
            F::ListTransform => self.plain(w, "list_transform", args),
            F::ListFlatten => self.plain(w, "flatten", args),
            F::ListConcat => self.plain(w, "list_concat", args),
            F::JsonMergePatch => self.plain(w, "json_merge_patch", args),
            F::ListGet => self.plain(w, "list_extract", args),
            F::ListPosition => self.plain(w, "list_position", args),
            F::ListZip => self.plain(w, "list_zip", args),
            F::ListDistinct => self.plain(w, "list_distinct", args),
            F::ListAppend => self.plain(w, "list_append", args),
            F::ListSum => self.plain(w, "list_sum", args),
            F::ListMin => self.plain(w, "list_min", args),
            F::ListMax => self.plain(w, "list_max", args),
            F::ListAvg => self.plain(w, "list_avg", args),
            F::ListMedian => self.plain(w, "list_median", args),
            F::ListMode => aggregate(w, "mode"),
            F::ListProduct => aggregate(w, "product"),
            F::ListReduce => self.plain(w, "list_reduce", args),
            F::ListSlice => self.plain(w, "array_slice", args),
            F::ListBoolAnd => aggregate(w, "bool_and"),
            F::ListBoolOr => aggregate(w, "bool_or"),
            F::ListReverse => self.plain(w, "list_reverse", args),
            F::Typeof => self.plain(w, "typeof", args),
            F::ListSort => self.plain(w, "list_sort", args),
            F::ListSortDesc => self.plain(w, "list_reverse_sort", args),
            F::ListTail => {
                w.expr(self, arg(args, 0)?, 8)?.append("[2:]");
                Ok(())
            }
            F::ListInit => {
                w.expr(self, arg(args, 0)?, 8)?.append("[:-2]");
                Ok(())
            }
            F::RangeFn => self.plain(w, "range", args),
            F::RepeatValue => {
                w.append("list_transform(range(").expr(self, arg(args, 1)?, 0)?.append("), _i -> ");
                w.expr(self, arg(args, 0)?, 0)?.append(")");
                Ok(())
            }
            other => Err(Refusal::illegal_state(format!("not a list call: {other}"))),
        }
    }

    /// hash() is UBIGINT: the sign bit flipped in unsigned space, then shifted down by 2^63 in HUGEINT space.
    fn hash_signed(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("CAST(CAST(xor(").function(self, "hash", a)?;
        w.append(", CAST(9223372036854775808 AS UBIGINT)) AS HUGEINT) - 9223372036854775808 AS BIGINT)");
        Ok(())
    }

    /// Pure's ROUND is half-even: an exact decimal to a constant scale steps an odd half back to its even
    /// neighbour; anything else is `ROUND_EVEN`.
    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 2
            && matches!(a[0].r#type(), TypeFact::Typed { r#type: SqlType::Decimal { .. }, .. })
            && let SqlExpr::IntLit { value: n, .. } = &a[1]
            && (0..=37).contains(n)
        {
            let (u, h, twice) = ansi::decimal_unit(*n);
            let x = |w: &mut SqlWriter| -> Result<(), Refusal> {
                w.append("(").expr(self, &a[0], 0)?.append(")");
                Ok(())
            };
            let rounded = |w: &mut SqlWriter| -> Result<(), Refusal> {
                w.append("ROUND(").piece(&x)?.append(&format!(", {n})"));
                Ok(())
            };
            w.append("(CASE WHEN abs(").piece(&x)?.append(&format!(" % {u}) = {h} AND ")).piece(&rounded)?;
            w.append(&format!(" % {twice} <> 0 THEN ")).piece(&rounded)?.append(" - sign(").piece(&x)?;
            w.append(&format!(") * {u} ELSE ")).piece(&rounded)?.append(" END)");
            return Ok(());
        }
        // round_even is a 2-arg macro: bare round(x) means precision 0
        if a.len() == 1 {
            w.append("ROUND_EVEN(").expr(self, &a[0], 0)?.append(", 0)");
            Ok(())
        } else {
            self.plain(w, "ROUND_EVEN", a)
        }
    }

    fn bit_op(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let binary = |w: &mut SqlWriter, op: &str| -> Result<(), Refusal> {
            w.append("(").expr(self, arg(a, 0)?, 6)?.append(op).expr(self, arg(a, 1)?, 6)?.append(")");
            Ok(())
        };
        match f {
            SqlFn::BitAnd => binary(w, " & "),
            SqlFn::BitOr => binary(w, " | "),
            SqlFn::BitXor => self.plain(w, "xor", a),
            SqlFn::BitShiftLeft => binary(w, " << "),
            SqlFn::BitShiftRight => binary(w, " >> "),
            other => Err(Refusal::illegal_state(format!("not a bit op: {other}"))),
        }
    }

    /// A stored Variant column is read as it is: to_json converts either storage once.
    fn variant_construct(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 1 && stored_variant(&a[0]) {
            w.append("to_json(");
            self.navigated(w, &a[0])?;
            w.append(")");
            return Ok(());
        }
        self.plain(w, "to_json", a)
    }

    /// DuckDB explodes a select-list unnest into rows.
    fn unnest_projection(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.plain(w, "UNNEST", args)
    }

    fn array_lit(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("[").list(self, elements)?.append("]");
        Ok(())
    }

    /// A struct literal, each field's name a string literal (a Pure property name may carry quotes).
    fn struct_lit(&self, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
        w.append("{").join(fields, ", ", |w, f| {
            let name = self.string_lit(&f.name);
            w.append(&name).append(": ");
            self.struct_field_value(w, f)
        })?;
        w.append("}");
        Ok(())
    }

    /// A positional field (list_zip's unnamed structs) is its 1-based index, never a quoted name.
    fn struct_get(&self, w: &mut SqlWriter, source: &SqlExpr, field: &str) -> Result<(), Refusal> {
        w.append("struct_extract(").expr(self, source, 0)?.append(", ");
        let f = if field.chars().all(|c| c.is_ascii_digit()) { field.to_owned() } else { self.string_lit(field) };
        w.append(&f).append(")");
        Ok(())
    }

    /// JSON access, parenthesized always (DuckDB's lambda arrow and the JSON arrow collide inside list lambdas).
    fn variant_get(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("(");
        self.navigated(w, arg(args, 0)?)?;
        w.append(" -> ").expr(self, arg(args, 1)?, 8)?.append(")");
        Ok(())
    }

    /// A stored Variant column reference is read as JSON: `CAST(col AS JSON)`.
    fn column_ref(&self, c: &Column) -> String {
        let r = ansi::column_ref(self, c);
        if stored_variant_column(c) { format!("CAST({r} AS JSON)") } else { r }
    }

    fn implicit_label(&self, p: &Projection) -> Option<String> {
        match &p.expr {
            SqlExpr::Column(c) if stored_variant_column(c) => Some(self.alias_ident(&c.name)),
            _ => ansi::implicit_label(self, p),
        }
    }

    /// A Variant's elements: an array's, a JSON null element kept as JSON's null; any other value the error Pure's
    /// toMany raises.
    fn variant_elements(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let v = arg(args, 0)?;
        let json = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("CAST(");
            if stored_variant(v) {
                self.navigated(w, v)?;
            } else {
                w.expr(self, v, 0)?;
            }
            w.append(" AS JSON)");
            Ok(())
        };
        w.append("(CASE WHEN json_type(").piece(&json)?.append(") = 'ARRAY' THEN CAST(json_extract(").piece(&json)?;
        w.append(", '$[*]') AS JSON[]) WHEN ").piece(&json)?.append(" IS NULL OR json_type(").piece(&json)?;
        w.append(") = 'NULL' THEN NULL ELSE error('Expect variant that contains an ''ARRAY'',");
        w.append(" but got ''' || json_type(").piece(&json)?.append(") || '''') END)");
        Ok(())
    }

    fn struct_insert(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let name = match arg(args, 1)? {
            SqlExpr::StringLit { value, .. } => value,
            other => return Err(Refusal::class_cast(crate::java::simple_name(other), "StringLit")),
        };
        w.append("struct_insert(").expr(self, arg(args, 0)?, 0)?.append(", \"");
        w.append(&name.replace('"', "\"\"")).append("\" := ").expr(self, arg(args, 2)?, 0)?.append(")");
        Ok(())
    }

    /// A scalar cast of a variant access extracts text first (`->>`); a whole stored Variant casts as it is, but to
    /// text.
    fn variant_aware_cast(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        if !matches!(target, SqlType::Array { .. })
            && let SqlExpr::Call { r#fn: SqlFn::VariantGet, args, .. } = value
        {
            w.append("CAST((");
            self.navigated(w, arg(args, 0)?)?;
            w.append(" ->> ").expr(self, arg(args, 1)?, 8)?.append(") AS ");
            let t = ansi::cast_type_name(self, target)?;
            w.append(&t).append(")");
            return Ok(());
        }
        if stored_variant(value)
            && !matches!(target, SqlType::Scalar(Scalar::Varchar | Scalar::TemporalText | Scalar::DecimalText))
        {
            w.append("CAST(");
            self.navigated(w, value)?;
            w.append(" AS ");
            let t = ansi::cast_type_name(self, target)?;
            w.append(&t).append(")");
            return Ok(());
        }
        ansi::variant_aware_cast(self, w, value, target, conform)
    }

    /// splitPart over the list encoding: `list_extract(list_filter(string_split(s, t), x -> x <> ''), p)`.
    fn split_part_call(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        use crate::build::{call, derived, lambda, string_lit};
        let parts = call(SqlFn::Split, vec![arg(a, 0)?.clone(), arg(a, 1)?.clone()]);
        let non_empty = call(
            SqlFn::ListFilter,
            vec![parts, lambda(&["x"], call(SqlFn::NotEqual, vec![derived(None, "x"), string_lit("")]))],
        );
        self.expr(w, &call(SqlFn::ListGet, vec![non_empty, arg(a, 2)?.clone()]), 0)
    }

    /// DuckDB's own json_type names.
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
        Err(Refusal::capability(format!("a TYPEOF compared with {scalar}'s name reached DuckDB")))
    }

    /// Measured on DuckDB 1.4: the standard spellings of the store writes.
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

impl DuckDb {
    /// `writer.function(name, args)`: this database's plain spelling of a call.
    fn plain(&self, w: &mut SqlWriter, name: &str, args: &[SqlExpr]) -> Result<(), Refusal> {
        w.function(self, name, args)?;
        Ok(())
    }
}
