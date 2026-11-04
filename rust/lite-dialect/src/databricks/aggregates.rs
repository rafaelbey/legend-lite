// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Databricks' bit operators, its aggregates and windows -- every `SqlAgg.Fn` decided by an exhaustive match -- the
//! frame bounds, the collected lists and the date format (Databricks.java, "Hooks of the base" to "Date format codes").

use super::{Databricks, P, pc, type_of};
use crate::ansi::{self, cast_type_name};
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{
    AggFn, Bound, DateFmt, DateFmtPart, Frame, FrameKind, JavaNumber, NullOrder, Reducer, Scalar, SortKey, SqlAgg,
    SqlExpr, SqlFn, SqlType,
};

/// A sort key of a collected list: the key, its direction, NULLs first or last (Java's private `SortKey` record).
pub(super) struct Key<'a> {
    pub key: P<'a>,
    pub ascending: bool,
    pub nulls_first: bool,
}

/// An array_sort comparator over the structs' keys from the `i`-th: -1, 0 or 1, NULLs where they sort (Java's
/// `comparator`).
fn comparator(keys: &[Key<'_>], i: usize) -> String {
    if i == keys.len() {
        return "0".to_owned();
    }
    let k = &keys[i];
    let l = format!("`__l`.`k{i}`");
    let r = format!("`__r`.`k{i}`");
    let rest = comparator(keys, i + 1);
    format!(
        "CASE WHEN {l} IS NULL AND {r} IS NULL THEN {rest} WHEN {l} IS NULL THEN {} WHEN {r} IS NULL THEN {} WHEN {l} \
         < {r} THEN {} WHEN {l} > {r} THEN {} ELSE {rest} END",
        if k.nulls_first { "-1" } else { "1" },
        if k.nulls_first { "1" } else { "-1" },
        if k.ascending { "-1" } else { "1" },
        if k.ascending { "1" } else { "-1" },
    )
}

/// `writer`, for an aggregate Databricks cannot order: an ordered one is refused (Java's `unordered`).
fn unordered(r: &Reducer) -> Result<(), Refusal> {
    if !r.order_by.is_empty() {
        return Err(Refusal::capability(format!("an ordered {} reached Databricks", r.r#fn)));
    }
    Ok(())
}

/// A Java `Number`'s `toString`.
fn number_text(n: &JavaNumber) -> String {
    match n {
        JavaNumber::Long(v) => v.to_string(),
        JavaNumber::Integer(v) => v.to_string(),
        JavaNumber::Double(v) => lite_javacompat::double_to_string(v.value()),
        JavaNumber::BigDecimal(v) => v.as_str().to_owned(),
    }
}

/// `Math.multiplyExact`'s overflow: Java's `ArithmeticException`.
fn multiply_exact(a: i64, b: i64) -> Result<i64, Refusal> {
    a.checked_mul(b)
        .ok_or_else(|| Refusal { class: "java.lang.ArithmeticException", message: "long overflow".to_owned() })
}

/// An interval frame offset as a DAY TO SECOND or YEAR TO MONTH literal (Java's `interval`): Spark wants both bounds of
/// a frame of ONE interval type.
pub(super) fn interval(n: i64, unit: &str) -> Result<String, Refusal> {
    let u = unit.to_uppercase();
    let seconds: i64 = match u.as_str() {
        "WEEKS" | "WEEK" => 7 * 86400,
        "DAYS" | "DAY" => 86400,
        "HOURS" | "HOUR" => 3600,
        "MINUTES" | "MINUTE" => 60,
        "SECONDS" | "SECOND" => 1,
        _ => -1,
    };
    if seconds > 0 {
        let total = multiply_exact(n, seconds)?;
        return Ok(format!(
            "INTERVAL '{} {:02}:{:02}:{:02}' DAY TO SECOND",
            total / 86400,
            total % 86400 / 3600,
            total % 3600 / 60,
            total % 60
        ));
    }
    let months: i64 = match u.as_str() {
        "YEARS" | "YEAR" => 12,
        "MONTHS" | "MONTH" => 1,
        _ => return Err(Refusal::capability(format!("a {unit} window frame offset has no Databricks interval"))),
    };
    let total = multiply_exact(n, months)?;
    Ok(format!("INTERVAL '{}-{}' YEAR TO MONTH", total / 12, total % 12))
}

/// The date format in Java's DateTimeFormatter letters (`date_format`; Java's `formatText`).
pub(super) fn format_text(parts: &[DateFmt]) -> String {
    let mut out = String::new();
    for p in parts {
        match p {
            DateFmt::Text { s } => {
                if !s.is_empty() {
                    out.push('\'');
                    out.push_str(&s.replace('\'', "''"));
                    out.push('\'');
                }
            }
            DateFmt::Part(part) => out.push_str(match part {
                DateFmtPart::Year4 => "yyyy",
                DateFmtPart::Month2 => "MM",
                DateFmtPart::Day2 => "dd",
                DateFmtPart::Hour2 => "HH",
                DateFmtPart::Min2 => "mm",
                DateFmtPart::Sec2 => "ss",
                DateFmtPart::SubsecMicro => "SSSSSS",
                DateFmtPart::SubsecNano => "SSSSSS'000'",
                // DuckDB's %g: milliseconds, three digits
                DateFmtPart::SubsecMin => "SSS",
                DateFmtPart::MonthAbbrev => "MMM",
                DateFmtPart::MonthName => "MMMM",
                DateFmtPart::WeekdayName => "EEEE",
                DateFmtPart::Hour12 => "hh",
                DateFmtPart::Hour12Nopad => "h",
                DateFmtPart::Ampm => "a",
            }),
        }
    }
    out
}

impl Databricks {
    /// The values collected into a list in the keys' order, NULL values kept (collect_list drops them): each row's
    /// value and keys as a struct, sorted by a comparator over the keys, then the value read back (Java's
    /// `collected`).
    pub(super) fn collected<'a>(&'a self, value: P<'a>, keys: Vec<Key<'a>>) -> P<'a> {
        pc(move |w| {
            w.append("transform(");
            if !keys.is_empty() {
                w.append("array_sort(");
            }
            w.append("collect_list(named_struct('v', ").piece(&*value)?;
            for (i, k) in keys.iter().enumerate() {
                w.append(&format!(", 'k{i}', ")).piece(&*k.key)?;
            }
            w.append("))");
            if !keys.is_empty() {
                w.append(", (`__l`, `__r`) -> ").append(&comparator(&keys, 0)).append(")");
            }
            w.append(", `__s` -> `__s`.`v`)");
            Ok(())
        })
    }

    /// A collected list's key from a sort key: NULLs where the key says, else where they sort (Java's `sortKeyOf`).
    fn sort_key_of<'a>(&'a self, k: &'a SortKey) -> Key<'a> {
        let nulls_first = match k.null_order {
            Some(o) => o == NullOrder::NullsFirst,
            None => !k.ascending,
        };
        Key { key: self.ex(&k.expr, 0), ascending: k.ascending, nulls_first }
    }

    pub(super) fn bit_op_arm(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let x = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS BIGINT)");
            Ok(())
        };
        let y = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.expr(self, arg(a, 1)?, 0)?;
            Ok(())
        };
        match f {
            SqlFn::BitAnd => w.append("(").piece(&x)?.append(" & CAST(").piece(&y)?.append(" AS BIGINT))"),
            SqlFn::BitOr => w.append("(").piece(&x)?.append(" | CAST(").piece(&y)?.append(" AS BIGINT))"),
            SqlFn::BitXor => w.append("(").piece(&x)?.append(" ^ CAST(").piece(&y)?.append(" AS BIGINT))"),
            SqlFn::BitShiftLeft => w.append("shiftleft(").piece(&x)?.append(", CAST(").piece(&y)?.append(" AS INT))"),
            SqlFn::BitShiftRight => {
                w.append("shiftright(").piece(&x)?.append(", CAST(").piece(&y)?.append(" AS INT))")
            }
            other => return Err(Refusal::illegal_state(format!("not a bit op: {other}"))),
        };
        Ok(())
    }

    /// Every aggregate, as Databricks spells it (Java's `reducer`).
    pub(super) fn reducer_arm(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        let plain = |w: &mut SqlWriter, name: &str| -> Result<(), Refusal> {
            unordered(r)?;
            ansi::aggregate(self, w, name, r)
        };
        match r.r#fn {
            AggFn::Sum => plain(w, "SUM"),
            AggFn::Count => plain(w, "COUNT"),
            AggFn::Avg => plain(w, "AVG"),
            AggFn::Min => plain(w, "MIN"),
            AggFn::Max => plain(w, "MAX"),
            AggFn::AnyValue => plain(w, "ANY_VALUE"),
            AggFn::StddevSamp => plain(w, "STDDEV_SAMP"),
            AggFn::StddevPop => plain(w, "STDDEV_POP"),
            AggFn::VarSamp => plain(w, "VAR_SAMP"),
            AggFn::VarPop => plain(w, "VAR_POP"),
            AggFn::Median => {
                w.append("median(").expr(self, arg(&r.args, 0)?, 0)?.append(")");
                Ok(())
            }
            AggFn::Mode => {
                w.append("mode(").expr(self, arg(&r.args, 0)?, 0)?.append(")");
                Ok(())
            }
            AggFn::StringAgg => self.listagg(w, r),
            AggFn::List => self.array_agg(w, r),
            AggFn::QuantileCont => self.percentile(w, "percentile_cont", r, false),
            // percentile_disc answers a DOUBLE whatever its input: cast to the value's own type
            AggFn::QuantileDisc => self.percentile(w, "percentile_disc", r, true),
            AggFn::Corr => plain(w, "CORR"),
            AggFn::CovarSamp => plain(w, "COVAR_SAMP"),
            AggFn::CovarPop => plain(w, "COVAR_POP"),
            // the value at the key's greatest (least) row, rows of a NULL key ignored
            AggFn::ArgMax => self.by_extreme(w, "max_by", r),
            AggFn::ArgMin => self.by_extreme(w, "min_by", r),
            AggFn::BoolAnd => plain(w, "BOOL_AND"),
            AggFn::BoolOr => plain(w, "BOOL_OR"),
            AggFn::Variance => plain(w, "VARIANCE"),
            AggFn::Stddev => plain(w, "STDDEV"),
            AggFn::Wavg | AggFn::HashList | AggFn::IsDistinctMark | AggFn::UniqueValueOnly => {
                Err(ansi::lowering_marker(r))
            }
            // a window function is a ranking or value function, never an aggregate (windowCall writes it)
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

    /// The `STRING_AGG` arm: `listagg(...)[ WITHIN GROUP (ORDER BY ...)]` (Java's `listagg`).
    fn listagg(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        w.append("listagg(").append(if r.distinct { "DISTINCT " } else { "" }).list(self, &r.args)?.append(")");
        if !r.order_by.is_empty() {
            w.append(" WITHIN GROUP (ORDER BY ");
            w.join(&r.order_by, ", ", |w, k| ansi::aggregate_order_key(self, w, k))?.append(")");
        }
        Ok(())
    }

    /// `name(level) WITHIN GROUP (ORDER BY value[ DESC])`, cast to the value's own type when `cast` (Java's
    /// `percentile`).
    fn percentile(&self, w: &mut SqlWriter, name: &str, r: &Reducer, cast: bool) -> Result<(), Refusal> {
        if r.args.len() != 2 || r.distinct || r.order_by.len() > 1 {
            return Err(Refusal::capability(format!("{} of this shape reached Databricks", r.r#fn)));
        }
        let desc = r.order_by.first().is_some_and(|k| !k.ascending);
        let ty = match &r.r#type {
            lite_sql::TypeFact::Typed { r#type, .. } => Some(r#type),
            _ => None,
        };
        let cast_to = match ty {
            Some(t) if cast => Some(cast_type_name(self, t)?),
            _ => None,
        };
        if cast_to.is_some() {
            w.append("CAST(");
        }
        w.append(name).append("(").expr(self, &r.args[1], 0)?.append(") WITHIN GROUP (ORDER BY ");
        w.expr(self, &r.args[0], 0)?.append(if desc { " DESC" } else { "" }).append(")");
        if let Some(t) = cast_to {
            w.append(" AS ").append(&t).append(")");
        }
        Ok(())
    }

    /// The `LIST` arm (Java's `arrayAgg`): the values collected in the reducer's order.
    fn array_agg(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        if r.distinct || r.args.len() != 1 {
            return Err(Refusal::capability("a distinct LIST aggregate reached Databricks"));
        }
        let keys = r.order_by.iter().map(|k| self.sort_key_of(k)).collect();
        self.collected(self.ex(&r.args[0], 0), keys)(w)
    }

    /// `max_by(value, key)` / `min_by`: the value at the key's greatest (least) row (Java's `byExtreme`).
    fn by_extreme(&self, w: &mut SqlWriter, name: &str, r: &Reducer) -> Result<(), Refusal> {
        if r.args.len() != 2 || r.distinct || !r.order_by.is_empty() {
            return Err(Refusal::capability(format!("{} of this shape reached Databricks", r.r#fn)));
        }
        w.append(name).append("(").list(self, &r.args)?.append(")");
        Ok(())
    }

    /// A windowed average is a DOUBLE; a value or ranking function's offset is an INT literal; a RANGE frame's offset
    /// has its ORDER BY key's type (Java's `windowCall`, which writes its own OVER clause).
    pub(super) fn window_call_arm(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        let cast = matches!(f, SqlAgg::Reducer(r) if r.r#fn == AggFn::Avg);
        if cast {
            w.append("CAST(");
        }
        match f {
            SqlAgg::Reducer(r) => self.reducer(w, r)?,
            SqlAgg::RankingFn { r#fn, args } | SqlAgg::ValueFn { r#fn, args } => {
                let name = self.aggregate_name(*r#fn);
                w.append(&name).append("(").join(args, ", ", |w, e| self.int_arg(w, e))?.append(")");
            }
        }
        w.append(" OVER (");
        if !partition_by.is_empty() {
            w.append("PARTITION BY ").list(self, partition_by)?;
        }
        if !order_by.is_empty() {
            w.append(if partition_by.is_empty() { "" } else { " " }).append("ORDER BY ");
            w.join(order_by, ", ", |w, k| self.sort_key(w, k))?;
        } else if matches!(f, SqlAgg::RankingFn { .. }) {
            // Spark refuses a ranking window with no order; a constant order numbers the rows as they come
            w.append(if partition_by.is_empty() { "" } else { " " }).append("ORDER BY 1");
        }
        if let Some(fr) = frame {
            let key =
                if fr.kind == FrameKind::Range && order_by.len() == 1 { type_of(&order_by[0].expr) } else { None };
            let text = format!(
                " {} BETWEEN {} AND {}",
                fr.kind.as_str(),
                self.frame_bound(&fr.from, key)?,
                self.frame_bound(&fr.to, key)?
            );
            w.append(&text);
        }
        w.append(")");
        if cast {
            w.append(" AS DOUBLE)");
        }
        Ok(())
    }

    /// A frame bound: a ROWS offset is an INT; a RANGE offset is its key's type; an interval offset singular (Java's
    /// `frameBound`).
    fn frame_bound(&self, b: &Bound, key: Option<&SqlType>) -> Result<String, Refusal> {
        Ok(match b {
            Bound::UnboundedPreceding => "UNBOUNDED PRECEDING".to_owned(),
            Bound::CurrentRow => "CURRENT ROW".to_owned(),
            Bound::UnboundedFollowing => "UNBOUNDED FOLLOWING".to_owned(),
            Bound::Preceding { n } => format!("{} PRECEDING", self.offset(n, key)?),
            Bound::Following { n } => format!("{} FOLLOWING", self.offset(n, key)?),
            Bound::IntervalPreceding { n, unit } => format!("{} PRECEDING", interval(*n, unit)?),
            Bound::IntervalFollowing { n, unit } => format!("{} FOLLOWING", interval(*n, unit)?),
        })
    }

    /// A frame offset in its key's type (Java's `offset`).
    fn offset(&self, n: &JavaNumber, key: Option<&SqlType>) -> Result<String, Refusal> {
        let whole = matches!(n, JavaNumber::Long(_) | JavaNumber::Integer(_));
        let Some(key) = key else {
            return Ok(number_text(n));
        };
        if *key == SqlType::Scalar(Scalar::Integer) && whole {
            return Ok(number_text(n));
        }
        if *key == SqlType::Scalar(Scalar::Bigint) && whole {
            return Ok(format!("{}L", number_text(n)));
        }
        Ok(format!("CAST({} AS {})", number_text(n), cast_type_name(self, key)?))
    }

    /// A ranking or value function's argument: an integer literal as its bare digits (an INT, never a BIGINT).
    fn int_arg(&self, w: &mut SqlWriter, e: &SqlExpr) -> Result<(), Refusal> {
        if let SqlExpr::IntLit { value, .. } = e {
            w.append(&value.to_string());
        } else {
            w.expr(self, e, 0)?;
        }
        Ok(())
    }
}
