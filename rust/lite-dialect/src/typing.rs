// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `SqlTyping` (`core/src/main/java/com/legend/sql/SqlTyping.java`): the per-node typing rules the MIR's
//! constructors call -- a node's type fact is a function of its children's stored facts, computed once where the
//! node is built -- and the label reconciliation `SqlSelect`'s and `SqlUnion`'s constructors apply. The passes build
//! and rebuild nodes through [`crate::build`], which calls these, so a rewritten tree carries the facts Java's would.
//!
//! Also here, as the rules read them: `SqlExpr.children()` (the one structural-recursion contract), `SqlQuery`'s and
//! `SqlSource`'s `outputs()` and `alias()`, and `SqlAgg.aggregatesSelect`.
//!
//! Not ported: `taggedType` and `taggedListType` (the Rust MIR has no `Tagged`/`TaggedList`: MirJson refuses them),
//! `subsumes` and `whereNullRejections` (no caller in the dialects).

use crate::refusal::Refusal;
use lite_sql::{
    AggFn, BigDecimal, JoinKind, Lambda, OutputCol, Reducer, Scalar, SqlAgg, SqlExpr, SqlFn, SqlQuery, SqlSelect,
    SqlSource, SqlType, StructField, StructLitField, TypeFact, When,
};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// `SqlTyping.typed(t)`: a non-null fact of `t`.
pub fn typed(t: SqlType) -> TypeFact {
    TypeFact::Typed { r#type: t, nullable: false }
}

/// A non-null fact of a scalar (`T_BOOLEAN`, `T_VARCHAR`, ...).
pub fn scalar(s: Scalar) -> TypeFact {
    typed(SqlType::Scalar(s))
}

/// `SqlTyping.nullable(f)`: a Typed fact marked may-be-null; Bottom, Raises and Unknown pass through.
pub fn nullable(f: TypeFact) -> TypeFact {
    match f {
        TypeFact::Typed { r#type, nullable: false } => TypeFact::Typed { r#type, nullable: true },
        other => other,
    }
}

/// `SqlTyping.mayBeNull(e)`: can this operand deliver SQL NULL? The non-value ride-alongs (a lambda, a format or a
/// date part) and a literal composite contribute nothing.
pub fn may_be_null(e: &SqlExpr) -> bool {
    if matches!(
        e,
        SqlExpr::Lambda(_)
            | SqlExpr::FormatLit { .. }
            | SqlExpr::DatePartLit { .. }
            | SqlExpr::ArrayLit { .. }
            | SqlExpr::StructLit { .. }
    ) {
        return false;
    }
    fact_may_be_null(e.r#type())
}

fn fact_may_be_null(f: &TypeFact) -> bool {
    match f {
        TypeFact::Typed { nullable, .. } => *nullable,
        TypeFact::Raises => false,
        TypeFact::Bottom | TypeFact::Unknown => true,
    }
}

/// `SqlTyping.elementsMayBeNull`: can this collection's elements be SQL NULL?
fn elements_may_be_null(coll: &SqlExpr) -> bool {
    match coll {
        SqlExpr::CompactList { .. } => false,
        SqlExpr::ArrayLit { elements, .. } => any_nullable(elements),
        SqlExpr::Cast { value, .. } => elements_may_be_null(value),
        SqlExpr::Group { inner, .. } => elements_may_be_null(inner),
        SqlExpr::Call { r#fn, .. } => !matches!(r#fn, SqlFn::Split | SqlFn::RegexpExtractAll | SqlFn::RangeFn),
        _ => true,
    }
}

fn any_nullable(a: &[SqlExpr]) -> bool {
    a.iter().any(may_be_null)
}

fn all_nullable(a: &[SqlExpr]) -> bool {
    a.iter().all(may_be_null)
}

/// `SqlTyping.reconcileSlot`: a projection's declared slot against its expression's stored fact -- the slot adopts
/// the computed type, and its nullability the slot truth ([`slot_nullable`]).
pub fn reconcile_slot(pe: &SqlExpr, oc: &OutputCol, grouped: bool) -> OutputCol {
    let t = match pe.r#type() {
        TypeFact::Typed { r#type, .. } => r#type.clone(),
        _ => oc.r#type.clone(),
    };
    let nul = slot_nullable(pe, grouped);
    if t == oc.r#type && nul == oc.nullable {
        return oc.clone();
    }
    OutputCol { name: oc.name.clone(), r#type: t, nullable: nul, origin: oc.origin }
}

/// `SqlTyping.reconcileUnionLabels`: column i adopts the branches' uniform type, their nullability's OR and the
/// first branch's origin; a mixed column keeps its label.
pub fn reconcile_union_labels(branches: &[SqlQuery], outputs: Vec<OutputCol>) -> Vec<OutputCol> {
    if branches.is_empty() {
        return outputs;
    }
    let mut os: Option<Vec<OutputCol>> = None;
    for i in 0..outputs.len() {
        let oc = &outputs[i];
        let mut t: Option<&SqlType> = None;
        let mut nul = false;
        let mut uniform = true;
        let mut first = None;
        for b in branches {
            let bo = query_outputs(b);
            if bo.len() != outputs.len() {
                uniform = false;
                break;
            }
            let bc = &bo[i];
            if first.is_none() {
                first = Some(bc.origin);
            }
            nul |= bc.nullable;
            match t {
                None => t = Some(&bc.r#type),
                Some(t0) if *t0 != bc.r#type => {
                    uniform = false;
                    break;
                }
                Some(_) => {}
            }
        }
        let (Some(t), Some(first)) = (t, first) else {
            continue;
        };
        if !uniform {
            continue;
        }
        if *t == oc.r#type && nul == oc.nullable && first == oc.origin {
            continue;
        }
        let replacement = OutputCol { name: oc.name.clone(), r#type: t.clone(), nullable: nul, origin: first };
        os.get_or_insert_with(|| outputs.clone())[i] = replacement;
    }
    os.unwrap_or(outputs)
}

// ---- the rule table, called by the node constructors

/// `SqlTyping.callType`: the kind from the per-function rules, the nullability from the per-function arm.
pub fn call_type(f: SqlFn, a: &[SqlExpr]) -> TypeFact {
    call_nullability(f, a, call_kind(f, a))
}

fn call_nullability(f: SqlFn, a: &[SqlExpr], base: TypeFact) -> TypeFact {
    let TypeFact::Typed { r#type, nullable } = base else {
        return base;
    };
    use SqlFn as F;
    let nul = match f {
        F::IsNull | F::IsNotNull => false,
        F::Concat | F::ConcatJoin => false,
        F::Hash | F::Typeof => false,
        F::ListConcat | F::MapConcat | F::ListAppend => false,
        F::Coalesce | F::Greatest | F::Least => all_nullable(a),
        F::ListGet | F::ListPosition | F::PureSplitPart => true,
        F::Unnest => a.is_empty() || elements_may_be_null(&a[0]),
        F::IntDivide | F::Rem | F::Mod => true,
        F::ListSum
        | F::ListAvg
        | F::ListMedian
        | F::ListMode
        | F::ListProduct
        | F::ListMax
        | F::ListMin
        | F::ListBoolAnd
        | F::ListBoolOr => true,
        F::ListReduce => true,
        F::VariantGet => true,
        _ => nullable || any_nullable(a),
    };
    TypeFact::Typed { r#type, nullable: nul }
}

fn first_type(a: &[SqlExpr]) -> Option<&TypeFact> {
    a.first().map(SqlExpr::r#type)
}

fn bottom_or(a: &[SqlExpr], t: Scalar) -> TypeFact {
    if a.is_empty() {
        return TypeFact::Unknown;
    }
    if a.iter().any(|x| matches!(x.r#type(), TypeFact::Bottom)) { TypeFact::Bottom } else { scalar(t) }
}

fn call_kind(f: SqlFn, a: &[SqlExpr]) -> TypeFact {
    use SqlFn as F;
    match f {
        F::And
        | F::Or
        | F::Not
        | F::Equal
        | F::NotEqual
        | F::Less
        | F::LessEqual
        | F::Greater
        | F::GreaterEqual
        | F::IsNull
        | F::IsNotNull
        | F::In
        | F::StartsWith
        | F::EndsWith
        | F::Matches
        | F::RegexpFullMatch
        | F::ListExists
        | F::ListForAll
        | F::IsDistinctFrom
        | F::AllDistinct
        | F::NullSafeEqual
        | F::NullSafeNotEqual
        | F::ListBoolAnd
        | F::ListBoolOr
        | F::Xor => scalar(Scalar::Boolean),
        F::Concat
        | F::ConcatJoin
        | F::Upper
        | F::Lower
        | F::Trim
        | F::Ltrim
        | F::Rtrim
        | F::Replace
        | F::Translate
        | F::Substring
        | F::Left
        | F::Right
        | F::Lpad
        | F::Rpad
        | F::ReverseString
        | F::UcFirst
        | F::LcFirst
        | F::SplitPart
        | F::PureSplitPart
        | F::RegexpExtract
        | F::RegexpReplace
        | F::Chr
        | F::EncodeBase64
        | F::DecodeBase64
        | F::Md5
        | F::Sha1
        | F::Sha256
        | F::Guid
        | F::Dayname
        | F::Monthname
        | F::Strftime
        | F::Typeof
        | F::BoolToText
        | F::FloatCanon
        | F::FloatPrint
        | F::Format
        | F::JsonType
        | F::JsonPretty
        | F::CurrentUserFn
        | F::RepeatStr => scalar(Scalar::Varchar),
        F::Length
        | F::Strpos
        | F::AsciiCode
        | F::Levenshtein
        | F::ListLength
        | F::ListPosition
        | F::Extract
        | F::DateDiff
        | F::EpochSeconds
        | F::EpochMs
        | F::EpochNs
        | F::ParseInt
        | F::Hash => scalar(Scalar::Bigint),
        F::Sqrt
        | F::Cbrt
        | F::Exp
        | F::Ln
        | F::Log10
        | F::Pow
        | F::Pi
        | F::Sin
        | F::Cos
        | F::Tan
        | F::Asin
        | F::Random
        | F::Acos
        | F::Atan
        | F::Atan2
        | F::Sinh
        | F::Cosh
        | F::Tanh
        | F::Cot
        | F::Radians
        | F::Degrees
        | F::Divide
        | F::JaroWinkler => scalar(Scalar::Double),
        F::Today | F::MakeDate => scalar(Scalar::Date),
        F::Now
        | F::MakeTimestamp
        | F::Strptime
        | F::ParseDate
        | F::FromEpochSeconds
        | F::FromEpochMs
        | F::FromEpochNs
        | F::Timezone
        | F::DateAtTime => scalar(Scalar::Timestamp),
        F::ListSum => a.first().map_or(TypeFact::Unknown, |c| reduce_collection_type(AggFn::Sum, c)),
        F::ListAvg => a.first().map_or(TypeFact::Unknown, |c| reduce_collection_type(AggFn::Avg, c)),
        F::ListMedian => a.first().map_or(TypeFact::Unknown, |c| reduce_collection_type(AggFn::Median, c)),
        F::ListMode => a.first().map_or(TypeFact::Unknown, |c| reduce_collection_type(AggFn::Mode, c)),
        F::ListProduct => list_product_type(a),
        F::ListAppend => list_append_type(a),
        F::ListReduce => match a {
            [_, SqlExpr::Lambda(lam)] => match lam.body.r#type() {
                TypeFact::Typed { r#type, .. } => typed(r#type.clone()),
                _ => TypeFact::Unknown,
            },
            _ => TypeFact::Unknown,
        },
        F::MapConcat => uniform(a, TypeFact::Bottom),
        F::MapFromEntries => map_from_entries_type(a),
        F::MapFromLists => match a {
            [k, v] => match (array_element(k.r#type()), array_element(v.r#type())) {
                (Some(ke), Some(ve)) => typed(SqlType::Map { key: Box::new(ke.clone()), value: Box::new(ve.clone()) }),
                _ => TypeFact::Unknown,
            },
            _ => TypeFact::Unknown,
        },
        F::MapExtract | F::MapValues => match first_type(a) {
            Some(TypeFact::Typed { r#type: SqlType::Map { value, .. }, .. }) => {
                typed(SqlType::Array { element: value.clone() })
            }
            _ => TypeFact::Unknown,
        },
        F::MapKeys => match first_type(a) {
            Some(TypeFact::Typed { r#type: SqlType::Map { key, .. }, .. }) => {
                typed(SqlType::Array { element: key.clone() })
            }
            _ => TypeFact::Unknown,
        },
        F::DateTrunc => bottom_or(a, Scalar::Timestamp),
        F::DateTruncDay => bottom_or(a, Scalar::Date),
        F::ToVariant | F::JsonMergePatch | F::VariantGet => scalar(Scalar::Json),
        F::VariantElements => typed(SqlType::Array { element: Box::new(SqlType::Scalar(Scalar::Json)) }),
        F::Split | F::RegexpExtractAll => typed(SqlType::Array { element: Box::new(SqlType::Scalar(Scalar::Varchar)) }),
        F::RangeFn => typed(SqlType::Array { element: Box::new(SqlType::Scalar(Scalar::Bigint)) }),
        F::RepeatValue => match first_type(a) {
            Some(TypeFact::Typed { r#type, .. }) => typed(SqlType::Array { element: Box::new(r#type.clone()) }),
            _ => TypeFact::Unknown,
        },
        F::Coalesce => uniform(a, TypeFact::Bottom),
        F::ListFilter
        | F::ListSort
        | F::ListSortDesc
        | F::ListTail
        | F::ListInit
        | F::ListSlice
        | F::ListDistinct
        | F::ListReverse => first_type(a).map_or(TypeFact::Unknown, array_pass),
        F::ListConcat => uniform(a, TypeFact::Bottom),
        F::ListGet | F::Unnest => first_type(a).map_or(TypeFact::Unknown, element),
        F::ListMax | F::ListMin => first_type(a).map_or(TypeFact::Unknown, element),
        F::AddInterval | F::AddIntervalTemporal => bottom_or(a, Scalar::Timestamp),
        F::BitAnd | F::BitOr | F::BitXor | F::BitNot | F::BitShiftLeft | F::BitShiftRight => bit_op_type(a),
        F::Greatest | F::Least => uniform(a, TypeFact::Bottom),
        F::ListFlatten => match first_type(a) {
            Some(TypeFact::Typed { r#type: SqlType::Array { element }, .. })
                if matches!(element.as_ref(), SqlType::Array { .. }) =>
            {
                typed(element.as_ref().clone())
            }
            _ => TypeFact::Unknown,
        },
        F::ListZip => {
            if a.len() < 2 {
                return TypeFact::Unknown;
            }
            match (array_element(a[0].r#type()), array_element(a[1].r#type())) {
                (Some(ea), Some(eb)) => typed(SqlType::Array {
                    element: Box::new(SqlType::Struct {
                        fields: vec![
                            StructField { name: "1".to_owned(), r#type: ea.clone() },
                            StructField { name: "2".to_owned(), r#type: eb.clone() },
                        ],
                    }),
                }),
                _ => TypeFact::Unknown,
            }
        }
        F::StructInsert => {
            let [s, name, v] = a else {
                return TypeFact::Unknown;
            };
            let (TypeFact::Typed { r#type: SqlType::Struct { fields }, .. }, SqlExpr::StringLit { value: name, .. }) =
                (s.r#type(), name)
            else {
                return TypeFact::Unknown;
            };
            let vt = match v.r#type() {
                TypeFact::Typed { r#type, .. } => r#type.clone(),
                TypeFact::Bottom => SqlType::Scalar(Scalar::Json),
                _ => return TypeFact::Unknown,
            };
            let mut fs = fields.clone();
            fs.push(StructField { name: name.clone(), r#type: vt });
            typed(SqlType::Struct { fields: fs })
        }
        F::ListTransform => {
            let [list, SqlExpr::Lambda(lam)] = a else {
                return TypeFact::Unknown;
            };
            if lam.params.len() > 2 {
                return TypeFact::Unknown;
            }
            if !matches!(list.r#type(), TypeFact::Typed { r#type: SqlType::Array { .. }, .. }) {
                return TypeFact::Unknown;
            }
            match lam.body.r#type() {
                TypeFact::Typed { r#type, .. } => typed(SqlType::Array { element: Box::new(r#type.clone()) }),
                _ => TypeFact::Unknown,
            }
        }
        F::Plus
        | F::Minus
        | F::Times
        | F::Rem
        | F::Mod
        | F::Negate
        | F::Abs
        | F::IntDivide
        | F::Round
        | F::Ceiling
        | F::Floor
        | F::Sign => arith_type(f, a),
        F::Error => TypeFact::Raises,
        _ => TypeFact::Unknown,
    }
}

fn array_element(f: &TypeFact) -> Option<&SqlType> {
    match f {
        TypeFact::Typed { r#type: SqlType::Array { element }, .. } => Some(element),
        _ => None,
    }
}

/// `SqlTyping.slotNullable`: a projection's delivered nullability at its select -- a top-level reducer under GROUP
/// BY sits over non-empty groups.
pub fn slot_nullable(pe: &SqlExpr, grouped: bool) -> bool {
    if grouped
        && let SqlExpr::Reducer(r) = pe
        && matches!(r.r#type, TypeFact::Typed { nullable: true, .. })
        && group_refines(r.r#fn)
    {
        return r.r#fn != AggFn::List && any_nullable(&r.args);
    }
    fact_may_be_null(pe.r#type())
}

fn group_refines(f: AggFn) -> bool {
    use AggFn as A;
    matches!(
        f,
        A::Sum
            | A::Avg
            | A::Min
            | A::Max
            | A::AnyValue
            | A::Mode
            | A::QuantileDisc
            | A::QuantileCont
            | A::Median
            | A::ArgMax
            | A::ArgMin
            | A::StringAgg
            | A::BoolAnd
            | A::BoolOr
            | A::List
            | A::StddevPop
            | A::VarPop
            | A::CovarPop
            | A::Corr
    )
}

/// `SqlTyping.wherePadNeutralized`: a star-framed join whose WHERE null-rejects a padded side's column restores that
/// side's nullability.
pub fn where_pad_neutralized(from: &SqlSource, r#where: &SqlExpr, outputs: Vec<OutputCol>) -> Vec<OutputCol> {
    let mut rejected = HashSet::new();
    strict_rejections(r#where, &mut rejected);
    if rejected.is_empty() {
        return outputs;
    }
    let mut tree = HashMap::new();
    tree_nullability(from, &rejected, &mut tree);
    let mut os: Option<Vec<OutputCol>> = None;
    for (i, oc) in outputs.iter().enumerate() {
        let Some(&nul) = tree.get(&oc.name) else {
            continue;
        };
        if nul == oc.nullable {
            continue;
        }
        let replacement =
            OutputCol { name: oc.name.clone(), r#type: oc.r#type.clone(), nullable: nul, origin: oc.origin };
        os.get_or_insert_with(|| outputs.clone())[i] = replacement;
    }
    os.unwrap_or(outputs)
}

fn tree_nullability(src: &SqlSource, rejected: &HashSet<String>, out: &mut HashMap<String, bool>) {
    if let SqlSource::Join { left, right, kind, .. } = src {
        let mut l = HashMap::new();
        let mut r = HashMap::new();
        tree_nullability(left, rejected, &mut l);
        tree_nullability(right, rejected, &mut r);
        if pads_left(*kind) && !rejected.iter().any(|n| l.contains_key(n)) {
            l.values_mut().for_each(|v| *v = true);
        }
        if pads_right(*kind) && !rejected.iter().any(|n| r.contains_key(n)) {
            r.values_mut().for_each(|v| *v = true);
        }
        out.extend(l);
        out.extend(r);
        return;
    }
    for c in source_outputs(src) {
        out.insert(c.name.clone(), c.nullable);
    }
}

/// `SqlTyping.strictRejections`: the column names the predicate null-rejects (AND and parentheses decompose).
fn strict_rejections(w: &SqlExpr, out: &mut HashSet<String>) {
    match w {
        SqlExpr::Group { inner, .. } => strict_rejections(inner, out),
        SqlExpr::Call { r#fn: SqlFn::And, args, .. } => args.iter().for_each(|a| strict_rejections(a, out)),
        _ => {
            if null_strict(w) {
                column_names(w, out);
            }
        }
    }
}

/// `SqlTyping.nullStrict`: no node in the subtree can turn a NULL operand into TRUE.
fn null_strict(e: &SqlExpr) -> bool {
    if matches!(e, SqlExpr::Case { .. }) {
        return false;
    }
    if let SqlExpr::Call { r#fn, .. } = e
        && matches!(
            r#fn,
            SqlFn::IsNull
                | SqlFn::Coalesce
                | SqlFn::Or
                | SqlFn::NullSafeEqual
                | SqlFn::NullSafeNotEqual
                | SqlFn::IsDistinctFrom
                | SqlFn::AllDistinct
                | SqlFn::ListBoolAnd
                | SqlFn::ListBoolOr
        )
    {
        return false;
    }
    children(e).iter().all(|c| null_strict(c))
}

fn column_names(e: &SqlExpr, out: &mut HashSet<String>) {
    if let SqlExpr::Column(c) = e {
        out.insert(c.name.clone());
    }
    for c in children(e) {
        column_names(&c, out);
    }
}

/// `SqlTyping.membershipType`: BOOLEAN, nullable when the needle or the collection may be NULL.
pub fn membership_type(needle: &SqlExpr, collection: &SqlExpr) -> TypeFact {
    if may_be_null(needle) || may_be_null(collection) {
        nullable(scalar(Scalar::Boolean))
    } else {
        scalar(Scalar::Boolean)
    }
}

/// `SqlTyping.castType`: the target, nullability transported.
pub fn cast_type(value: &SqlExpr, target: &SqlType) -> TypeFact {
    let t = typed(target.clone());
    if may_be_null(value) { nullable(t) } else { t }
}

fn arith_type(f: SqlFn, a: &[SqlExpr]) -> TypeFact {
    use SqlFn as F;
    match f {
        F::Plus | F::Minus => {
            if let [l, r] = a
                && let (TypeFact::Typed { r#type: lt, .. }, TypeFact::Typed { r#type: rt, .. }) =
                    (l.r#type(), r.r#type())
            {
                let date = SqlType::Scalar(Scalar::Date);
                let (l_date, r_date) = (*lt == date, *rt == date);
                if l_date && r_date {
                    return if f == F::Minus { scalar(Scalar::Bigint) } else { TypeFact::Unknown };
                }
                if l_date && integer_kind(rt) || r_date && f == F::Plus && integer_kind(lt) {
                    return scalar(Scalar::Date);
                }
            }
            decimal_arith(false, a).unwrap_or_else(|| numeric_promotion(a))
        }
        F::Times => decimal_arith(true, a).unwrap_or_else(|| numeric_promotion(a)),
        F::Rem => rem_decimal_type(a).unwrap_or_else(|| numeric_promotion(a)),
        F::Mod => numeric_promotion(a),
        F::Negate | F::Abs => match first_type(a) {
            None => TypeFact::Unknown,
            Some(TypeFact::Bottom) => TypeFact::Bottom,
            Some(f @ TypeFact::Typed { r#type, .. })
                if integer_kind(r#type)
                    || *r#type == SqlType::Scalar(Scalar::Double)
                    || matches!(r#type, SqlType::Decimal { .. }) =>
            {
                f.clone()
            }
            Some(_) => TypeFact::Unknown,
        },
        F::IntDivide => match numeric_promotion(a) {
            p @ (TypeFact::Typed { .. } | TypeFact::Bottom) => p,
            _ => TypeFact::Unknown,
        },
        F::Round => match first_type(a) {
            None => TypeFact::Unknown,
            Some(TypeFact::Bottom) => TypeFact::Bottom,
            Some(f @ TypeFact::Typed { r#type, .. }) => {
                if integer_kind(r#type) {
                    f.clone()
                } else if *r#type == SqlType::Scalar(Scalar::Double) {
                    scalar(Scalar::Double)
                } else {
                    TypeFact::Unknown
                }
            }
            Some(_) => TypeFact::Unknown,
        },
        F::Ceiling | F::Floor | F::Sign => match first_type(a) {
            None => TypeFact::Unknown,
            Some(TypeFact::Bottom) => TypeFact::Bottom,
            Some(TypeFact::Typed { r#type, .. })
                if integer_kind(r#type)
                    || *r#type == SqlType::Scalar(Scalar::Double)
                    || matches!(r#type, SqlType::Decimal { .. }) =>
            {
                scalar(Scalar::Bigint)
            }
            Some(_) => TypeFact::Unknown,
        },
        // Java's "non-arithmetic fn routed to arithType": callKind routes only the twelve above here
        _ => TypeFact::Unknown,
    }
}

fn bit_op_type(a: &[SqlExpr]) -> TypeFact {
    let mut bottom = false;
    let mut width = 0;
    for e in a {
        match e.r#type() {
            TypeFact::Bottom => bottom = true,
            TypeFact::Typed { r#type, .. } if integer_kind(r#type) => width = width.max(int_width(r#type)),
            _ => {
                width = -1;
                break;
            }
        }
    }
    if width < 0 || a.is_empty() {
        return TypeFact::Unknown;
    }
    if bottom {
        return TypeFact::Bottom;
    }
    width_type(width)
}

fn width_type(width: i32) -> TypeFact {
    match width {
        1 => scalar(Scalar::Integer),
        2 => scalar(Scalar::Bigint),
        3 => scalar(Scalar::Hugeint),
        _ => TypeFact::Unknown,
    }
}

fn list_product_type(a: &[SqlExpr]) -> TypeFact {
    match first_type(a) {
        None => TypeFact::Unknown,
        Some(TypeFact::Bottom) => TypeFact::Bottom,
        Some(TypeFact::Typed { r#type: SqlType::Array { .. }, .. }) => scalar(Scalar::Double),
        Some(_) => TypeFact::Unknown,
    }
}

fn list_append_type(a: &[SqlExpr]) -> TypeFact {
    let [list, e] = a else {
        return TypeFact::Unknown;
    };
    let TypeFact::Typed { r#type: at @ SqlType::Array { element }, .. } = list.r#type() else {
        return TypeFact::Unknown;
    };
    match e.r#type() {
        TypeFact::Bottom => typed(at.clone()),
        TypeFact::Typed { r#type, .. } if r#type == element.as_ref() => typed(at.clone()),
        _ => TypeFact::Unknown,
    }
}

fn map_from_entries_type(a: &[SqlExpr]) -> TypeFact {
    if let [e] = a
        && let Some(SqlType::Struct { fields }) = array_element(e.r#type())
        && fields.len() == 2
    {
        return typed(SqlType::Map {
            key: Box::new(fields[0].r#type.clone()),
            value: Box::new(fields[1].r#type.clone()),
        });
    }
    TypeFact::Unknown
}

/// `SqlTyping.caseType`: the branches' shared type; nullable when a branch may be NULL or the ELSE is missing.
pub fn case_type(whens: &[When], otherwise: Option<&SqlExpr>) -> TypeFact {
    let branches: Vec<&SqlExpr> = whens.iter().map(|w| &w.then).chain(otherwise).collect();
    let t = uniform_refs(&branches, TypeFact::Bottom);
    let nul = otherwise.is_none() || branches.iter().any(|b| may_be_null(b));
    if nul { nullable(t) } else { t }
}

/// `SqlTyping.arrayLitType`: the uniform element type, wrapped; UNKNOWN for an empty or all-NULL literal.
pub fn array_lit_type(elements: &[SqlExpr]) -> TypeFact {
    if elements.is_empty() {
        return TypeFact::Unknown;
    }
    match uniform(elements, TypeFact::Unknown) {
        TypeFact::Typed { r#type, .. } => typed(SqlType::Array { element: Box::new(r#type) }),
        _ => TypeFact::Unknown,
    }
}

/// `SqlTyping.structLitType`: every field's type in order; a NULL field takes its declared slot.
pub fn struct_lit_type(fields: &[StructLitField]) -> TypeFact {
    let mut fs = Vec::with_capacity(fields.len());
    for f in fields {
        match (f.value.r#type(), &f.declared) {
            (TypeFact::Typed { r#type, .. }, _) => {
                fs.push(StructField { name: f.name.clone(), r#type: r#type.clone() })
            }
            (TypeFact::Bottom, Some(declared)) => {
                fs.push(StructField { name: f.name.clone(), r#type: declared.clone() })
            }
            _ => return TypeFact::Unknown,
        }
    }
    typed(SqlType::Struct { fields: fs })
}

/// `SqlTyping.structGetType`: the named field of a typed struct, nullable; the NULL value's field is the NULL value.
pub fn struct_get_type(source: &SqlExpr, field: &str) -> TypeFact {
    match source.r#type() {
        TypeFact::Bottom => TypeFact::Bottom,
        TypeFact::Typed { r#type: SqlType::Struct { fields }, .. } => {
            fields.iter().find(|f| f.name == field).map_or(TypeFact::Unknown, |f| nullable(typed(f.r#type.clone())))
        }
        _ => TypeFact::Unknown,
    }
}

/// `SqlTyping.scalarSubqueryType`: the single output's label (or a label-less single projection's own fact),
/// nullable unless the inner select provably returns one row.
pub fn scalar_subquery_type(sub: &SqlQuery) -> TypeFact {
    let one_row = one_row_aggregate(sub);
    let outputs = query_outputs(sub);
    if outputs.len() == 1 {
        let t = typed(outputs[0].r#type.clone());
        return if one_row && !outputs[0].nullable { t } else { nullable(t) };
    }
    if let SqlQuery::Select(s) = sub
        && s.outputs.is_empty()
        && s.projections.len() == 1
        && !matches!(s.projections[0].expr, SqlExpr::Star { .. } | SqlExpr::StarExcept { .. })
    {
        let inner = s.projections[0].expr.r#type().clone();
        return if one_row { inner } else { nullable(inner) };
    }
    TypeFact::Unknown
}

fn one_row_aggregate(sub: &SqlQuery) -> bool {
    let SqlQuery::Select(s) = sub else {
        return false;
    };
    if !s.group_by.is_empty()
        || s.having.is_some()
        || s.qualify.is_some()
        || s.offset.is_some()
        || s.limit.is_some_and(|l| l < 1)
    {
        return false;
    }
    s.projections.iter().any(|p| aggregates_select(&p.expr))
}

/// `SqlAgg.aggregatesSelect(e)`: whether `e` aggregates its select -- a reducer (or an ordered or JSON aggregation)
/// outside any window or lambda.
pub fn aggregates_select(e: &SqlExpr) -> bool {
    match e {
        SqlExpr::Reducer(_) | SqlExpr::OrderedListAgg { .. } | SqlExpr::JsonArrayAgg { .. } => true,
        SqlExpr::WindowCall { .. } | SqlExpr::Lambda(_) => false,
        _ => children(e).iter().any(|c| aggregates_select(c)),
    }
}

/// `SqlTyping.checkedOneType`: the element of a definite list, nullable.
pub fn checked_one_type(list: &SqlExpr) -> TypeFact {
    match list.r#type() {
        TypeFact::Bottom => TypeFact::Bottom,
        TypeFact::Typed { r#type: SqlType::Array { element }, .. } => nullable(typed(element.as_ref().clone())),
        _ => TypeFact::Unknown,
    }
}

/// `SqlTyping.foldType`: the accumulator's type, when the body and the init agree.
pub fn fold_type(source: &SqlExpr, lambda: &Lambda, init: &SqlExpr, acc_is_list: bool) -> TypeFact {
    let t = match (lambda.body.r#type(), init.r#type()) {
        (TypeFact::Typed { r#type: bt, .. }, TypeFact::Typed { r#type: it, .. })
            if bt == it && (!acc_is_list || matches!(bt, SqlType::Array { .. })) =>
        {
            typed(bt.clone())
        }
        _ => TypeFact::Unknown,
    };
    if may_be_null(source) || may_be_null(init) || may_be_null(&lambda.body) { nullable(t) } else { t }
}

/// `SqlTyping.windowType`: a windowed reducer keeps its own fact; the ranking and value kinds by kind.
pub fn window_type(f: &SqlAgg) -> TypeFact {
    match f {
        SqlAgg::Reducer(r) => r.r#type.clone(),
        SqlAgg::RankingFn { r#fn, .. } => match r#fn {
            AggFn::RowNumber | AggFn::Rank | AggFn::DenseRank | AggFn::Ntile => scalar(Scalar::Bigint),
            AggFn::PercentRank | AggFn::CumeDist => scalar(Scalar::Double),
            _ => TypeFact::Unknown,
        },
        SqlAgg::ValueFn { args, .. } => args.first().map_or(TypeFact::Unknown, |a| nullable(a.r#type().clone())),
    }
}

/// `SqlTyping.reducerType`: the aggregate promotion rules; every reducer but COUNT nullable at the node.
pub fn reducer_type(f: AggFn, arg0: &TypeFact) -> TypeFact {
    let base = reducer_kind(f, arg0);
    if f == AggFn::Count { base } else { nullable(base) }
}

fn reducer_kind(f: AggFn, arg0: &TypeFact) -> TypeFact {
    use AggFn as A;
    match f {
        A::Count => return scalar(Scalar::Bigint),
        A::StringAgg => return scalar(Scalar::Varchar),
        A::BoolAnd | A::BoolOr => return scalar(Scalar::Boolean),
        A::StddevSamp
        | A::StddevPop
        | A::VarSamp
        | A::VarPop
        | A::Stddev
        | A::Variance
        | A::Corr
        | A::CovarSamp
        | A::CovarPop => return scalar(Scalar::Double),
        _ => {}
    }
    let TypeFact::Typed { r#type: t, .. } = arg0 else {
        return TypeFact::Unknown;
    };
    let is = |s: Scalar| *t == SqlType::Scalar(s);
    match f {
        A::Sum => {
            if integer_family(t) || is(Scalar::Boolean) {
                scalar(Scalar::Hugeint)
            } else if is(Scalar::Double) {
                scalar(Scalar::Double)
            } else if let SqlType::Decimal { scale, .. } = t {
                typed(SqlType::Decimal { precision: 38, scale: *scale })
            } else {
                TypeFact::Unknown
            }
        }
        A::Avg => {
            if integer_family(t) || is(Scalar::Double) || matches!(t, SqlType::Decimal { .. }) {
                scalar(Scalar::Double)
            } else if is(Scalar::Date) || is(Scalar::Timestamp) {
                scalar(Scalar::Timestamp)
            } else {
                TypeFact::Unknown
            }
        }
        A::Min | A::Max | A::AnyValue | A::Mode | A::QuantileDisc | A::ArgMax | A::ArgMin => arg0.clone(),
        A::Median | A::QuantileCont => {
            if integer_family(t) || is(Scalar::Double) {
                scalar(Scalar::Double)
            } else if matches!(t, SqlType::Decimal { .. }) {
                typed(t.clone())
            } else if is(Scalar::Date) || is(Scalar::Timestamp) {
                scalar(Scalar::Timestamp)
            } else if f == A::Median && (is(Scalar::Varchar) || is(Scalar::Boolean)) {
                typed(t.clone())
            } else {
                TypeFact::Unknown
            }
        }
        A::List => typed(SqlType::Array { element: Box::new(t.clone()) }),
        _ => TypeFact::Unknown,
    }
}

/// `SqlTyping.reduceCollectionType`: the aggregate promotion read through the collection; a NULL collection reduces
/// to NULL.
pub fn reduce_collection_type(f: AggFn, collection: &SqlExpr) -> TypeFact {
    let r = match array_element(collection.r#type()) {
        Some(e) => reducer_type(f, &typed(e.clone())),
        None => TypeFact::Unknown,
    };
    if may_be_null(collection) { nullable(r) } else { r }
}

fn integer_family(t: &SqlType) -> bool {
    matches!(t, SqlType::Scalar(Scalar::Bigint | Scalar::Integer | Scalar::Hugeint))
}

fn array_pass(v: &TypeFact) -> TypeFact {
    match v {
        TypeFact::Bottom => TypeFact::Bottom,
        TypeFact::Typed { r#type: SqlType::Array { .. }, .. } => v.clone(),
        _ => TypeFact::Unknown,
    }
}

fn element(v: &TypeFact) -> TypeFact {
    match v {
        TypeFact::Bottom => TypeFact::Bottom,
        TypeFact::Typed { r#type: SqlType::Array { element }, .. } => typed(element.as_ref().clone()),
        _ => TypeFact::Unknown,
    }
}

fn integer_kind(t: &SqlType) -> bool {
    matches!(t, SqlType::Scalar(Scalar::Integer | Scalar::Bigint | Scalar::Hugeint))
}

fn decimal_arith(multiply: bool, a: &[SqlExpr]) -> Option<TypeFact> {
    if a.is_empty() {
        return None;
    }
    let mut saw_decimal = false;
    let mut bottom = false;
    let mut dbl = false;
    let mut ds: Vec<(i32, i32)> = Vec::with_capacity(a.len());
    for e in a {
        let t = match e.r#type() {
            TypeFact::Bottom => {
                bottom = true;
                continue;
            }
            TypeFact::Typed { r#type, .. } => r#type,
            _ => return None,
        };
        match t {
            SqlType::Scalar(Scalar::Double) => dbl = true,
            SqlType::Decimal { precision, scale } => {
                saw_decimal = true;
                ds.push((*precision, *scale));
            }
            SqlType::Scalar(Scalar::Integer) => ds.push((10, 0)),
            SqlType::Scalar(Scalar::Bigint) => ds.push((19, 0)),
            SqlType::Scalar(Scalar::Hugeint) => ds.push((38, 0)),
            _ => return None,
        }
    }
    if !saw_decimal {
        return None;
    }
    if bottom {
        return Some(TypeFact::Bottom);
    }
    if dbl {
        return Some(scalar(Scalar::Double));
    }
    let (mut p, mut s) = ds[0];
    for &(p2, s2) in &ds[1..] {
        let cap = if p <= 18 && p2 <= 18 { 18 } else { 38 };
        let (ns, nw) = if multiply {
            (s + s2, p + p2)
        } else {
            let ns = s.max(s2);
            (ns, (p - s).max(p2 - s2) + ns + 1)
        };
        p = nw.min(cap);
        s = ns;
    }
    Some(typed(SqlType::Decimal { precision: p, scale: s }))
}

fn numeric_promotion(a: &[SqlExpr]) -> TypeFact {
    if a.is_empty() {
        return TypeFact::Unknown;
    }
    let mut bottom = false;
    let mut dbl = false;
    let mut width = 0;
    for e in a {
        let t = match e.r#type() {
            TypeFact::Bottom => {
                bottom = true;
                continue;
            }
            TypeFact::Typed { r#type, .. } => r#type,
            _ => return TypeFact::Unknown,
        };
        match t {
            SqlType::Scalar(Scalar::Double) => dbl = true,
            SqlType::Scalar(Scalar::Integer) => width = width.max(1),
            SqlType::Scalar(Scalar::Bigint) => width = width.max(2),
            SqlType::Scalar(Scalar::Hugeint) => width = width.max(3),
            _ => return TypeFact::Unknown,
        }
    }
    if bottom {
        return TypeFact::Bottom;
    }
    if dbl {
        return scalar(Scalar::Double);
    }
    width_type(width)
}

/// `SqlTyping.uniform`: the single shared type of a branch or argument family.
fn uniform(es: &[SqlExpr], all_bottom: TypeFact) -> TypeFact {
    let refs: Vec<&SqlExpr> = es.iter().collect();
    uniform_refs(&refs, all_bottom)
}

fn uniform_refs(es: &[&SqlExpr], all_bottom: TypeFact) -> TypeFact {
    let mut common: Option<SqlType> = None;
    let mut saw = false;
    let mut empty_array = false;
    for e in es {
        let v = e.r#type();
        if matches!(v, TypeFact::Bottom | TypeFact::Raises) {
            saw = true;
            continue;
        }
        if let SqlExpr::ArrayLit { elements, .. } = e
            && elements.is_empty()
        {
            saw = true;
            empty_array = true;
            continue;
        }
        let TypeFact::Typed { r#type: t, .. } = v else {
            return TypeFact::Unknown;
        };
        saw = true;
        common = match common {
            None => Some(t.clone()),
            Some(c) if c == *t => Some(c),
            Some(c) => match branch_promote(&c, t) {
                Some(p) => Some(p),
                None => return TypeFact::Unknown,
            },
        };
    }
    if let Some(c) = common {
        return if !empty_array || matches!(c, SqlType::Array { .. }) { typed(c) } else { TypeFact::Unknown };
    }
    if empty_array {
        return TypeFact::Unknown;
    }
    if saw { all_bottom } else { TypeFact::Unknown }
}

fn branch_promote(a: &SqlType, b: &SqlType) -> Option<SqlType> {
    if integer_kind(a) && integer_kind(b) {
        return Some(if int_width(a) >= int_width(b) { a.clone() } else { b.clone() });
    }
    let a_num = integer_kind(a) || matches!(a, SqlType::Decimal { .. });
    let b_num = integer_kind(b) || matches!(b, SqlType::Decimal { .. });
    let double = SqlType::Scalar(Scalar::Double);
    if *a == double && b_num || *b == double && a_num {
        return Some(double);
    }
    if a_num && b_num && (matches!(a, SqlType::Decimal { .. }) || matches!(b, SqlType::Decimal { .. })) {
        let (da, db) = (union_dec(a), union_dec(b));
        let s = da.1.max(db.1);
        let w = ((da.0 - da.1).max(db.0 - db.1) + s).min(38);
        return Some(SqlType::Decimal { precision: w, scale: s });
    }
    let (date, ts) = (SqlType::Scalar(Scalar::Date), SqlType::Scalar(Scalar::Timestamp));
    if *a == date && *b == ts || *a == ts && *b == date {
        return Some(ts);
    }
    None
}

/// `SqlTyping.decimalLitType`, over `BigDecimal`'s text: typed as the emission -- a fractional literal its rendered
/// digits, a scale-0 one by magnitude.
pub fn decimal_lit_type(v: &BigDecimal) -> TypeFact {
    decimal_text_type(v.as_str())
}

/// `decimalLitType(BigDecimal.valueOf(v))`, the fact of a Float literal, over `Double.toString`'s text of `v` (which
/// `BigDecimal.valueOf` reads). A NaN or an infinity has none (Java's `BigDecimal.valueOf` refuses it).
pub fn float_lit_type(double_text: &str) -> TypeFact {
    if double_text.contains(['N', 'I']) {
        return TypeFact::Unknown;
    }
    decimal_text_type(double_text)
}

fn decimal_text_type(text: &str) -> TypeFact {
    let (negative, mut digits, mut scale) = unscaled(text);
    if scale < 0 {
        // setScale(0)
        if digits != "0" {
            digits.push_str(&"0".repeat((-scale) as usize));
        }
        scale = 0;
    }
    let precision = digits.len() as i64;
    if scale > 0 {
        let rendered = (precision - scale).max(1) + scale;
        if rendered > 38 {
            return scalar(Scalar::Double);
        }
        return typed(SqlType::Decimal { precision: rendered as i32, scale: scale as i32 });
    }
    let bits = bit_length(negative, &digits);
    if bits >= 128 {
        return scalar(Scalar::Double);
    }
    if bits >= 64 {
        return scalar(Scalar::Hugeint);
    }
    typed(SqlType::Decimal { precision: precision.max(1) as i32, scale: 0 })
}

/// `BigDecimal.precision()`: the digits of its unscaled value (a zero has one).
pub fn big_decimal_precision(v: &BigDecimal) -> usize {
    unscaled(v.as_str()).1.len()
}

/// A BigDecimal's text read as its sign, its unscaled digits (no leading zero but a lone `0`) and its scale.
pub(crate) fn unscaled(text: &str) -> (bool, String, i64) {
    let (negative, unsigned) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (significand, exponent) = match unsigned.find(['e', 'E']) {
        Some(i) => (&unsigned[..i], unsigned[i + 1..].parse::<i64>().unwrap_or(0)),
        None => (unsigned, 0),
    };
    let (int, frac) = significand.split_once('.').unwrap_or((significand, ""));
    let joined = format!("{int}{frac}");
    let trimmed = joined.trim_start_matches('0');
    let digits = if trimmed.is_empty() { "0".to_owned() } else { trimmed.to_owned() };
    let negative = negative && digits != "0";
    (negative, digits, frac.len() as i64 - exponent)
}

/// `BigInteger.bitLength()` of the integer whose sign and decimal digits these are: the bits of its minimal two's
/// complement form, the sign bit excluded.
fn bit_length(negative: bool, digits: &str) -> u32 {
    if digits.len() > 39 {
        return 200;
    }
    let Ok(m) = digits.parse::<u128>() else {
        return 200;
    };
    // a negative n's bit length is that of -n - 1
    let m = if negative { m - 1 } else { m };
    128 - m.leading_zeros()
}

fn rem_decimal_type(a: &[SqlExpr]) -> Option<TypeFact> {
    let [l, r] = a else {
        return None;
    };
    let (TypeFact::Typed { r#type: lt, .. }, TypeFact::Typed { r#type: rt, .. }) = (l.r#type(), r.r#type()) else {
        return None;
    };
    let l_dec = matches!(lt, SqlType::Decimal { .. });
    let r_dec = matches!(rt, SqlType::Decimal { .. });
    if !l_dec && !r_dec || !(l_dec || integer_kind(lt)) || !(r_dec || integer_kind(rt)) {
        return None;
    }
    let (da, db) = (union_dec(lt), union_dec(rt));
    let s = da.1.max(db.1);
    let w = ((da.0 - da.1).max(db.0 - db.1) + s).min(38);
    Some(typed(SqlType::Decimal { precision: w, scale: s }))
}

fn union_dec(t: &SqlType) -> (i32, i32) {
    match t {
        SqlType::Decimal { precision, scale } => (*precision, *scale),
        SqlType::Scalar(Scalar::Integer) => (10, 0),
        SqlType::Scalar(Scalar::Bigint) => (19, 0),
        _ => (38, 0),
    }
}

fn int_width(t: &SqlType) -> i32 {
    match t {
        SqlType::Scalar(Scalar::Integer) => 1,
        SqlType::Scalar(Scalar::Bigint) => 2,
        _ => 3,
    }
}

// ---- the tree's structural contract

/// Java's `SqlExpr.children()`: the direct expression children, in rebuild order; a node's subquery is its own
/// (expression walkers never descend into one). A child Java holds as its narrow record (a stored read's `Column`, a
/// fold's `Lambda`, a window's `Reducer`) is a copy wrapped as an expression.
pub fn children(e: &SqlExpr) -> Vec<Cow<'_, SqlExpr>> {
    let b = Cow::Borrowed;
    match e {
        SqlExpr::StoredRead { column, .. } => vec![Cow::Owned(SqlExpr::Column(column.clone()))],
        SqlExpr::Membership { needle, collection, .. } => vec![b(needle.as_ref()), b(collection.as_ref())],
        SqlExpr::ReduceCollection { collection, extras, .. } => {
            std::iter::once(b(collection.as_ref())).chain(extras.iter().map(b)).collect()
        }
        SqlExpr::Group { inner, .. } => vec![b(inner.as_ref())],
        SqlExpr::ArrayLit { elements, .. } => elements.iter().map(b).collect(),
        SqlExpr::OrderedListAgg { value, order_by, .. } => vec![b(value.as_ref()), b(order_by.as_ref())],
        SqlExpr::StructLit { fields, .. } => fields.iter().map(|f| b(&f.value)).collect(),
        SqlExpr::StructGet { source, .. } => vec![b(source.as_ref())],
        SqlExpr::Call { args, .. } => args.iter().map(b).collect(),
        SqlExpr::Case { whens, otherwise, .. } => whens
            .iter()
            .flat_map(|w| [b(&w.condition), b(&w.then)])
            .chain(otherwise.iter().map(|o| b(o.as_ref())))
            .collect(),
        SqlExpr::InSubquery { value, .. } | SqlExpr::Quantified { value, .. } => vec![b(value.as_ref())],
        SqlExpr::CheckedDefects { own, hoists, .. } => {
            std::iter::once(b(own.as_ref())).chain(hoists.iter().map(|h| b(&h.envelope))).collect()
        }
        SqlExpr::CheckedChildValue { envelope, .. } => vec![b(envelope.as_ref())],
        SqlExpr::CheckedOne { list, .. } | SqlExpr::CompactList { list, .. } => vec![b(list.as_ref())],
        SqlExpr::WindowCall { r#fn, partition_by, order_by, .. } => {
            let mut out = Vec::new();
            if let SqlAgg::Reducer(r) = r#fn.as_ref() {
                out.push(Cow::Owned(SqlExpr::Reducer(r.clone())));
            }
            out.extend(partition_by.iter().map(b));
            out.extend(order_by.iter().map(|k| b(&k.expr)));
            out
        }
        SqlExpr::Lambda(l) => vec![b(l.body.as_ref())],
        SqlExpr::Cast { value, .. } => vec![b(value.as_ref())],
        SqlExpr::FoldCall { source, lambda, init, .. } => {
            vec![b(source.as_ref()), Cow::Owned(SqlExpr::Lambda(lambda.clone())), b(init.as_ref())]
        }
        SqlExpr::JsonObject { kv, .. } => kv.iter().map(b).collect(),
        SqlExpr::JsonArray { elements, .. } => elements.iter().map(b).collect(),
        SqlExpr::JsonArrayAgg { value, order_keys, .. } => {
            std::iter::once(b(value.as_ref())).chain(order_keys.iter().map(|k| b(&k.expr))).collect()
        }
        SqlExpr::Reducer(r) => r.args.iter().map(b).chain(r.order_by.iter().map(|k| b(&k.expr))).collect(),
        // the leaves, and the nodes whose children are queries
        SqlExpr::Column(_)
        | SqlExpr::RowOrder { .. }
        | SqlExpr::StarExcept { .. }
        | SqlExpr::Star { .. }
        | SqlExpr::StringLit { .. }
        | SqlExpr::IntLit { .. }
        | SqlExpr::FloatLit { .. }
        | SqlExpr::DecimalLit { .. }
        | SqlExpr::BoolLit { .. }
        | SqlExpr::NullLit { .. }
        | SqlExpr::DateLit { .. }
        | SqlExpr::KeyCell { .. }
        | SqlExpr::PlanParam { .. }
        | SqlExpr::FormatLit { .. }
        | SqlExpr::DatePartLit { .. }
        | SqlExpr::JsonKindLit { .. }
        | SqlExpr::TypeNameLit { .. }
        | SqlExpr::TimestampLit { .. }
        | SqlExpr::TimeLit { .. }
        | SqlExpr::Exists { .. }
        | SqlExpr::ScalarSubquery { .. }
        | SqlExpr::DeferredTdsString { .. }
        | SqlExpr::TempTableInSplice { .. } => Vec::new(),
    }
}

/// Java's `SqlQuery.outputs()`: a select's and a union's own, a WITH's body's.
pub fn query_outputs(q: &SqlQuery) -> &[OutputCol] {
    match q {
        SqlQuery::Select(s) => &s.outputs,
        SqlQuery::Union(u) => &u.outputs,
        SqlQuery::With(w) => query_outputs(&w.body),
    }
}

/// Java's `SqlSource.outputs()`: a subselect's its query's, Dual's none, every other source's its own.
pub fn source_outputs(s: &SqlSource) -> &[OutputCol] {
    match s {
        SqlSource::Subselect { inner, .. } => query_outputs(inner),
        SqlSource::Dual => &[],
        SqlSource::Table { outputs, .. }
        | SqlSource::Cte { outputs, .. }
        | SqlSource::Join { outputs, .. }
        | SqlSource::Pivot { outputs, .. }
        | SqlSource::Values { outputs, .. }
        | SqlSource::SourceUrl { outputs, .. }
        | SqlSource::RawSql { outputs, .. }
        | SqlSource::Catalog { outputs, .. }
        | SqlSource::VarSetPlaceholder { outputs, .. } => outputs,
    }
}

/// Java's `SqlSource.alias()`: a nested join and Dual have none (Java's `IllegalStateException`).
pub fn source_alias(s: &SqlSource) -> Result<&str, Refusal> {
    match s {
        SqlSource::Join { .. } => Err(Refusal::illegal_state("a nested join has no single alias — resolve per side")),
        SqlSource::Dual => Err(Refusal::illegal_state("a Dual (FROM-less) source has no alias — caller bug")),
        SqlSource::Table { alias, .. }
        | SqlSource::Subselect { alias, .. }
        | SqlSource::Cte { alias, .. }
        | SqlSource::Pivot { alias, .. }
        | SqlSource::Values { alias, .. }
        | SqlSource::SourceUrl { alias, .. }
        | SqlSource::RawSql { alias, .. }
        | SqlSource::Catalog { alias, .. }
        | SqlSource::VarSetPlaceholder { alias, .. } => Ok(alias),
    }
}

/// `Join.Kind.padsLeft()`: RIGHT and FULL outer joins NULL-pad their left side.
pub fn pads_left(k: JoinKind) -> bool {
    matches!(k, JoinKind::Right | JoinKind::Full)
}

/// `Join.Kind.padsRight()`: LEFT and FULL outer, ASOF LEFT and LEFT LATERAL NULL-pad their right side.
pub fn pads_right(k: JoinKind) -> bool {
    matches!(k, JoinKind::Left | JoinKind::Full | JoinKind::AsofLeft | JoinKind::LeftLateral)
}

/// `Join.delivered(left, right, kind)`: both sides' outputs, a padded side's non-null columns weakened to nullable.
pub fn join_delivered(left: &SqlSource, right: &SqlSource, kind: JoinKind) -> Vec<OutputCol> {
    let mut all = Vec::new();
    padded_side(source_outputs(left), pads_left(kind), &mut all);
    padded_side(source_outputs(right), pads_right(kind), &mut all);
    all
}

fn padded_side(outs: &[OutputCol], padded: bool, into: &mut Vec<OutputCol>) {
    for c in outs {
        into.push(if padded && !c.nullable { OutputCol { nullable: true, ..c.clone() } } else { c.clone() });
    }
}

/// `SqlSelect.outputsFrom`: a projection frame's outputs -- declared slots reconciled against their expressions,
/// stars expanding their sources, output-less projections contributing none.
pub fn outputs_from(s: &SqlSelect) -> Result<Vec<OutputCol>, Refusal> {
    let grouped = !s.group_by.is_empty();
    let mut out = Vec::with_capacity(s.projections.len());
    for p in &s.projections {
        match &p.expr {
            SqlExpr::Star { table, .. } => expand_star(&s.from, table.as_deref(), false, None, &mut out)?,
            SqlExpr::StarExcept { table, except, .. } => {
                expand_star(&s.from, table.as_deref(), false, Some(except), &mut out)?
            }
            _ => {
                if let Some(oc) = &p.out {
                    out.push(reconcile_slot(&p.expr, oc, grouped));
                }
            }
        }
    }
    Ok(out)
}

fn expand_star(
    src: &SqlSource,
    table: Option<&str>,
    padded: bool,
    except: Option<&[String]>,
    into: &mut Vec<OutputCol>,
) -> Result<(), Refusal> {
    if let SqlSource::Join { left, right, kind, .. } = src {
        expand_star(left, table, padded || pads_left(*kind), except, into)?;
        return expand_star(right, table, padded || pads_right(*kind), except, into);
    }
    if let Some(t) = table
        && t != source_alias(src)?
    {
        return Ok(());
    }
    for c in source_outputs(src) {
        if except.is_some_and(|ex| ex.contains(&c.name)) {
            continue;
        }
        into.push(if padded && !c.nullable { OutputCol { nullable: true, ..c.clone() } } else { c.clone() });
    }
    Ok(())
}

/// The fact a reducer's node carries (Java's `Reducer` constructor): its promotion over its first argument.
pub fn reducer_fact(r: &Reducer) -> TypeFact {
    reducer_type(r.r#fn, r.args.first().map_or(&TypeFact::Unknown, SqlExpr::r#type))
}
