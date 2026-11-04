// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `MongoDb` aggregates: the `$group` stage's accumulators and their reads (`reducer`, `accumulator`, the
//! pushed route reduced at the read), and a window function's `$setWindowFields` output and its read
//! (`windowOutput`). A read is a piece the stage registers and a later clause writes: it owns what it writes.

use super::json;
use super::{MongoDb, Pc, THIS, VALUE, java_text, of, pc, text};
use crate::ansi;
use crate::refusal::{Refusal, arg};
use crate::writer::SqlWriter;
use lite_sql::{AggFn, NullOrder, Reducer, SortKey, SqlAgg, SqlExpr};

/// The aggregates with no native accumulator, or whose exact answer needs the group's values: pushed into an array by
/// the group stage and reduced at the read.
fn pushed_fn(f: AggFn) -> bool {
    matches!(
        f,
        AggFn::VarSamp
            | AggFn::VarPop
            | AggFn::Variance
            | AggFn::Median
            | AggFn::StringAgg
            | AggFn::QuantileCont
            | AggFn::QuantileDisc
            | AggFn::Mode
            | AggFn::Corr
            | AggFn::CovarSamp
            | AggFn::CovarPop
            | AggFn::ArgMax
            | AggFn::ArgMin
    )
}

/// The aggregates over two columns, pushed as pairs (`{"x": .., "y": ..}`).
fn bivariate(f: AggFn) -> bool {
    matches!(f, AggFn::Corr | AggFn::CovarSamp | AggFn::CovarPop | AggFn::ArgMax | AggFn::ArgMin)
}

/// Whether the aggregate is pushed and reduced at the read, rather than accumulated natively.
fn push_route(r: &Reducer) -> bool {
    r.distinct || !r.order_by.is_empty() || pushed_fn(r.r#fn)
}

/// A group stage's field for an aggregate (`__agg0`); a dynamic pivot's per-key aggregate's field carries the key's
/// position (`__agg0_pk3`), written by the runner in each copy of the per-key region (Java's `GroupField`).
#[derive(Debug, Clone)]
pub struct GroupField {
    stem: String,
    per_key: bool,
}

impl GroupField {
    /// Java's constructor: the stem is a field name.
    pub fn new(stem: &str, per_key: bool) -> Result<GroupField, Refusal> {
        json::key(stem)?;
        Ok(GroupField { stem: stem.to_owned(), per_key })
    }

    /// The field's name, quoted, with `suffix`.
    pub fn name<'w>(&self, w: &'w mut SqlWriter, suffix: &str) -> Result<&'w mut SqlWriter, Refusal> {
        w.append(&format!("\"{}", self.stem));
        if self.per_key {
            w.append("_pk").counter()?;
        }
        w.append(&format!("{suffix}\""));
        Ok(w)
    }

    /// The read of the field (with `suffix`) as a path.
    pub fn path(&self, suffix: &str) -> Pc<'static> {
        let head = format!("\"${}", self.stem);
        let tail = format!("{suffix}\"");
        let per_key = self.per_key;
        pc(move |_, x| {
            x.append(&head);
            if per_key {
                x.append("_pk").counter()?;
            }
            x.append(&tail);
            Ok(())
        })
    }
}

/// The native accumulator for an aggregate, one argument, unordered and not distinct; every other form is pushed and
/// reduced at the read (Java's `reducer` override).
pub fn reducer(d: &MongoDb, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
    match r.r#fn {
        AggFn::Sum => accumulate(d, w, "$sum", r),
        AggFn::Count => count_accumulator(d, w, r),
        AggFn::Avg => accumulate(d, w, "$avg", r),
        AggFn::Min => accumulate(d, w, "$min", r),
        AggFn::Max => accumulate(d, w, "$max", r),
        AggFn::AnyValue => accumulate(d, w, "$first", r),
        AggFn::StddevSamp | AggFn::Stddev => accumulate(d, w, "$stdDevSamp", r),
        AggFn::StddevPop => accumulate(d, w, "$stdDevPop", r),
        AggFn::List => accumulate(d, w, "$push", r),
        AggFn::BoolAnd => accumulate(d, w, "$min", r),
        AggFn::BoolOr => accumulate(d, w, "$max", r),
        AggFn::VarSamp
        | AggFn::VarPop
        | AggFn::Variance
        | AggFn::Median
        | AggFn::StringAgg
        | AggFn::QuantileCont
        | AggFn::QuantileDisc
        | AggFn::Mode
        | AggFn::Corr
        | AggFn::CovarSamp
        | AggFn::CovarPop
        | AggFn::ArgMax
        | AggFn::ArgMin => pushed(d, w, r),
        AggFn::Wavg | AggFn::HashList | AggFn::IsDistinctMark | AggFn::UniqueValueOnly => Err(ansi::lowering_marker(r)),
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

/// `{"$op": <arg>}`.
fn accumulate(d: &MongoDb, w: &mut SqlWriter, op: &str, r: &Reducer) -> Result<(), Refusal> {
    let Some(a0) = r.args.first() else {
        return Err(Refusal::illegal_state(format!("{} accumulates no argument", r.r#fn)));
    };
    w.append("{\"").append(op).append("\": ").expr(d, a0, 0)?.append("}");
    Ok(())
}

/// `count(*)` counts rows; `count(x)` the non-null values.
fn count_accumulator(d: &MongoDb, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
    let Some(a0) = r.args.first() else {
        w.append("{\"$sum\": 1}");
        return Ok(());
    };
    w.append("{\"$sum\": {\"$cond\": [{\"$eq\": [").expr(d, a0, 0)?.append(", null]}, 0, 1]}}");
    Ok(())
}

/// The group's values pushed, each with its order keys when the aggregate is ordered.
fn pushed(d: &MongoDb, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
    w.append("{\"$push\": ");
    pushed_value(d, w, r)?;
    w.append("}");
    Ok(())
}

/// What an aggregate on the push route pushes: its value -- a two-column statistic's pair -- with its order keys
/// when the aggregate is ordered.
fn pushed_value(d: &MongoDb, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
    if r.args.is_empty() {
        return Err(Refusal::illegal_state(format!("{} accumulates no argument", r.r#fn)));
    }
    let value: Pc<'_> = if bivariate(r.r#fn) {
        pc(move |d, x| {
            x.append("{\"x\": ").expr(d, &r.args[0], 0)?.append(", \"y\": ").expr(d, arg(&r.args, 1)?, 0)?;
            x.append("}");
            Ok(())
        })
    } else {
        of(&r.args[0])
    };
    if r.order_by.is_empty() {
        return value(d, w);
    }
    let keys: Vec<&SqlExpr> = r.order_by.iter().map(|k| &k.expr).collect();
    pushed_with_keys(d, w, &value, &keys)
}

/// `{"v": value, "k0": key, "n0": <key is null>, ...}`: a pushed value with its order keys and each key's null flag,
/// so the read's sort places nulls as the SQL engine does.
fn pushed_with_keys(d: &MongoDb, w: &mut SqlWriter, value: &Pc<'_>, keys: &[&SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"v\": ");
    value(d, w)?;
    for (i, k) in keys.iter().enumerate() {
        w.append(", \"k").append_long(i as i64).append("\": ").expr(d, k, 0)?.append(", \"n").append_long(i as i64);
        w.append("\": {\"$eq\": [").expr(d, k, 0)?.append(", null]}");
    }
    w.append("}");
    Ok(())
}

/// The read of a pushed aggregate: its values in their keys' order, the nulls dropped (a pair with a null side dropped
/// whole), made distinct where the aggregate asks, and reduced; a quantile over the value's own descending order where
/// the reducer orders it so.
fn pushed_read(r: Reducer, path: Pc<'static>) -> Pc<'static> {
    let extras: Vec<SqlExpr> = if bivariate(r.r#fn) { Vec::new() } else { r.args[1..].to_vec() };
    let values = pushed_values(path, r.order_by.clone());
    let present: Pc<'static> = if bivariate(r.r#fn) {
        pc(move |d, x| non_null_pairs(d, x, &values))
    } else {
        pc(move |d, x| non_null(d, x, &values))
    };
    let set: Pc<'static> = if r.distinct { pc(move |d, x| distinct_in_order(d, x, &present)) } else { present };
    let descending = !r.order_by.is_empty() && r.order_by.iter().all(|k| !k.ascending);
    let f = r.r#fn;
    pc(move |d, x| array_reduction(d, x, f, &set, &extras, descending))
}

/// An aggregate's accumulator, written as the group field's value, and the read of that field the projection writes in
/// the marker's place (Java's `accumulator`).
pub fn accumulator(d: &MongoDb, w: &mut SqlWriter, node: &SqlExpr, field: &GroupField) -> Result<Pc<'static>, Refusal> {
    let path = field.path("");
    match node {
        SqlExpr::Reducer(r) => {
            if !push_route(r) {
                reducer(d, w, r)?;
                if r.r#fn == AggFn::Sum {
                    // SQL's SUM over no non-null value is null, and $sum answers 0: a count of the non-null values
                    // beside the sum decides at the read
                    field.name(w.append(", "), "_n")?.append(": {\"$sum\": {\"$cond\": [{\"$eq\": [");
                    w.expr(d, arg(&r.args, 0)?, 0)?.append(", null]}, 0, 1]}}");
                    let count = field.path("_n");
                    return Ok(pc(move |d, x| {
                        x.append("{\"$cond\": [{\"$gt\": [");
                        count(d, x)?;
                        x.append(", 0]}, ");
                        path(d, x)?;
                        x.append(", null]}");
                        Ok(())
                    }));
                }
                return Ok(path);
            }
            pushed(d, w, r)?;
            Ok(pushed_read(r.clone(), path))
        }
        SqlExpr::OrderedListAgg { value, order_by, .. } => {
            w.append("{\"$push\": ");
            pushed_with_keys(d, w, &of(value), &[order_by])?;
            w.append("}");
            let keys =
                vec![SortKey { expr: order_by.as_ref().clone(), ascending: true, null_order: None, output_name: None }];
            let values = pushed_values(path, keys);
            Ok(pc(move |d, x| array_reduction(d, x, AggFn::List, &values, &[], false)))
        }
        SqlExpr::JsonArrayAgg { value, order_keys, .. } => {
            w.append("{\"$push\": ");
            if order_keys.is_empty() {
                w.expr(d, value, 0)?;
            } else {
                let keys: Vec<&SqlExpr> = order_keys.iter().map(|k| &k.expr).collect();
                pushed_with_keys(d, w, &of(value), &keys)?;
            }
            w.append("}");
            let keys: Vec<SortKey> = order_keys
                .iter()
                .map(|k| SortKey { expr: k.expr.clone(), ascending: !k.desc, null_order: None, output_name: None })
                .collect();
            let values = pushed_values(path, keys);
            Ok(pc(move |d, x| {
                x.append("{\"$ifNull\": [");
                values(d, x)?;
                x.append(", []]}");
                Ok(())
            }))
        }
        other => Err(Refusal::illegal_state(format!("not an aggregate: {}", java_text::expr(other)))),
    }
}

/// The value the group's field holds when no row reached it: SQL's one row over no input.
pub fn empty_input(node: &SqlExpr) -> Result<&'static str, Refusal> {
    match node {
        SqlExpr::Reducer(r) => Ok(if push_route(r) {
            "[]"
        } else if r.r#fn == AggFn::Count {
            "0"
        } else {
            "null"
        }),
        SqlExpr::OrderedListAgg { .. } | SqlExpr::JsonArrayAgg { .. } => Ok("[]"),
        other => Err(Refusal::illegal_state(format!("not an aggregate: {}", java_text::expr(other)))),
    }
}

/// The pushed values in their keys' order: the array sorted by the keys, each element's value read.
fn pushed_values(path: Pc<'static>, keys: Vec<SortKey>) -> Pc<'static> {
    if keys.is_empty() {
        return path;
    }
    // each key's null flag before the key: SQL's placement, which $sortArray's own order (null first) is not
    let mut by = String::from("{");
    for (i, k) in keys.iter().enumerate() {
        let nulls_first = match k.null_order {
            None => !k.ascending,
            Some(o) => o == NullOrder::NullsFirst,
        };
        by.push_str(if i > 0 { ", " } else { "" });
        by.push_str(&format!(
            "\"n{i}\": {}, \"k{i}\": {}",
            if nulls_first { -1 } else { 1 },
            if k.ascending { 1 } else { -1 }
        ));
    }
    by.push('}');
    pc(move |d, x| {
        x.append("{\"$map\": {\"input\": {\"$sortArray\": {\"input\": ");
        path(d, x)?;
        x.append(&format!(", \"sortBy\": {by}}}}}, \"in\": \"$$this.v\"}}}}"));
        Ok(())
    })
}

/// The array without its nulls.
pub fn non_null(d: &MongoDb, w: &mut SqlWriter, array: &Pc<'_>) -> Result<(), Refusal> {
    w.append("{\"$filter\": {\"input\": {\"$ifNull\": [");
    array(d, w)?;
    w.append(&[", []]}, \"cond\": {\"$ne\": [", THIS, ", null]}}}"].concat());
    Ok(())
}

/// The array of pairs without those a null side.
fn non_null_pairs(d: &MongoDb, w: &mut SqlWriter, array: &Pc<'_>) -> Result<(), Refusal> {
    w.append("{\"$filter\": {\"input\": {\"$ifNull\": [");
    array(d, w)?;
    w.append(", []]}, \"cond\": {\"$and\": [{\"$ne\": [\"$$this.x\", null]}, {\"$ne\": [\"$$this.y\", null]}]}}}");
    Ok(())
}

/// The array's distinct elements, the first occurrence of each kept in place.
fn distinct_in_order(d: &MongoDb, w: &mut SqlWriter, array: &Pc<'_>) -> Result<(), Refusal> {
    w.append("{\"$reduce\": {\"input\": ");
    array(d, w)?;
    w.append(
        &[
            ", \"initialValue\": [], \"in\":",
            " {\"$cond\": [{\"$in\": [",
            THIS,
            ", ",
            VALUE,
            "]}, ",
            VALUE,
            ", {\"$concatArrays\": [",
            VALUE,
            ", [",
            THIS,
            "]]}]}}}",
        ]
        .concat(),
    );
    Ok(())
}

/// An aggregate over an array of non-null values: its count, or SQL's null over no value, else its reduction.
pub fn array_reduction(
    d: &MongoDb,
    w: &mut SqlWriter,
    f: AggFn,
    array: &Pc<'_>,
    extras: &[SqlExpr],
    descending: bool,
) -> Result<(), Refusal> {
    w.append("{\"$let\": {\"vars\": {\"a\": ");
    array(d, w)?;
    w.append("}, \"in\": ");
    if f == AggFn::Count {
        w.append("{\"$size\": \"$$a\"}}}");
        return Ok(());
    }
    w.append("{\"$cond\": [{\"$eq\": [{\"$size\": \"$$a\"}, 0]}, null, ");
    reduce_array(d, w, f, extras, descending)?;
    w.append("]}}}");
    Ok(())
}

/// The reduction of the bound array `$$a`, by aggregate; a quantile or a median over the values sorted the way the
/// reducer orders them.
pub(super) fn reduce_array(
    d: &MongoDb,
    w: &mut SqlWriter,
    f: AggFn,
    extras: &[SqlExpr],
    descending: bool,
) -> Result<(), Refusal> {
    let sorted = format!("{{\"$sortArray\": {{\"input\": \"$$a\", \"sortBy\": {}}}}}", if descending { -1 } else { 1 });
    // the pairs' sides and their products, for the two-column statistics
    let xs = "{\"$map\": {\"input\": \"$$a\", \"in\": \"$$this.x\"}}";
    let ys = "{\"$map\": {\"input\": \"$$a\", \"in\": \"$$this.y\"}}";
    let xys = "{\"$map\": {\"input\": \"$$a\", \"in\": {\"$multiply\": [\"$$this.x\", \"$$this.y\"]}}}";
    // the population covariance: E[xy] - E[x]E[y]
    let covar_pop =
        ["{\"$subtract\": [{\"$avg\": ", xys, "}, {\"$multiply\": [{\"$avg\": ", xs, "},", " {\"$avg\": ", ys, "}]}]}"]
            .concat();
    match f {
        AggFn::Sum => {
            w.append("{\"$sum\": \"$$a\"}");
        }
        AggFn::Avg => {
            w.append("{\"$avg\": \"$$a\"}");
        }
        AggFn::Min => {
            w.append("{\"$min\": \"$$a\"}");
        }
        AggFn::Max => {
            w.append("{\"$max\": \"$$a\"}");
        }
        AggFn::AnyValue => {
            w.append("{\"$arrayElemAt\": [\"$$a\", 0]}");
        }
        AggFn::StddevSamp | AggFn::Stddev => {
            w.append("{\"$stdDevSamp\": \"$$a\"}");
        }
        AggFn::StddevPop => {
            w.append("{\"$stdDevPop\": \"$$a\"}");
        }
        AggFn::VarSamp | AggFn::Variance => {
            w.append("{\"$pow\": [{\"$stdDevSamp\": \"$$a\"}, 2]}");
        }
        AggFn::VarPop => {
            w.append("{\"$pow\": [{\"$stdDevPop\": \"$$a\"}, 2]}");
        }
        AggFn::List => {
            w.append("\"$$a\"");
        }
        AggFn::BoolAnd => {
            w.append("{\"$allElementsTrue\": [\"$$a\"]}");
        }
        AggFn::BoolOr => {
            w.append("{\"$anyElementTrue\": [\"$$a\"]}");
        }
        AggFn::Median => {
            w.append(
                &[
                    "{\"$let\": {\"vars\": {\"s\": ",
                    &sorted,
                    ", \"n\": {\"$size\": \"$$a\"}},",
                    " \"in\": {\"$cond\": [{\"$eq\": [{\"$mod\": [\"$$n\", 2]}, 1]}, {\"$arrayElemAt\": [\"$$s\",",
                    " {\"$toInt\": {\"$divide\": [{\"$subtract\": [\"$$n\", 1]}, 2]}}]}, {\"$avg\":",
                    " [{\"$arrayElemAt\": [\"$$s\", {\"$toInt\": {\"$subtract\": [{\"$divide\": [\"$$n\", 2]},",
                    " 1]}}]}, {\"$arrayElemAt\": [\"$$s\", {\"$toInt\": {\"$divide\": [\"$$n\", 2]}}]}]}]}}}",
                ]
                .concat(),
            );
        }
        AggFn::StringAgg => {
            w.append(
                &[
                    "{\"$reduce\": {\"input\": \"$$a\", \"initialValue\": null, \"in\":",
                    " {\"$cond\": [{\"$eq\": [",
                    VALUE,
                    ", null]}, {\"$toString\": ",
                    THIS,
                    "},",
                    " {\"$concat\": [",
                    VALUE,
                    ", ",
                ]
                .concat(),
            );
            match extras.first() {
                None => {
                    w.append("\"\"");
                }
                Some(s) => {
                    w.expr(d, s, 0)?;
                }
            }
            w.append(&[", {\"$toString\": ", THIS, "}]}]}}}"].concat());
        }
        AggFn::QuantileCont => {
            w.append(&["{\"$let\": {\"vars\": {\"s\": ", &sorted, ", \"p\": {\"$multiply\":", " ["].concat());
            quantile(d, w, extras)?;
            w.append(concat!(
                ", {\"$subtract\": [{\"$size\": \"$$a\"}, 1]}]}},",
                " \"in\": {\"$let\": {\"vars\": {\"lo\": {\"$arrayElemAt\": [\"$$s\", {\"$toInt\": {\"$floor\":",
                " \"$$p\"}}]}, \"hi\": {\"$arrayElemAt\": [\"$$s\", {\"$toInt\": {\"$ceil\": \"$$p\"}}]}},",
                " \"in\": {\"$add\": [\"$$lo\", {\"$multiply\": [{\"$subtract\": [\"$$p\", {\"$floor\":",
                " \"$$p\"}]}, {\"$subtract\": [\"$$hi\", \"$$lo\"]}]}]}}}}}"
            ));
        }
        AggFn::QuantileDisc => {
            w.append(
                &[
                    "{\"$arrayElemAt\": [",
                    &sorted,
                    ", {\"$max\": [0, {\"$subtract\":",
                    " [{\"$toInt\": {\"$ceil\": {\"$multiply\": [",
                ]
                .concat(),
            );
            quantile(d, w, extras)?;
            w.append(", {\"$size\": \"$$a\"}]}}}, 1]}]}]}");
        }
        AggFn::Count => {
            w.append("{\"$size\": \"$$a\"}");
        }
        // the most frequent value, the smallest among equally frequent ones: the sorted values' runs counted
        AggFn::Mode => {
            w.append(
                &[
                    "{\"$let\": {\"vars\": {\"m\": {\"$reduce\": {\"input\": ",
                    &sorted,
                    ", \"initialValue\": {\"v\": null, \"n\": 0, \"p\": null, \"c\": 0}, \"in\": {\"$let\":",
                    " {\"vars\": {\"c\": {\"$cond\": [{\"$eq\": [",
                    THIS,
                    ", \"$$value.p\"]}, {\"$add\":",
                    " [\"$$value.c\", 1]}, 1]}}, \"in\": {\"$cond\": [{\"$gt\": [\"$$c\", \"$$value.n\"]}, {\"v\": ",
                    THIS,
                    ", \"n\": \"$$c\", \"p\": ",
                    THIS,
                    ", \"c\": \"$$c\"}, {\"v\": \"$$value.v\", \"n\":",
                    " \"$$value.n\", \"p\": ",
                    THIS,
                    ", \"c\": \"$$c\"}]}}}}}}, \"in\": \"$$m.v\"}}",
                ]
                .concat(),
            );
        }
        AggFn::CovarPop => {
            w.append(&covar_pop);
        }
        // the sample covariance scales the population's by n/(n-1); null over one pair, as SQL answers
        AggFn::CovarSamp => {
            w.append(
                &[
                    "{\"$cond\": [{\"$lte\": [{\"$size\": \"$$a\"}, 1]}, null, {\"$multiply\":",
                    " [",
                    &covar_pop,
                    ", {\"$divide\": [{\"$size\": \"$$a\"}, {\"$subtract\": [{\"$size\": \"$$a\"},",
                    " 1]}]}]}]}",
                ]
                .concat(),
            );
        }
        // the correlation: the population covariance over the product of the population deviations
        AggFn::Corr => {
            w.append(
                &[
                    "{\"$let\": {\"vars\": {\"sx\": {\"$stdDevPop\": ",
                    xs,
                    "}, \"sy\":",
                    " {\"$stdDevPop\": ",
                    ys,
                    "}}, \"in\": {\"$cond\": [{\"$or\": [{\"$eq\": [\"$$sx\", 0]},",
                    " {\"$eq\": [\"$$sy\", 0]}]}, null, {\"$divide\": [",
                    &covar_pop,
                    ", {\"$multiply\": [\"$$sx\",",
                    " \"$$sy\"]}]}]}}}",
                ]
                .concat(),
            );
        }
        // the value (x) of the pair with the greatest or the least key (y)
        AggFn::ArgMax => {
            w.append(concat!(
                "{\"$let\": {\"vars\": {\"p\": {\"$arrayElemAt\": [{\"$sortArray\": {\"input\":",
                " \"$$a\", \"sortBy\": {\"y\": -1}}}, 0]}}, \"in\": \"$$p.x\"}}"
            ));
        }
        AggFn::ArgMin => {
            w.append(concat!(
                "{\"$let\": {\"vars\": {\"p\": {\"$arrayElemAt\": [{\"$sortArray\": {\"input\":",
                " \"$$a\", \"sortBy\": {\"y\": 1}}}, 0]}}, \"in\": \"$$p.x\"}}"
            ));
        }
        AggFn::Wavg | AggFn::HashList | AggFn::IsDistinctMark | AggFn::UniqueValueOnly => {
            return Err(Refusal::illegal_state(format!("lowering marker {f} reached the renderer")));
        }
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
        | AggFn::NthValue => {
            return Err(Refusal::illegal_state(format!("window-only function {f} in a reduction")));
        }
    }
    Ok(())
}

fn quantile(d: &MongoDb, w: &mut SqlWriter, extras: &[SqlExpr]) -> Result<(), Refusal> {
    let Some(q) = extras.first() else {
        return Err(Refusal::illegal_state("a quantile without its fraction"));
    };
    w.expr(d, q, 0)?;
    Ok(())
}

// ==================================================================
// Windows: a `$setWindowFields` stage's outputs
// ==================================================================

/// A window function's outputs of a `$setWindowFields` stage, and the read of its field (Java's `windowOutput`):
/// `window` writes `, "window": {...}` or nothing.
pub fn window_output(
    d: &MongoDb,
    w: &mut SqlWriter,
    call: &SqlAgg,
    field: &str,
    window: &Pc<'_>,
) -> Result<Pc<'static>, Refusal> {
    let path = json::string(&format!("${field}"));
    let key = json::string(field);
    match call {
        SqlAgg::Reducer(r) => {
            if !push_route(r) {
                w.append(&key).append(": ");
                reducer_open(d, w, r)?;
                window(d, w)?;
                w.append("}");
                if r.r#fn == AggFn::Sum {
                    let count = format!("{field}_n");
                    w.append(", ").append(&json::string(&count)).append(": {\"$sum\": {\"$cond\": [{\"$eq\": [");
                    w.expr(d, arg(&r.args, 0)?, 0)?.append(", null]}, 0, 1]}");
                    window(d, w)?;
                    w.append("}");
                    return Ok(text(
                        ["{\"$cond\": [{\"$gt\": [", &json::string(&format!("${count}")), ", 0]}, ", &path, ", null]}"]
                            .concat(),
                    ));
                }
                return Ok(text(path));
            }
            w.append(&key).append(": {\"$push\": ");
            pushed_value(d, w, r)?;
            window(d, w)?;
            w.append("}");
            Ok(pushed_read(r.clone(), text(path)))
        }
        SqlAgg::RankingFn { r#fn, args } => match r#fn {
            AggFn::RowNumber => {
                w.append(&key).append(": {\"$documentNumber\": {}}");
                Ok(text(path))
            }
            AggFn::Rank => {
                w.append(&key).append(": {\"$rank\": {}}");
                Ok(text(path))
            }
            AggFn::DenseRank => {
                w.append(&key).append(": {\"$denseRank\": {}}");
                Ok(text(path))
            }
            AggFn::PercentRank => {
                // (rank - 1) / (count - 1), 0 over one row
                let n = json::string(&format!("${field}_n"));
                w.append(&key).append(": {\"$rank\": {}}, ").append(&json::string(&format!("{field}_n")));
                w.append(": {\"$count\": {}}");
                Ok(text(
                    [
                        "{\"$cond\": [{\"$lte\": [",
                        &n,
                        ", 1]}, 0.0, {\"$divide\": [{\"$subtract\": [",
                        &path,
                        ", 1]}, {\"$subtract\": [",
                        &n,
                        ", 1]}]}]}",
                    ]
                    .concat(),
                ))
            }
            AggFn::CumeDist => {
                // the rows up to and including the current row's peers, over the partition's count
                let n = json::string(&format!("${field}_n"));
                w.append(&key).append(": {\"$count\": {}");
                window(d, w)?;
                w.append("}, ").append(&json::string(&format!("{field}_n"))).append(": {\"$count\": {}}");
                Ok(text(["{\"$divide\": [", &path, ", ", &n, "]}"].concat()))
            }
            AggFn::Ntile => {
                // SQL's buckets: floor((row - 1) * k / count) + 1
                let n = json::string(&format!("${field}_n"));
                w.append(&key).append(": {\"$documentNumber\": {}}, ").append(&json::string(&format!("{field}_n")));
                w.append(": {\"$count\": {}}");
                let k = arg(args, 0)?.clone();
                let head =
                    ["{\"$add\": [{\"$floor\": {\"$divide\": [{\"$multiply\": [{\"$subtract\": [", &path, ", 1]}, "]
                        .concat();
                let tail = ["]}, ", &n, "]}}, 1]}"].concat();
                Ok(pc(move |d, x| {
                    x.append(&head).expr(d, &k, 0)?.append(&tail);
                    Ok(())
                }))
            }
            other => Err(Refusal::capability(format!("the window function {other} reached MongoDb: not shaped yet"))),
        },
        SqlAgg::ValueFn { r#fn, args } => match r#fn {
            AggFn::Lag | AggFn::Lead => {
                // $shift: the value by rows before (lag) or after (lead) the current one, else the default
                if args.len() > 1 && !matches!(args[1], SqlExpr::IntLit { .. }) {
                    return Err(Refusal::capability(format!(
                        "a {fn} by a computed offset reached MongoDb, whose $shift takes a literal",
                        fn = r#fn
                    )));
                }
                let by = match args.get(1) {
                    Some(SqlExpr::IntLit { value, .. }) if args.len() > 1 => *value,
                    _ => 1,
                };
                w.append(&key).append(": {\"$shift\": {\"output\": ").expr(d, arg(args, 0)?, 0)?;
                w.append(", \"by\": ").append_long(if *r#fn == AggFn::Lag { by.wrapping_neg() } else { by });
                w.append(", \"default\": ");
                if args.len() > 2 {
                    w.expr(d, &args[2], 0)?;
                } else {
                    w.append("null");
                }
                w.append("}}");
                Ok(text(path))
            }
            AggFn::FirstValue => {
                w.append(&key).append(": {\"$first\": ").expr(d, arg(args, 0)?, 0)?;
                window(d, w)?;
                w.append("}");
                Ok(text(path))
            }
            AggFn::LastValue => {
                w.append(&key).append(": {\"$last\": ").expr(d, arg(args, 0)?, 0)?;
                window(d, w)?;
                w.append("}");
                Ok(text(path))
            }
            AggFn::NthValue => {
                w.append(&key).append(": {\"$push\": ").expr(d, arg(args, 0)?, 0)?;
                window(d, w)?;
                w.append("}");
                let n = arg(args, 1)?.clone();
                let head = ["{\"$arrayElemAt\": [", &path, ", {\"$subtract\": ["].concat();
                Ok(pc(move |d, x| {
                    x.append(&head).expr(d, &n, 0)?.append(", 1]}]}");
                    Ok(())
                }))
            }
            other => Err(Refusal::capability(format!("the window function {other} reached MongoDb: not shaped yet"))),
        },
    }
}

/// A native reducer's window operator, opened: `{"$sum": <arg>` without its closing brace, so the stage writes the
/// window inside it.
fn reducer_open(d: &MongoDb, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
    let op = match r.r#fn {
        AggFn::Sum => "$sum",
        AggFn::Avg => "$avg",
        AggFn::Min => "$min",
        AggFn::Max => "$max",
        AggFn::Count => "$count",
        AggFn::StddevSamp | AggFn::Stddev => "$stdDevSamp",
        AggFn::StddevPop => "$stdDevPop",
        AggFn::AnyValue => "$first",
        other => {
            return Err(Refusal::capability(format!("the window aggregate {other} reached MongoDb: not shaped yet")));
        }
    };
    if r.r#fn == AggFn::Count {
        match r.args.first() {
            None => {
                w.append("{\"$count\": {}");
            }
            Some(a0) => {
                // COUNT(x): the non-null values
                w.append("{\"$sum\": {\"$cond\": [{\"$eq\": [").expr(d, a0, 0)?.append(", null]}, 0, 1]}");
            }
        }
        return Ok(());
    }
    w.append("{\"").append(op).append("\": ").expr(d, arg(&r.args, 0)?, 0)?;
    Ok(())
}
