// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! BigQuery's aggregates and windows (Java's `reducer` and its arms, the grouped quantiles, the analytic PERCENTILE_*
//! and the descending one, `bqOver` with its RANGE keys and interval offsets), the bit operators, the date format, the
//! DDL and the DDL types (BigQuery.java, "Hooks of the base" to "DDL and DML").

use super::{BigQuery, Mode, P, pc, type_of, type_text};
use crate::ansi;
use crate::pieces::Ex;
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::typing;
use crate::writer::SqlWriter;
use lite_sql::{
    AggFn, Bound, ColumnKind, ColumnType, DateFmt, Frame, FrameKind, NullOrder, Reducer, Scalar, SortKey, SqlAgg,
    SqlDdl, SqlExpr, SqlFn, SqlType,
};

/// An ORDER BY key of an aggregate or a window, its NULLs placed by a leading `(k) IS NULL` key (Java's `orderKey`).
pub(super) fn order_key(key: P<'_>, ascending: bool, nulls_first: bool) -> P<'_> {
    pc(move |w| {
        w.append("(").piece(&*key)?.append(") IS NULL").append(if nulls_first { " DESC, " } else { " ASC, " });
        w.piece(&*key)?.append(if ascending { " ASC" } else { " DESC" });
        Ok(())
    })
}

/// A key's NULL placement: its declared one, else NULL is largest (Java's `nullsFirst`).
fn nulls_first(k: &SortKey) -> bool {
    match k.null_order {
        Some(order) => order == NullOrder::NullsFirst,
        None => !k.ascending,
    }
}

fn offset(b: &Bound) -> bool {
    matches!(b, Bound::Preceding { .. } | Bound::Following { .. }) || interval_bound(b)
}

fn interval_bound(b: &Bound) -> bool {
    matches!(b, Bound::IntervalPreceding { .. } | Bound::IntervalFollowing { .. })
}

/// An interval offset in the key's number unit (`scale`: -1 days, 1 microseconds) (Java's `intervalOffset`).
fn interval_offset(n: i64, unit: &str, scale: Option<i64>) -> Result<i64, Refusal> {
    let micros: Option<i64> = match unit.to_uppercase().as_str() {
        "WEEKS" => Some(7 * 86_400_000_000),
        "DAYS" => Some(86_400_000_000),
        "HOURS" => Some(3_600_000_000),
        "MINUTES" => Some(60_000_000),
        "SECONDS" => Some(1_000_000),
        "MILLISECONDS" => Some(1000),
        _ => None,
    };
    let refuse = || Refusal::capability(format!("a {unit} window frame offset over this key reached BigQuery"));
    let (Some(scale), Some(micros)) = (scale, micros) else {
        return Err(refuse());
    };
    if scale < 0 && micros % 86_400_000_000 != 0 {
        return Err(refuse());
    }
    let factor = if scale < 0 { micros / 86_400_000_000 } else { micros };
    n.checked_mul(factor)
        .ok_or_else(|| Refusal { class: "java.lang.ArithmeticException", message: "long overflow".to_owned() })
}

/// The `QUANTILE_CONT` / `QUANTILE_DISC` of a group: its non-NULL values sorted and indexed (Java's `quantile`).
fn quantile<'a>(d: &'a BigQuery, f: AggFn, x: &'a SqlExpr, q: P<'a>, r: &Reducer) -> P<'a> {
    let desc = r.order_by.first().is_some_and(|k| !k.ascending);
    let sorted: P<'a> = pc(move |w| {
        w.append("ARRAY_AGG(").expr(d, x, 0)?.append(" IGNORE NULLS ORDER BY ").expr(d, x, 0)?;
        w.append(if desc { " DESC)" } else { " ASC)" });
        Ok(())
    });
    let n: P<'a> = pc(move |w| {
        w.append("COUNT(").expr(d, x, 0)?.append(")");
        Ok(())
    });
    let qd: P<'a> = pc(move |w| {
        w.append("CAST(").piece(&*q)?.append(" AS FLOAT64)");
        Ok(())
    });
    if f == AggFn::QuantileDisc {
        return pc(move |w| {
            w.piece(&*sorted)?.append("[SAFE_OFFSET(GREATEST(CAST(CEIL(").piece(&*n)?.append(" * ").piece(&*qd)?;
            w.append(") AS INT64) - 1, 0))]");
            Ok(())
        });
    }
    let pos: P<'a> = {
        let (qd, n) = (qd.clone(), n.clone());
        pc(move |w| {
            w.append("(").piece(&*qd)?.append(" * (").piece(&*n)?.append(" - 1))");
            Ok(())
        })
    };
    let at = |round: &'static str| -> P<'a> {
        let (sorted, pos) = (sorted.clone(), pos.clone());
        pc(move |w| {
            w.append("CAST(").piece(&*sorted)?.append(&format!("[SAFE_OFFSET(CAST({round}("));
            w.piece(&*pos)?.append(") AS INT64))] AS FLOAT64)");
            Ok(())
        })
    };
    let (lo, hi) = (at("FLOOR"), at("CEIL"));
    pc(move |w| {
        w.append("(").piece(&*lo)?.append(" + (").piece(&*hi)?.append(" - ").piece(&*lo)?.append(") * (");
        w.piece(&*pos)?.append(" - FLOOR(").piece(&*pos)?.append(")))");
        Ok(())
    })
}

impl BigQuery {
    /// INT64 bit operators; `>>` a LOGICAL shift here, DuckDB's arithmetic (Java's `bitOp`).
    pub(super) fn bit_op_arm(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        let x = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("CAST(").expr(self, arg(a, 0)?, 0)?.append(" AS INT64)");
            Ok(())
        };
        let y = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("CAST(").expr(self, arg(a, 1)?, 0)?.append(" AS INT64)");
            Ok(())
        };
        let op = match f {
            SqlFn::BitAnd => " & ",
            SqlFn::BitOr => " | ",
            SqlFn::BitXor => " ^ ",
            SqlFn::BitShiftLeft => " << ",
            SqlFn::BitShiftRight => {
                w.append("(CASE WHEN ").piece(&x)?.append(" < 0 THEN ~((~").piece(&x)?.append(") >> ").piece(&y)?;
                w.append(") ELSE ").piece(&x)?.append(" >> ").piece(&y)?.append(" END)");
                return Ok(());
            }
            other => return Err(Refusal::illegal_state(format!("not a bit op: {other}"))),
        };
        w.append("(").piece(&x)?.append(op).piece(&y)?.append(")");
        Ok(())
    }

    /// An aggregate's own order key: its declared null placement, else NULLs last (Java's `aggregateKey`).
    fn aggregate_key<'a>(&'a self, k: &'a SortKey) -> P<'a> {
        order_key(self.ex(&k.expr, 0), k.ascending, k.null_order == Some(NullOrder::NullsFirst))
    }

    /// `[DISTINCT ]args` (Java's `aggregateArgs`).
    fn aggregate_args(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        w.append(if r.distinct { "DISTINCT " } else { "" }).list(self, &r.args)?;
        Ok(())
    }

    pub(super) fn reducer_arm(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        let unordered = || -> Result<(), Refusal> {
            if !r.order_by.is_empty() {
                return Err(Refusal::capability(format!("an ordered {} reached BigQuery", r.r#fn)));
            }
            Ok(())
        };
        let plain = |w: &mut SqlWriter, name: &str| -> Result<(), Refusal> {
            unordered()?;
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
                if r.args.len() != 1 || r.distinct {
                    return Err(Refusal::capability("a median of this shape reached BigQuery"));
                }
                let q = quantile(self, AggFn::QuantileCont, &r.args[0], super::text("0.5"), r);
                w.piece(&*q)?;
                Ok(())
            }
            AggFn::Mode => self.mode_arm(w, Mode::Grouped(r)),
            AggFn::StringAgg => self.ordered_aggregate(w, "STRING_AGG", r),
            AggFn::List => {
                if r.args.len() != 1 {
                    return Err(Refusal::capability(format!(
                        "a LIST aggregate of {} arguments reached BigQuery",
                        r.args.len()
                    )));
                }
                if matches!(type_of(&r.args[0]), Some(SqlType::Array { .. })) {
                    return Err(super::nested_list());
                }
                self.ordered_aggregate(w, "ARRAY_AGG", r)
            }
            AggFn::QuantileCont | AggFn::QuantileDisc => {
                if r.args.len() != 2 || r.distinct || r.order_by.len() > 1 {
                    return Err(Refusal::capability(format!("{} of this shape reached BigQuery", r.r#fn)));
                }
                let q = quantile(self, r.r#fn, &r.args[0], self.ex(&r.args[1], 0), r);
                w.piece(&*q)?;
                Ok(())
            }
            AggFn::Corr => plain(w, "CORR"),
            AggFn::CovarSamp => plain(w, "COVAR_SAMP"),
            AggFn::CovarPop => plain(w, "COVAR_POP"),
            // the value at the key's greatest (least) row, rows of a NULL key ignored
            AggFn::ArgMax | AggFn::ArgMin => {
                if r.args.len() != 2 || r.distinct || !r.order_by.is_empty() {
                    return Err(Refusal::capability(format!("{} of this shape reached BigQuery", r.r#fn)));
                }
                let name = if r.r#fn == AggFn::ArgMax { "MAX_BY" } else { "MIN_BY" };
                w.append(name).append("(").list(self, &r.args)?.append(")");
                Ok(())
            }
            AggFn::BoolAnd | AggFn::BoolOr => {
                unordered()?;
                w.append(if r.r#fn == AggFn::BoolAnd { "LOGICAL_AND(" } else { "LOGICAL_OR(" });
                self.aggregate_args(w, r)?;
                w.append(")");
                Ok(())
            }
            AggFn::Variance => plain(w, "VARIANCE"),
            AggFn::Stddev => plain(w, "STDDEV"),
            AggFn::Wavg | AggFn::HashList | AggFn::IsDistinctMark | AggFn::UniqueValueOnly => {
                Err(ansi::lowering_marker(r))
            }
            // a window function is a ranking or value function, never an aggregate
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

    /// `name([DISTINCT ]args[ ORDER BY keys])`: the string and list aggregates (Java's `orderedAggregate`).
    fn ordered_aggregate(&self, w: &mut SqlWriter, name: &str, r: &Reducer) -> Result<(), Refusal> {
        w.append(name).append("(");
        self.aggregate_args(w, r)?;
        if !r.order_by.is_empty() {
            w.append(" ORDER BY ").join(&r.order_by, ", ", |w, k| {
                w.piece(&*self.aggregate_key(k))?;
                Ok(())
            })?;
        }
        w.append(")");
        Ok(())
    }

    /// A windowed average is a DOUBLE; a median or quantile over a whole partition the native analytic
    /// PERCENTILE_CONT/DISC (Java's `windowCall`).
    pub(super) fn window_call_arm(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        match f {
            SqlAgg::Reducer(r)
                if matches!(r.r#fn, AggFn::Median | AggFn::Mode | AggFn::QuantileCont | AggFn::QuantileDisc) =>
            {
                return self.ordered_set_over(w, f, r, partition_by, order_by, frame);
            }
            SqlAgg::Reducer(r) if r.r#fn == AggFn::Avg => {
                w.append("CAST(");
                self.reducer(w, r)?;
                self.bq_over(w, partition_by, order_by, frame)?;
                w.append(" AS FLOAT64)");
                return Ok(());
            }
            SqlAgg::Reducer(r) => self.reducer(w, r)?,
            SqlAgg::RankingFn { r#fn, args } | SqlAgg::ValueFn { r#fn, args } => {
                let name = self.aggregate_name(*r#fn);
                w.append(&name).append("(").list(self, args)?.append(")");
            }
        }
        self.bq_over(w, partition_by, order_by, frame)
    }

    fn ordered_set_over(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        r: &Reducer,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        if r.r#fn == AggFn::Mode {
            return self.mode_arm(w, Mode::Window { r#fn: f, partition_by, order_by, frame });
        }
        if !order_by.is_empty() || frame.is_some() || r.distinct {
            return Err(Refusal::capability(format!(
                "{} over an ordered or framed window reached BigQuery, whose PERCENTILE_* take a partition only",
                r.r#fn
            )));
        }
        if r.order_by.first().is_some_and(|k| !k.ascending) {
            return self.descending_over(w, f, r, partition_by);
        }
        if r.r#fn == AggFn::Median {
            w.append("PERCENTILE_CONT(").expr(self, arg(&r.args, 0)?, 0)?.append(", 0.5)");
        } else {
            w.append(if r.r#fn == AggFn::QuantileCont { "PERCENTILE_CONT(" } else { "PERCENTILE_DISC(" });
            w.expr(self, arg(&r.args, 0)?, 0)?.append(", ").expr(self, arg(&r.args, 1)?, 0)?.append(")");
        }
        self.bq_over(w, partition_by, order_by, frame)
    }

    /// A descending continuous quantile over a window, as the DuckDB lane writes it (Java's `descendingOver`).
    fn descending_over(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        r: &Reducer,
        partition_by: &[SqlExpr],
    ) -> Result<(), Refusal> {
        let x = arg(&r.args, 0)?;
        let t = type_of(x);
        let numeric = matches!(
            t,
            Some(SqlType::Scalar(Scalar::Integer | Scalar::Bigint | Scalar::Double) | SqlType::Decimal { .. })
        );
        if !numeric || r.r#fn != AggFn::QuantileCont {
            return Err(Refusal::capability(format!(
                "a descending {} over a window of {} reached BigQuery",
                r.r#fn,
                type_text(t)
            )));
        }
        let q = arg(&r.args, 1)?;
        let exact_q = |w: &mut SqlWriter| -> Result<(), Refusal> {
            match q {
                SqlExpr::FloatLit { value, .. } => {
                    let text = lite_javacompat::double_to_string(value.value());
                    w.append(&format!("BIGNUMERIC '{}'", crate::java::big_decimal_plain(&text)));
                }
                SqlExpr::DecimalLit { value, .. } => {
                    w.append(&format!("BIGNUMERIC '{}'", crate::java::big_decimal_plain(value.as_str())));
                }
                other => {
                    w.expr(self, other, 0)?;
                }
            }
            Ok(())
        };
        let partition = |w: &mut SqlWriter| -> Result<(), Refusal> {
            if !partition_by.is_empty() {
                w.append("PARTITION BY ").list(self, partition_by)?;
            }
            Ok(())
        };
        let sorted = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("ARRAY_AGG(STRUCT(-(").expr(self, x, 0)?.append(") AS v)) OVER (").piece(&partition)?;
            w.append(if partition_by.is_empty() { "" } else { " " }).append("ORDER BY -(").expr(self, x, 0)?;
            w.append(") ASC NULLS LAST ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)");
            Ok(())
        };
        let pos = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.append("(")
                .piece(&exact_q)?
                .append(" * (COUNT(")
                .expr(self, x, 0)?
                .append(") OVER (")
                .piece(&partition)?;
            w.append(") - 1))");
            Ok(())
        };
        let lo = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.piece(&sorted)?.append("[SAFE_OFFSET(CAST(FLOOR(").piece(&pos)?.append(") AS INT64))].v");
            Ok(())
        };
        let hi = |w: &mut SqlWriter| -> Result<(), Refusal> {
            w.piece(&sorted)?.append("[SAFE_OFFSET(CAST(CEIL(").piece(&pos)?.append(") AS INT64))].v");
            Ok(())
        };
        // the window's own type (Java's `typeOf(w)`): its function's
        let dbl =
            matches!(typing::window_type(f), lite_sql::TypeFact::Typed { r#type: SqlType::Scalar(Scalar::Double), .. });
        w.append(if dbl { "CAST(-(" } else { "(-(" }).piece(&lo)?.append(" + (").piece(&pos)?.append(" - FLOOR(");
        w.piece(&pos)?.append(")) * (").piece(&hi)?.append(" - ").piece(&lo)?.append("))");
        w.append(if dbl { " AS FLOAT64)" } else { ")" });
        Ok(())
    }

    /// A window's `OVER (...)`: its keys' NULLs placed by a leading IS NULL key; a RANGE frame with an offset over
    /// one numeric key, a date or date-time key its day or microsecond number (Java's `bqOver`).
    fn bq_over(
        &self,
        w: &mut SqlWriter,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        w.append(" OVER (");
        let mut any = false;
        if !partition_by.is_empty() {
            w.append("PARTITION BY ").list(self, partition_by)?;
            any = true;
        }
        let mut scale: Option<i64> = None;
        if !order_by.is_empty() {
            w.append(if any { " " } else { "" }).append("ORDER BY ");
            if let Some(f) = frame
                && f.kind == FrameKind::Range
                && (offset(&f.from) || offset(&f.to))
            {
                if order_by.len() != 1 {
                    return Err(Refusal::capability(format!(
                        "a RANGE frame over {} keys reached BigQuery",
                        order_by.len()
                    )));
                }
                let k = &order_by[0];
                let interval = interval_bound(&f.from) || interval_bound(&f.to);
                let kt = type_of(&k.expr);
                let key: P<'_> = if interval && matches!(kt, Some(SqlType::Scalar(Scalar::Date))) {
                    scale = Some(-1);
                    pc(move |w| {
                        w.append("UNIX_DATE(").expr(self, &k.expr, 0)?.append(")");
                        Ok(())
                    })
                } else if interval && matches!(kt, Some(SqlType::Scalar(Scalar::Timestamp))) {
                    scale = Some(1);
                    pc(move |w| {
                        w.append("UNIX_MICROS(TIMESTAMP(").expr(self, &k.expr, 0)?.append("))");
                        Ok(())
                    })
                } else if interval {
                    return Err(Refusal::capability(format!(
                        "an interval RANGE frame over a {} key reached BigQuery",
                        type_text(kt)
                    )));
                } else {
                    self.ex(&k.expr, 0)
                };
                // BigQuery's own placement is ASC NULLS FIRST, DESC NULLS LAST
                let moved = nulls_first(k) == !k.ascending;
                if moved {
                    w.append("IFNULL(CAST(").piece(&*key)?.append(" AS FLOAT64), CAST('inf' AS FLOAT64))");
                } else {
                    w.piece(&*key)?;
                }
                w.append(if k.ascending { " ASC" } else { " DESC" });
            } else {
                w.join(order_by, ", ", |w, k| {
                    w.piece(&*order_key(self.ex(&k.expr, 0), k.ascending, nulls_first(k)))?;
                    Ok(())
                })?;
            }
            any = true;
        }
        if let Some(f) = frame {
            let from = self.frame_bound(&f.from, scale)?;
            let to = self.frame_bound(&f.to, scale)?;
            w.append(if any { " " } else { "" }).append(&format!("{} BETWEEN {from} AND {to}", f.kind.as_str()));
        }
        w.append(")");
        Ok(())
    }

    /// A frame bound; an interval offset in the key's number unit (Java's `frameBound`).
    fn frame_bound(&self, b: &Bound, scale: Option<i64>) -> Result<String, Refusal> {
        match b {
            Bound::IntervalPreceding { n, unit } => Ok(format!("{} PRECEDING", interval_offset(*n, unit, scale)?)),
            Bound::IntervalFollowing { n, unit } => Ok(format!("{} FOLLOWING", interval_offset(*n, unit, scale)?)),
            other => self.bound(other),
        }
    }

    /// GoogleSQL DDL: a temporary table lives in a session, unqualified; a key is declared NOT ENFORCED (Java's
    /// `render(SqlDdl)`).
    pub(super) fn render_ddl_arm(&self, ddl: &SqlDdl) -> Result<String, Refusal> {
        Ok(match ddl {
            SqlDdl::CreateTable { schema, table, quoting, columns, temporary } => {
                let mut sb = String::from(if *temporary { "CREATE TEMP TABLE " } else { "CREATE TABLE " });
                sb.push_str(&self.qualified(schema.as_deref(), table, *quoting));
                sb.push_str(" (");
                let mut keys = Vec::new();
                for (i, col) in columns.iter().enumerate() {
                    sb.push_str(if i > 0 { ", " } else { "" });
                    sb.push_str(&self.ident(&col.name));
                    sb.push(' ');
                    sb.push_str(&ddl_type(&col.r#type)?);
                    sb.push_str(if col.primary_key || col.not_null { " NOT NULL" } else { "" });
                    if col.primary_key {
                        keys.push(self.ident(&col.name));
                    }
                }
                if !keys.is_empty() {
                    sb.push_str(&format!(", PRIMARY KEY ({}) NOT ENFORCED", keys.join(", ")));
                }
                sb.push_str(");");
                sb
            }
            SqlDdl::DropTable { schema, table, quoting } => {
                format!("DROP TABLE IF EXISTS {};", self.qualified(schema.as_deref(), table, *quoting))
            }
            SqlDdl::CreateSchema { schema, quoted } => {
                format!("CREATE SCHEMA IF NOT EXISTS {};", AnsiSqlRenderer::stored_name(self, schema, *quoted))
            }
            SqlDdl::DropSchema { schema, quoted } => {
                format!("DROP SCHEMA IF EXISTS {} CASCADE;", AnsiSqlRenderer::stored_name(self, schema, *quoted))
            }
        })
    }
}

/// The format for PARSE_DATETIME and any format literal (Java's `formatText`).
pub(super) fn format_text(parts: &[DateFmt]) -> Result<String, Refusal> {
    super::functions::format_codes(parts)
}

/// GoogleSQL's column types (Java's `ddlType`).
pub(super) fn ddl_type(t: &ColumnType) -> Result<String, Refusal> {
    Ok(match t {
        ColumnType::Plain { kind } => match kind {
            ColumnKind::Bigint | ColumnKind::Smallint | ColumnKind::Tinyint | ColumnKind::Integer => "INT64",
            ColumnKind::Float | ColumnKind::Double | ColumnKind::Real => "FLOAT64",
            ColumnKind::Bit => "BOOL",
            ColumnKind::Timestamp => "DATETIME",
            ColumnKind::Date => "DATE",
            ColumnKind::Time => "TIME",
            ColumnKind::Json => "JSON",
            ColumnKind::Varchar => "STRING",
            // a nanosecond timestamp column: BigQuery's DATETIME keeps microseconds
            ColumnKind::TimestampNs
            | ColumnKind::Other
            | ColumnKind::Distinct
            | ColumnKind::Array
            | ColumnKind::Object => {
                return Err(Refusal::capability(format!("a {kind} column reached BigQuery's DDL")));
            }
        }
        .to_owned(),
        ColumnType::Sized { kind, .. } => {
            if kind.to_uppercase().contains("BINARY") { "BYTES" } else { "STRING" }.to_owned()
        }
        ColumnType::Scaled { precision, scale, .. } => {
            if *scale <= 9 && precision - scale <= 29 {
                format!("NUMERIC({precision}, {scale})")
            } else {
                format!("BIGNUMERIC({precision}, {scale})")
            }
        }
    })
}
