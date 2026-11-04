// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `MongoDb.call` and its helpers: every function by name -- MongoDB's operator, an emulation over its
//! operators, or a refusal -- in one exhaustive match, each helper Java's private method of the same name, its text
//! and its refusals Java's, in Java's order of evaluation.

use super::json::{self, guarded, guarded_op, op, op1};
use super::{
    G0, G1, G2, MongoDb, POSITION_DIVIDER, RAISE_SENTINEL, THIS, UNFOLDED, VALUE, java_text, of, pc, text, texts,
};
use crate::ansi;
use crate::refusal::{Refusal, arg, tail};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{AggFn, DatePart, Lambda, Scalar, SqlExpr, SqlFn, SqlType, TypeFact};

use super::Pc;
use super::aggregates::{array_reduction, non_null};

/// The day names `$dayOfWeek` numbers from Sunday.
const DAY_NAMES: &[&str] = &["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
/// The month names `$month` numbers from January.
const MONTH_NAMES: &[&str] = &[
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The 128 ASCII characters in code order, as a JSON string: the character-code table.
fn ascii() -> String {
    let table: String = (0u8..128).map(char::from).collect();
    json::string(&table)
}

/// Java's `call(writer, c, parentPrec)`: every function, each by name.
pub fn call(
    d: &MongoDb,
    w: &mut SqlWriter,
    f: SqlFn,
    a: &[SqlExpr],
    r#type: &TypeFact,
    parent_prec: i32,
) -> Result<(), Refusal> {
    let _ = r#type;
    match f {
        // ---- logic: MongoDB's AND and OR read null as false; NOT answers null for null (D7)
        SqlFn::And => op(d, w, "$and", a),
        SqlFn::Or => op(d, w, "$or", a),
        SqlFn::Not => guarded_op(d, w, "$not", a),
        // ---- comparison: three-valued over BSON's total comparisons (D7)
        SqlFn::Equal => guarded_op(d, w, "$eq", a),
        SqlFn::NotEqual => guarded_op(d, w, "$ne", a),
        SqlFn::Less => guarded_op(d, w, "$lt", a),
        SqlFn::LessEqual => guarded_op(d, w, "$lte", a),
        SqlFn::Greater => guarded_op(d, w, "$gt", a),
        SqlFn::GreaterEqual => guarded_op(d, w, "$gte", a),
        SqlFn::IsDistinctFrom | SqlFn::NullSafeNotEqual => op(d, w, "$ne", a),
        SqlFn::NullSafeEqual => op(d, w, "$eq", a),
        SqlFn::Xor => guarded_op(d, w, "$ne", a),
        // ---- arithmetic: MongoDB's operators answer null for a null operand already
        SqlFn::Plus => op(d, w, "$add", a),
        SqlFn::Minus => op(d, w, "$subtract", a),
        SqlFn::Times => op(d, w, "$multiply", a),
        // Pure's divide answers a Float, every dialect dividing as doubles
        SqlFn::Divide => {
            w.append("{\"$divide\": [{\"$toDouble\": ").expr(d, arg(a, 0)?, 0)?;
            w.append("}, {\"$toDouble\": ").expr(d, arg(a, 1)?, 0)?.append("}]}");
            Ok(())
        }
        SqlFn::Mod => floored_mod(d, w, a),
        SqlFn::Rem => {
            w.append("{\"$mod\": [").expr(d, arg(a, 0)?, 0)?.append(", ");
            unfolded(arg(a, 1)?)(d, w)?;
            w.append("]}");
            Ok(())
        }
        SqlFn::Negate => {
            w.append("{\"$multiply\": [").expr(d, arg(a, 0)?, 0)?.append(", -1]}");
            Ok(())
        }
        SqlFn::Abs => op1(d, w, "$abs", arg(a, 0)?),
        SqlFn::IntDivide => {
            w.append("{\"$toLong\": {\"$trunc\": [{\"$divide\": [").expr(d, arg(a, 0)?, 0)?.append(", ");
            w.expr(d, arg(a, 1)?, 0)?.append("]}, 0]}}");
            Ok(())
        }
        // ---- null and membership
        SqlFn::IsNull => null_test(d, w, a, true),
        SqlFn::IsNotNull => null_test(d, w, a, false),
        SqlFn::In => in_list(d, w, a),
        SqlFn::Coalesce => {
            if a.len() == 1 {
                w.expr(d, &a[0], 0)?;
                Ok(())
            } else {
                op(d, w, "$ifNull", a)
            }
        }
        // ---- numbers
        SqlFn::ParseInt => op1(d, w, "$toLong", arg(a, 0)?),
        SqlFn::Sqrt => {
            w.append("{\"$sqrt\": ");
            unfolded(arg(a, 0)?)(d, w)?;
            w.append("}");
            Ok(())
        }
        SqlFn::Cbrt => guarded(
            d,
            w,
            a,
            &text(
                [
                    "{\"$cond\": [{\"$lt\": [",
                    G0,
                    ", 0]}, {\"$multiply\":",
                    " [-1, {\"$pow\": [{\"$abs\": ",
                    G0,
                    "}, 0.3333333333333333]}]}, {\"$pow\": [",
                    G0,
                    ", 0.3333333333333333]}]}",
                ]
                .concat(),
            ),
        ),
        SqlFn::Exp => op1(d, w, "$exp", arg(a, 0)?),
        SqlFn::Ln => op1(d, w, "$ln", arg(a, 0)?),
        SqlFn::Log10 => op1(d, w, "$log10", arg(a, 0)?),
        // a power is a Float, as Pure's pow is
        SqlFn::Pow => guarded(d, w, a, &text(["{\"$pow\": [{\"$toDouble\": ", G0, "}, ", G1, "]}"].concat())),
        SqlFn::Pi => {
            w.append("{\"$numberDouble\": \"3.141592653589793\"}");
            Ok(())
        }
        SqlFn::Sin => op1(d, w, "$sin", arg(a, 0)?),
        SqlFn::Cos => op1(d, w, "$cos", arg(a, 0)?),
        SqlFn::Tan => op1(d, w, "$tan", arg(a, 0)?),
        SqlFn::Asin => domain_guarded(d, w, "$asin", a),
        SqlFn::Acos => domain_guarded(d, w, "$acos", a),
        SqlFn::Atan => op1(d, w, "$atan", arg(a, 0)?),
        SqlFn::Atan2 => op(d, w, "$atan2", a),
        SqlFn::Sinh => op1(d, w, "$sinh", arg(a, 0)?),
        SqlFn::Cosh => op1(d, w, "$cosh", arg(a, 0)?),
        SqlFn::Tanh => op1(d, w, "$tanh", arg(a, 0)?),
        SqlFn::Cot => {
            w.append("{\"$divide\": [1, {\"$tan\": ").expr(d, arg(a, 0)?, 0)?.append("}]}");
            Ok(())
        }
        SqlFn::Radians => op1(d, w, "$degreesToRadians", arg(a, 0)?),
        SqlFn::Degrees => op1(d, w, "$radiansToDegrees", arg(a, 0)?),
        SqlFn::Ceiling => long_of(d, w, "$ceil", a),
        SqlFn::Floor => long_of(d, w, "$floor", a),
        SqlFn::FloorRaw => op1(d, w, "$floor", arg(a, 0)?),
        SqlFn::Round => d.round_half_even(w, a),
        SqlFn::RoundHalfUp => round_half_up(d, w, a),
        SqlFn::Sign => guarded(
            d,
            w,
            a,
            &text(
                ["{\"$cond\": [{\"$gt\": [", G0, ", 0]}, 1, {\"$cond\":", " [{\"$lt\": [", G0, ", 0]}, -1, 0]}]}"]
                    .concat(),
            ),
        ),
        SqlFn::Greatest => op(d, w, "$max", a),
        SqlFn::Least => op(d, w, "$min", a),
        SqlFn::BitAnd | SqlFn::BitOr | SqlFn::BitXor | SqlFn::BitShiftLeft | SqlFn::BitShiftRight => d.bit_op(w, f, a),
        SqlFn::BitNot => op1(d, w, "$bitNot", arg(a, 0)?),
        SqlFn::Hash => d.hash_signed(w, a),
        // ---- strings (a string operator over null errors or answers "", so each is null-guarded)
        SqlFn::Concat => concat_skipping_null(d, w, a),
        SqlFn::ConcatJoin => op(d, w, "$concat", a),
        SqlFn::Length => length(d, w, a),
        SqlFn::Upper => guarded_op(d, w, "$toUpper", a),
        SqlFn::Lower => guarded_op(d, w, "$toLower", a),
        SqlFn::Substring => substring(d, w, a),
        SqlFn::Strpos => guarded(d, w, a, &text(["{\"$add\": [{\"$indexOfCP\": [", G0, ", ", G1, "]}, 1]}"].concat())),
        SqlFn::StartsWith => {
            guarded(d, w, a, &text(["{\"$eq\": [{\"$indexOfCP\": [", G0, ", ", G1, "]}, 0]}"].concat()))
        }
        SqlFn::EndsWith => ends_with(d, w, a),
        SqlFn::Matches => regex_match(d, w, a, false),
        SqlFn::RegexpFullMatch => regex_match(d, w, a, true),
        SqlFn::RegexpExtract => regex_extract(d, w, a),
        SqlFn::RegexpExtractAll => regex_extract_all(d, w, a),
        SqlFn::RegexpReplace => regex_replace(d, w, a),
        SqlFn::Left => guarded(
            d,
            w,
            a,
            &text(
                [
                    "{\"$substrCP\": [",
                    G0,
                    ", 0, {\"$max\":",
                    " [0, {\"$min\": [",
                    G1,
                    ", {\"$strLenCP\": ",
                    G0,
                    "}]}]}]}",
                ]
                .concat(),
            ),
        ),
        SqlFn::Right => right_chars(d, w, a),
        SqlFn::Lpad => pad(d, w, a, true),
        SqlFn::Rpad => pad(d, w, a, false),
        SqlFn::Trim => trim(d, w, "$trim", a),
        SqlFn::Ltrim => trim(d, w, "$ltrim", a),
        SqlFn::Rtrim => trim(d, w, "$rtrim", a),
        SqlFn::Replace => {
            w.append("{\"$replaceAll\": {\"input\": ").expr(d, arg(a, 0)?, 0)?.append(", \"find\": ");
            w.expr(d, arg(a, 1)?, 0)?.append(", \"replacement\": ").expr(d, arg(a, 2)?, 0)?.append("}}");
            Ok(())
        }
        SqlFn::Split => op(d, w, "$split", a),
        SqlFn::SplitPart => split_part(d, w, a, false),
        SqlFn::PureSplitPart => d.split_part_call(w, a),
        SqlFn::Translate => Err(Refusal::capability("translate reached MongoDb, which has no character-map operator")),
        SqlFn::ReverseString => guarded(
            d,
            w,
            a,
            &text(
                [
                    "{\"$reduce\": {\"input\":",
                    " {\"$range\": [0, {\"$strLenCP\": ",
                    G0,
                    "}]}, \"initialValue\": \"\", \"in\":",
                    " {\"$concat\": [{\"$substrCP\": [",
                    G0,
                    ", ",
                    THIS,
                    ", 1]}, ",
                    VALUE,
                    "]}}}",
                ]
                .concat(),
            ),
        ),
        // MongoDB has no character-code operator: the ASCII table spelled as text, a code its position in it
        SqlFn::AsciiCode => guarded(
            d,
            w,
            a,
            &text(
                [
                    "{\"$let\": {\"vars\": {\"c\":",
                    " {\"$indexOfCP\": [",
                    &ascii(),
                    ", {\"$substrCP\": [",
                    G0,
                    ", 0, 1]}]}}, \"in\": {\"$cond\":",
                    " [{\"$lt\": [\"$$c\", 0]}, ",
                    &texts::raise_text(
                        "a character beyond ASCII reached ascii on MongoDb, which has no character-code operator",
                    ),
                    ", \"$$c\"]}}}",
                ]
                .concat(),
            ),
        ),
        SqlFn::Chr => guarded(
            d,
            w,
            a,
            &text(
                [
                    "{\"$cond\": [{\"$or\": [{\"$lt\": [",
                    G0,
                    ", 0]}, {\"$gte\": [",
                    G0,
                    ", 128]}]}, ",
                    &texts::raise_text(
                        "a code beyond ASCII reached char on MongoDb, which has no code-to-character operator",
                    ),
                    ", {\"$substrCP\": [",
                    &ascii(),
                    ", ",
                    G0,
                    ", 1]}]}",
                ]
                .concat(),
            ),
        ),
        SqlFn::UcFirst => first_cased(d, w, a, "$toUpper"),
        SqlFn::LcFirst => first_cased(d, w, a, "$toLower"),
        SqlFn::RepeatStr => guarded(
            d,
            w,
            a,
            &text(
                [
                    "{\"$reduce\": {\"input\": {\"$range\":",
                    " [0, {\"$max\": [0, ",
                    G1,
                    "]}]}, \"initialValue\": \"\", \"in\": {\"$concat\": [",
                    VALUE,
                    ", ",
                    G0,
                    "]}}}",
                ]
                .concat(),
            ),
        ),
        // base64: a text's UTF-8 bytes written as base64; the reverse reads base64, padded to a multiple of four first
        SqlFn::EncodeBase64 => guarded(
            d,
            w,
            a,
            &text(
                [
                    "{\"$convert\": {\"input\":",
                    " {\"$convert\": {\"input\": ",
                    G0,
                    ", \"to\": \"binData\", \"format\": \"utf8\"}},",
                    " \"to\": \"string\", \"format\": \"base64\"}}",
                ]
                .concat(),
            ),
        ),
        SqlFn::DecodeBase64 => guarded(
            d,
            w,
            a,
            &text(
                [
                    "{\"$convert\": {\"input\":",
                    " {\"$convert\": {\"input\": {\"$concat\": [",
                    G0,
                    ", {\"$substrCP\": [\"==\", 0, {\"$mod\":",
                    " [{\"$subtract\": [4, {\"$mod\": [{\"$strLenCP\": ",
                    G0,
                    "}, 4]}]}, 4]}]}]}, \"to\":",
                    " \"binData\", \"format\": \"base64\"}}, \"to\": \"string\", \"format\": \"utf8\"}}",
                ]
                .concat(),
            ),
        ),
        // no string-distance operator: the distances composed over the characters, as folds
        SqlFn::Levenshtein => guarded(d, w, a, &text(texts::levenshtein())),
        SqlFn::JaroWinkler => guarded(d, w, a, &text(texts::jaro_winkler())),
        // the database's draw in [0, 1), per document it is spelled for
        SqlFn::Random => {
            w.append("{\"$rand\": {}}");
            Ok(())
        }
        // no uuid operator: thirty-two random hexadecimal digits in a uuid's five groups
        SqlFn::Guid => {
            w.append(concat!(
                "{\"$let\": {\"vars\": {\"h\": {\"$reduce\": {\"input\": {\"$range\": [0, 32]},",
                " \"initialValue\": \"\", \"in\": {\"$concat\": [\"$$value\", {\"$substrCP\": [\"0123456789abcdef\",",
                " {\"$toInt\": {\"$floor\": {\"$multiply\": [{\"$rand\": {}}, 16]}}}, 1]}]}}}}, \"in\": {\"$concat\":",
                " [{\"$substrCP\": [\"$$h\", 0, 8]}, \"-\", {\"$substrCP\": [\"$$h\", 8, 4]}, \"-\", {\"$substrCP\":",
                " [\"$$h\", 12, 4]}, \"-\", {\"$substrCP\": [\"$$h\", 16, 4]}, \"-\", {\"$substrCP\": [\"$$h\", 20,",
                " 12]}]}}}"
            ));
            Ok(())
        }
        SqlFn::Format => texts::format(d, w, a),
        SqlFn::Md5 | SqlFn::Sha1 | SqlFn::Sha256 => {
            Err(Refusal::capability(format!("{f} reached MongoDb, which has no digest operator")))
        }
        SqlFn::CurrentUserFn => Err(Refusal::capability("currentUserId reached MongoDb, whose pipeline names no user")),
        // ---- dates
        SqlFn::Extract => extract(d, w, a),
        SqlFn::Today => {
            w.append("{\"$dateTrunc\": {\"date\": \"$$NOW\", \"unit\": \"day\"}}");
            Ok(())
        }
        SqlFn::Now => {
            w.append("\"$$NOW\"");
            Ok(())
        }
        SqlFn::DateTruncDay => date_trunc(d, w, arg(a, 0)?, DatePart::Day),
        SqlFn::DateTrunc => {
            let date = arg(a, 1)?;
            date_trunc(d, w, date, part(arg(a, 0)?)?)
        }
        SqlFn::MakeDate | SqlFn::MakeTimestamp => date_from_parts(d, w, a),
        SqlFn::AddInterval | SqlFn::AddIntervalTemporal => date_add(d, w, a),
        SqlFn::DateDiff => date_diff(d, w, a),
        SqlFn::TimeBucket => time_bucket(d, w, a),
        SqlFn::Timezone => timezone(d, w, a),
        SqlFn::DateAtTime => {
            Err(Refusal::capability("a date at a time of day reached MongoDb, which has no time type"))
        }
        SqlFn::EpochNs | SqlFn::FromEpochNs => {
            Err(Refusal::capability(format!("{f} reached MongoDb, whose date holds milliseconds")))
        }
        SqlFn::EpochSeconds => {
            w.append("{\"$divide\": [{\"$toLong\": ").expr(d, arg(a, 0)?, 0)?.append("}, 1000]}");
            Ok(())
        }
        SqlFn::EpochMs => op1(d, w, "$toLong", arg(a, 0)?),
        SqlFn::FromEpochSeconds => {
            w.append("{\"$toDate\": {\"$multiply\": [").expr(d, arg(a, 0)?, 0)?.append(", 1000]}}");
            Ok(())
        }
        SqlFn::FromEpochMs => op1(d, w, "$toDate", arg(a, 0)?),
        SqlFn::Dayname => calendar_name(d, w, a, DAY_NAMES, "$dayOfWeek"),
        SqlFn::Monthname => calendar_name(d, w, a, MONTH_NAMES, "$month"),
        SqlFn::Strftime => texts::strftime(d, w, a),
        SqlFn::Strptime => {
            w.append("{\"$dateFromString\": {\"dateString\": ").expr(d, arg(a, 0)?, 0)?.append(", \"format\": ");
            w.expr(d, arg(a, 1)?, 0)?.append("}}");
            Ok(())
        }
        SqlFn::ParseDate => {
            w.append("{\"$dateFromString\": {\"dateString\": ").expr(d, arg(a, 0)?, 0)?.append("}}");
            Ok(())
        }
        // ---- lists (BSON arrays)
        SqlFn::ListFilter => list_filter(d, w, a),
        SqlFn::ListTransform => list_map(d, w, a),
        SqlFn::ListConcat => list_concat(d, w, a),
        // an element past the end is null, where $arrayElemAt answers nothing
        SqlFn::ListGet => {
            w.append("{\"$ifNull\": [{\"$arrayElemAt\": [").expr(d, arg(a, 0)?, 0)?.append(", {\"$subtract\": [");
            w.expr(d, arg(a, 1)?, 0)?.append(", 1]}]}, null]}");
            Ok(())
        }
        SqlFn::ListPosition => {
            w.append("{\"$let\": {\"vars\": {\"i\": {\"$indexOfArray\": [").expr(d, arg(a, 0)?, 0)?.append(", ");
            w.expr(d, arg(a, 1)?, 0)?;
            w.append("]}}, \"in\": {\"$cond\": [{\"$lt\": [\"$$i\", 0]}, null, {\"$add\": [\"$$i\", 1]}]}}}");
            Ok(())
        }
        SqlFn::ListExists => d.list_exists(w, a),
        SqlFn::ListForAll => d.list_for_all(w, a),
        SqlFn::ListLength | SqlFn::JsonArrayLength => guarded_op(d, w, "$size", a),
        SqlFn::ListFlatten => {
            w.append("{\"$reduce\": {\"input\": ").expr(d, arg(a, 0)?, 0)?;
            w.append(&[", \"initialValue\": [], \"in\": {\"$concatArrays\": [", VALUE, ", ", THIS, "]}}}"].concat());
            Ok(())
        }
        SqlFn::AllDistinct => d.all_distinct(w, a),
        SqlFn::ListZip
        | SqlFn::ListDistinct
        | SqlFn::ListAppend
        | SqlFn::ListSum
        | SqlFn::ListMin
        | SqlFn::ListMax
        | SqlFn::ListAvg
        | SqlFn::ListMedian
        | SqlFn::ListMode
        | SqlFn::ListSort
        | SqlFn::ListSortDesc
        | SqlFn::ListTail
        | SqlFn::ListInit
        | SqlFn::RangeFn
        | SqlFn::ListProduct
        | SqlFn::ListReduce
        | SqlFn::ListSlice
        | SqlFn::RepeatValue
        | SqlFn::ListBoolAnd
        | SqlFn::ListBoolOr
        | SqlFn::ListReverse
        | SqlFn::Typeof => d.list_call(w, f, a),
        SqlFn::Unnest => d.unnest_projection(w, a),
        SqlFn::StructInsert => d.struct_insert(w, a),
        // ---- maps (documents)
        SqlFn::MapFromLists => {
            w.append("{\"$arrayToObject\": {\"$zip\": {\"inputs\": [").list(d, a)?.append("]}}}");
            Ok(())
        }
        // the entries' pairs (first, second) as the document's fields (k, v); no entry, an empty document
        SqlFn::MapFromEntries => {
            w.append("{\"$arrayToObject\": {\"$map\": {\"input\": {\"$ifNull\": [").expr(d, arg(a, 0)?, 0)?;
            w.append(", []]}, \"in\": {\"k\": \"$$this.first\", \"v\": \"$$this.second\"}}}}");
            Ok(())
        }
        SqlFn::MapEmpty => {
            w.append("{\"$literal\": {}}");
            Ok(())
        }
        SqlFn::MapExtract => map_extract(d, w, a),
        SqlFn::MapKeys => map_side(d, w, a, "k"),
        SqlFn::MapValues => map_side(d, w, a, "v"),
        SqlFn::MapConcat => op(d, w, "$mergeObjects", a),
        // ---- variants (a BSON value is the variant)
        SqlFn::ToVariant => d.variant_construct(w, a),
        SqlFn::VariantElements => d.variant_elements(w, a),
        SqlFn::VariantGet => d.variant_get(w, a),
        SqlFn::JsonType => json_type(d, w, a),
        SqlFn::JsonMergePatch => {
            w.append("{\"$arrayToObject\": {\"$filter\": {\"input\": {\"$objectToArray\": {\"$mergeObjects\": [");
            w.expr(d, arg(a, 0)?, 0)?.append(", ").expr(d, arg(a, 1)?, 0)?;
            w.append("]}}, \"cond\": {\"$ne\": [\"$$this.v\", null]}}}}");
            Ok(())
        }
        SqlFn::JsonPretty => {
            Err(Refusal::capability("a pretty-printed JSON text reached MongoDb, which has no JSON text operator"))
        }
        SqlFn::BoolToText => op1(d, w, "$toString", arg(a, 0)?),
        SqlFn::FloatCanon => ansi::float_canon_by_text(d, w, a, parent_prec),
        SqlFn::FloatPrint => ansi::float_print_by_text(d, w, a, parent_prec),
        // ---- raising
        SqlFn::Error => raise(d, w, a),
    }
}

// ---- the arms' helpers, each one spelling

/// `((a mod b) + b) mod b`: the always-positive modulus.
fn floored_mod(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let b = unfolded(arg(a, 1)?);
    w.append("{\"$mod\": [{\"$add\": [{\"$mod\": [").expr(d, arg(a, 0)?, 0)?.append(", ");
    b(d, w)?;
    w.append("]}, ");
    b(d, w)?;
    w.append("]}, ");
    b(d, w)?;
    w.append("]}");
    Ok(())
}

/// A literal operand of an operator that raises on its value read through the row ([`UNFOLDED`], its length 0
/// added), so the server's optimizer cannot fold it; any other operand as it is.
pub(crate) fn unfolded(x: &SqlExpr) -> Pc<'_> {
    if matches!(x, SqlExpr::IntLit { .. } | SqlExpr::FloatLit { .. } | SqlExpr::DecimalLit { .. }) {
        return pc(move |d, w| {
            w.append("{\"$add\": [").expr(d, x, 0)?.append(&[", {\"$strLenCP\": ", UNFOLDED, "}]}"].concat());
            Ok(())
        });
    }
    of(x)
}

/// Whether `x` is null or missing, by its type: `$eq` compares a missing value unequal to null, and `x` is read once.
fn null_test(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr], is_null: bool) -> Result<(), Refusal> {
    w.append(if is_null { "" } else { "{\"$not\": [" }).append("{\"$in\": [{\"$type\": ").expr(d, arg(a, 0)?, 0)?;
    w.append("}, [\"missing\", \"null\"]]}");
    w.append(if is_null { "" } else { "]}" });
    Ok(())
}

/// `x IN (a, b, ...)`: null for a null needle.
fn in_list(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let rest = tail(a, 1)?;
    let needle = arg(a, 0)?;
    let body = pc(move |d, x| {
        x.append(&["{\"$in\": [", G0, ", ["].concat()).list(d, rest)?.append("]]}");
        Ok(())
    });
    guarded(d, w, std::slice::from_ref(needle), &body)
}

/// NaN outside [-1, 1], the engine's answer, where MongoDB's operator errors.
fn domain_guarded(d: &MongoDb, w: &mut SqlWriter, op: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
    let body = [
        "{\"$cond\": [{\"$and\": [{\"$gte\": [",
        G0,
        ", -1]},",
        " {\"$lte\": [",
        G0,
        ", 1]}]}, {\"",
        op,
        "\": ",
        G0,
        "}, {\"$numberDouble\": \"NaN\"}]}",
    ]
    .concat();
    guarded(d, w, a, &text(body))
}

/// `ceil`/`floor` as an integer.
fn long_of(d: &MongoDb, w: &mut SqlWriter, op: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$toLong\": {\"").append(op).append("\": ").expr(d, arg(a, 0)?, 0)?.append("}}");
    Ok(())
}

/// `$round` rounds half to even, Pure's banker's rounding (Java's `roundHalfEven` override).
pub fn round_half_even(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$round\": [").expr(d, arg(a, 0)?, 0)?.append(", ");
    scale_or_zero(d, w, a)?;
    w.append("]}");
    Ok(())
}

fn scale_or_zero(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    if a.len() > 1 {
        w.expr(d, &a[1], 0)?;
    } else {
        w.append("0");
    }
    Ok(())
}

/// Half away from zero at a scale: `trunc(x * 10^s + sign(x) / 2) / 10^s`.
fn round_half_up(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$let\": {\"vars\": {\"x\": ").expr(d, arg(a, 0)?, 0)?.append(", \"f\": {\"$pow\": [10, ");
    scale_or_zero(d, w, a)?;
    w.append(concat!(
        "]}}, \"in\": {\"$divide\": [{\"$trunc\": [{\"$add\": [{\"$multiply\": [\"$$x\",",
        " \"$$f\"]}, {\"$cond\": [{\"$lt\": [\"$$x\", 0]}, -0.5, 0.5]}]}, 0]}, \"$$f\"]}}}"
    ));
    Ok(())
}

/// `$bitAnd`, `$bitOr`, `$bitXor` over integers; a shift by arithmetic (Java's `bitOp` override).
pub fn bit_op(d: &MongoDb, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
    match f {
        SqlFn::BitAnd => op(d, w, "$bitAnd", a),
        SqlFn::BitOr => op(d, w, "$bitOr", a),
        SqlFn::BitXor => op(d, w, "$bitXor", a),
        // no shift operator: the product with, or the floored quotient by, the power of two -- in Decimal128
        SqlFn::BitShiftLeft => guarded(
            d,
            w,
            a,
            &text(
                [
                    "{\"$toLong\": {\"$multiply\": [{\"$toDecimal\": ",
                    G0,
                    "}, {\"$pow\": [{\"$toDecimal\": 2}, ",
                    G1,
                    "]}]}}",
                ]
                .concat(),
            ),
        ),
        SqlFn::BitShiftRight => guarded(
            d,
            w,
            a,
            &text(
                [
                    "{\"$toLong\": {\"$floor\":",
                    " {\"$divide\": [{\"$toDecimal\": ",
                    G0,
                    "}, {\"$pow\": [{\"$toDecimal\": 2}, ",
                    G1,
                    "]}]}}}",
                ]
                .concat(),
            ),
        ),
        other => Err(Refusal::illegal_state(format!("not a bit operation: {other}"))),
    }
}

/// A list's slice by the platform's bounds (`LIST_SLICE(list, begin, end)`: 1-based, both ends inclusive, a negative
/// bound counted from the end), resolved against the size first, an empty range the empty array.
fn list_slice(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let body = [
        "{\"$let\": {\"vars\": {\"n\": {\"$size\": ",
        G0,
        "}}, \"in\": {\"$let\": {\"vars\": {\"b\": {\"$max\": [1, {\"$cond\": [{\"$lt\": [",
        G1,
        ", 0]}, {\"$add\": [\"$$n\", ",
        G1,
        ", 1]}, ",
        G1,
        "]}]}, \"e\": {\"$min\": [\"$$n\",",
        " {\"$cond\": [{\"$lt\": [",
        G2,
        ", 0]}, {\"$add\": [\"$$n\", ",
        G2,
        ", 1]}, ",
        G2,
        "]}]}}, \"in\": {\"$cond\": [{\"$lt\": [\"$$e\", \"$$b\"]}, [], {\"$slice\": [",
        G0,
        ", {\"$subtract\": [\"$$b\", 1]}, {\"$add\": [{\"$subtract\": [\"$$e\", \"$$b\"]}, 1]}]}]}}}}}",
    ]
    .concat();
    guarded(d, w, a, &text(body))
}

/// A null operand is the empty list, as the platform's list concatenation reads it.
fn list_concat(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$concatArrays\": [");
    for (i, x) in a.iter().enumerate() {
        w.append(if i > 0 { ", " } else { "" }).append("{\"$ifNull\": [").expr(d, x, 0)?.append(", []]}");
    }
    w.append("]}");
    Ok(())
}

/// Null-skipping concat: each argument's null is the empty string, nested concats flattened.
fn concat_skipping_null(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$concat\": [");
    for (i, x) in ansi::flatten_concat(a).into_iter().enumerate() {
        if i > 0 {
            w.append(", ");
        }
        w.append("{\"$ifNull\": [").expr(d, x, 0)?.append(", \"\"]}");
    }
    w.append("]}");
    Ok(())
}

/// The length in code points; a number's, of its text.
fn length(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let a0 = arg(a, 0)?;
    let is_text = super::typed_as(a0, Scalar::Varchar) || matches!(a0, SqlExpr::StringLit { .. });
    let len = if is_text {
        ["{\"$strLenCP\": ", G0, "}"].concat()
    } else {
        ["{\"$strLenCP\": {\"$toString\": ", G0, "}}"].concat()
    };
    guarded(d, w, a, &text(["{\"$cond\": [{\"$eq\": [", G0, ", null]}, null, ", &len, "]}"].concat()))
}

/// `timeBucket(unit, quantity, date)`: the engine's bucket by arithmetic, from the origin 1970-01-01 (a week's
/// 1969-12-29); a month, a quarter or a year by the months since 1970-01; a sub-millisecond unit refused by name.
fn time_bucket(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let unit = part(arg(a, 0)?)?;
    let unit_millis: Option<i64> = match unit {
        DatePart::Week => Some(604_800_000),
        DatePart::Day => Some(86_400_000),
        DatePart::Hour => Some(3_600_000),
        DatePart::Minute => Some(60_000),
        DatePart::Second => Some(1_000),
        DatePart::Millisecond => Some(1),
        DatePart::Year | DatePart::Quarter | DatePart::Month => None,
        DatePart::Microsecond | DatePart::Nanosecond => {
            return Err(Refusal::capability(format!(
                "a time bucket by {unit}s reached MongoDb, whose dates hold milliseconds"
            )));
        }
        DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
            return Err(Refusal::illegal_state(format!(
                "a time bucket by {unit}, which is no duration (DateShifts.durationPart)"
            )));
        }
    };
    // a literal finer than a millisecond is bucketed at the millisecond
    let a2 = arg(a, 2)?;
    let date = match a2 {
        SqlExpr::TimestampLit { iso, .. } => {
            SqlExpr::TimestampLit { iso: to_millisecond(iso), r#type: crate::typing::scalar(Scalar::Timestamp) }
        }
        other => other.clone(),
    };
    let args = vec![arg(a, 1)?.clone(), date];
    if let Some(unit_millis) = unit_millis {
        let origin: i64 = if unit == DatePart::Week { -259_200_000 } else { 0 };
        let body = [
            "{\"$let\": {\"vars\": {\"w\": {\"$multiply\": [",
            G0,
            ", ",
            &unit_millis.to_string(),
            "]}, \"t\": {\"$subtract\": [{\"$toLong\": ",
            G1,
            "}, ",
            &origin.to_string(),
            "]}}, \"in\": {\"$toDate\": {\"$add\": [",
            &origin.to_string(),
            ", {\"$multiply\": [{\"$floor\": {\"$divide\":",
            " [\"$$t\", \"$$w\"]}}, \"$$w\"]}]}}}}",
        ]
        .concat();
        return guarded(d, w, &args, &text(body));
    }
    let per_unit = match unit {
        DatePart::Year => 12,
        DatePart::Quarter => 3,
        DatePart::Month => 1,
        other => return Err(Refusal::illegal_state(format!("a time bucket by {other} counts no months"))),
    };
    let body = [
        "{\"$let\": {\"vars\": {\"w\": {\"$multiply\": [",
        G0,
        ", ",
        &per_unit.to_string(),
        "]}, \"p\": {\"$dateToParts\": {\"date\": ",
        G1,
        "}}}, \"in\": {\"$let\": {\"vars\":",
        " {\"b\": {\"$multiply\": [{\"$floor\": {\"$divide\": [{\"$add\": [{\"$multiply\": [{\"$subtract\":",
        " [\"$$p.year\", 1970]}, 12]}, {\"$subtract\": [\"$$p.month\", 1]}]}, \"$$w\"]}}, \"$$w\"]}}, \"in\":",
        " {\"$let\": {\"vars\": {\"y\": {\"$floor\": {\"$divide\": [\"$$b\", 12]}}}, \"in\": {\"$dateFromParts\":",
        " {\"year\": {\"$add\": [1970, \"$$y\"]}, \"month\": {\"$add\": [{\"$subtract\": [\"$$b\", {\"$multiply\":",
        " [\"$$y\", 12]}]}, 1]}, \"day\": 1}}}}}}}}",
    ]
    .concat();
    guarded(d, w, &args, &text(body))
}

/// The ISO text with its fraction of a second cut to three digits (Java's `(\.[0-9]{3})[0-9]+` found once).
fn to_millisecond(iso: &str) -> String {
    let b = iso.as_bytes();
    for start in 0..b.len() {
        if b[start] != b'.' {
            continue;
        }
        let mut end = start + 1;
        while end < b.len() && b[end].is_ascii_digit() {
            end += 1;
        }
        if end - (start + 1) >= 4 {
            return format!("{}{}{}", &iso[..start], &iso[start..start + 4], &iso[end..]);
        }
    }
    iso.to_owned()
}

/// `timezone(zone, v)`: UTC is the value itself; another zone converts an instant to its wall time there, and a wall
/// time there to its instant -- the value's kind decides.
fn timezone(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    if let SqlExpr::StringLit { value, .. } = arg(a, 0)?
        && value == "UTC"
    {
        w.expr(d, arg(a, 1)?, 0)?;
        return Ok(());
    }
    let instant = instant_valued(arg(a, 1)?);
    let parts =
        ["{\"$dateToParts\": {\"date\": \"$$d\"", if instant { ", \"timezone\": \"$$z\"" } else { "" }, "}}"].concat();
    w.append("{\"$let\": {\"vars\": {\"d\": ").expr(d, arg(a, 1)?, 0)?.append(", \"z\": ").expr(d, arg(a, 0)?, 0)?;
    w.append(
        &[
            "}, \"in\": {\"$let\": {\"vars\": {\"p\": ",
            &parts,
            "}, \"in\":",
            " {\"$dateFromParts\": {\"year\": \"$$p.year\", \"month\": \"$$p.month\", \"day\": \"$$p.day\",",
            " \"hour\": \"$$p.hour\", \"minute\": \"$$p.minute\", \"second\": \"$$p.second\",",
            " \"millisecond\": \"$$p.millisecond\"",
            if instant { "" } else { ", \"timezone\": \"$$z\"" },
            "}}}}}}",
        ]
        .concat(),
    );
    Ok(())
}

/// Whether the value is an instant: typed as a zoned timestamp, cast to one, or a value UTC is pinned on.
fn instant_valued(e: &SqlExpr) -> bool {
    super::typed_as(e, Scalar::Timestamptz)
        || matches!(e, SqlExpr::Cast { target: SqlType::Scalar(Scalar::Timestamptz), .. })
        || matches!(e, SqlExpr::Call { r#fn: SqlFn::Timezone, args, .. }
            if matches!(args.first(), Some(SqlExpr::StringLit { value, .. }) if value == "UTC"))
}

/// 1-based SQL substring, its start and length clamped at zero as the engine's substring is.
fn substring(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let len = if a.len() > 2 { ["{\"$max\": [0, ", G2, "]}"].concat() } else { ["{\"$strLenCP\": ", G0, "}"].concat() };
    let body =
        ["{\"$substrCP\": [", G0, ", {\"$max\": [0,", " {\"$subtract\": [", G1, ", 1]}]}, ", &len, "]}"].concat();
    guarded(d, w, a, &text(body))
}

fn ends_with(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let body = [
        "{\"$let\": {\"vars\": {\"ls\": {\"$strLenCP\": ",
        G0,
        "}, \"lp\": {\"$strLenCP\": ",
        G1,
        "}}, \"in\": {\"$and\": [{\"$gte\": [\"$$ls\", \"$$lp\"]},",
        " {\"$eq\": [{\"$substrCP\": [",
        G0,
        ", {\"$subtract\": [\"$$ls\", \"$$lp\"]}, \"$$lp\"]}, ",
        G1,
        "]}]}}}",
    ]
    .concat();
    guarded(d, w, a, &text(body))
}

/// `$regexMatch`; a full match anchors the pattern.
fn regex_match(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr], full: bool) -> Result<(), Refusal> {
    let regex = if full { ["{\"$concat\": [\"^(?:\", ", G1, ", \")$\"]}"].concat() } else { G1.to_owned() };
    guarded(d, w, a, &text(["{\"$regexMatch\": {\"input\": ", G0, ", \"regex\": ", &regex, "}}"].concat()))
}

/// `regexp_extract(s, pattern[, group])`: the group's capture, the whole match for group 0 or none; the empty string
/// when nothing matches.
fn regex_extract(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let found = if a.len() > 2 {
        [
            "{\"$cond\": [{\"$eq\": [",
            G2,
            ", 0]}, \"$$m.match\", {\"$ifNull\":",
            " [{\"$arrayElemAt\": [\"$$m.captures\", {\"$subtract\": [",
            G2,
            ", 1]}]}, \"\"]}]}",
        ]
        .concat()
    } else {
        "\"$$m.match\"".to_owned()
    };
    let body = [
        "{\"$let\": {\"vars\": {\"m\": {\"$regexFind\":",
        " {\"input\": ",
        G0,
        ", \"regex\": ",
        G1,
        "}}}, \"in\": {\"$cond\": [{\"$eq\": [\"$$m\",",
        " null]}, \"\", ",
        &found,
        "]}}}",
    ]
    .concat();
    guarded(d, w, a, &text(body))
}

fn regex_extract_all(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let each = if a.len() > 2 {
        [
            "{\"$cond\": [{\"$eq\": [",
            G2,
            ", 0]}, \"$$this.match\", {\"$arrayElemAt\":",
            " [\"$$this.captures\", {\"$subtract\": [",
            G2,
            ", 1]}]}]}",
        ]
        .concat()
    } else {
        "\"$$this.match\"".to_owned()
    };
    let body = [
        "{\"$map\": {\"input\": {\"$regexFindAll\": {\"input\": ",
        G0,
        ", \"regex\": ",
        G1,
        "}}, \"in\": ",
        &each,
        "}}",
    ]
    .concat();
    guarded(d, w, a, &text(body))
}

/// `regexp_replace(s, pattern, replacement, 'g' | '')`: the matches found (`$regexFindAll`, or the first alone) and
/// the text spliced, a `$N` in the replacement the match's Nth capture.
fn regex_replace(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let all = a.len() > 3 && matches!(&a[3], SqlExpr::StringLit { value, .. } if value.contains('g'));
    let matches = if all {
        ["{\"$regexFindAll\": {\"input\": ", G0, ", \"regex\": ", G1, "}}"].concat()
    } else {
        [
            "{\"$let\": {\"vars\": {\"m\": {\"$regexFind\": {\"input\": ",
            G0,
            ", \"regex\": ",
            G1,
            "}}},",
            " \"in\": {\"$cond\": [{\"$eq\": [\"$$m\", null]}, [], [\"$$m\"]]}}}",
        ]
        .concat()
    };
    // the replacement's capture references: those a literal replacement spells, else any of $1 to $9
    let a2 = arg(a, 2)?;
    let mut groups = Vec::new();
    for n in (1..=9).rev() {
        let spelled = match a2 {
            SqlExpr::StringLit { value, .. } => value.contains(&format!("${n}")),
            _ => true,
        };
        if spelled {
            groups.push(n);
        }
    }
    let mut replacement = G2.to_owned();
    for n in groups {
        // the reference under $literal: a text beginning with $ is otherwise a field path
        replacement = [
            "{\"$replaceAll\": {\"input\": ",
            &replacement,
            ", \"find\": {\"$literal\": \"$",
            &n.to_string(),
            "\"},",
            " \"replacement\": {\"$ifNull\": [{\"$arrayElemAt\": [\"$$this.captures\", ",
            &(n - 1).to_string(),
            "]}, \"\"]}}}",
        ]
        .concat();
    }
    let body = [
        "{\"$let\": {\"vars\": {\"r\": {\"$reduce\": {\"input\": ",
        &matches,
        ", \"initialValue\": {\"t\": \"\", \"p\": 0}, \"in\": {\"t\": {\"$concat\": [\"$$value.t\",",
        " {\"$substrCP\": [",
        G0,
        ", \"$$value.p\", {\"$subtract\": [\"$$this.idx\", \"$$value.p\"]}]}, ",
        &replacement,
        "]}, \"p\": {\"$add\": [\"$$this.idx\", {\"$strLenCP\": \"$$this.match\"}]}}}}}, \"in\":",
        " {\"$concat\": [\"$$r.t\", {\"$substrCP\": [",
        G0,
        ", \"$$r.p\", {\"$subtract\": [{\"$strLenCP\": ",
        G0,
        "}, \"$$r.p\"]}]}]}}}",
    ]
    .concat();
    guarded(d, w, a, &text(body))
}

fn right_chars(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let body = [
        "{\"$let\": {\"vars\": {\"n\": {\"$max\": [0, {\"$min\":",
        " [",
        G1,
        ", {\"$strLenCP\": ",
        G0,
        "}]}]}}, \"in\": {\"$substrCP\": [",
        G0,
        ", {\"$subtract\": [{\"$strLenCP\": ",
        G0,
        "}, \"$$n\"]}, \"$$n\"]}}}",
    ]
    .concat();
    guarded(d, w, a, &text(body))
}

/// `lpad`/`rpad`: the string cut to the length, or the pad text repeated and cut to fill it.
fn pad(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr], left: bool) -> Result<(), Refusal> {
    let pad_text = if a.len() > 2 { G2 } else { "\" \"" };
    let fill = [
        "{\"$substrCP\": [{\"$reduce\": {\"input\": {\"$range\": [0, \"$$k\"]}, \"initialValue\": \"\",",
        " \"in\": {\"$concat\": [",
        VALUE,
        ", ",
        pad_text,
        "]}}}, 0, \"$$k\"]}",
    ]
    .concat();
    let padded = if left {
        ["{\"$concat\": [", &fill, ", ", G0, "]}"].concat()
    } else {
        ["{\"$concat\": [", G0, ", ", &fill, "]}"].concat()
    };
    let body = [
        "{\"$let\": {\"vars\": {\"k\": {\"$subtract\": [",
        G1,
        ", {\"$strLenCP\": ",
        G0,
        "}]}}, \"in\": {\"$cond\": [{\"$lte\": [\"$$k\", 0]}, {\"$substrCP\":",
        " [",
        G0,
        ", 0, {\"$max\": [0, ",
        G1,
        "]}]}, ",
        &padded,
        "]}}}",
    ]
    .concat();
    guarded(d, w, a, &text(body))
}

fn trim(d: &MongoDb, w: &mut SqlWriter, op: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"").append(op).append("\": {\"input\": ").expr(d, arg(a, 0)?, 0)?;
    if a.len() > 1 {
        w.append(", \"chars\": ").expr(d, &a[1], 0)?;
    }
    w.append("}}");
    Ok(())
}

/// The n-th (1-based) part of a split; dropping empty parts for Pure's splitPart.
pub fn split_part(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr], drop_empty: bool) -> Result<(), Refusal> {
    let parts = if drop_empty {
        ["{\"$filter\": {\"input\": {\"$split\": [", G0, ", ", G1, "]}, \"cond\":", " {\"$ne\": [", THIS, ", \"\"]}}}"]
            .concat()
    } else {
        ["{\"$split\": [", G0, ", ", G1, "]}"].concat()
    };
    let body = [
        "{\"$let\": {\"vars\": {\"p\": ",
        &parts,
        "}, \"in\":",
        " {\"$cond\": [{\"$gt\": [",
        G2,
        ", {\"$size\": \"$$p\"}]}, null, {\"$arrayElemAt\": [\"$$p\",",
        " {\"$subtract\": [",
        G2,
        ", 1]}]}]}}}",
    ]
    .concat();
    guarded(d, w, a, &text(body))
}

fn first_cased(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr], op: &str) -> Result<(), Refusal> {
    let body = [
        "{\"$concat\": [{\"",
        op,
        "\": {\"$substrCP\": [",
        G0,
        ", 0, 1]}}, {\"$substrCP\": [",
        G0,
        ", 1, {\"$strLenCP\": ",
        G0,
        "}]}]}",
    ]
    .concat();
    guarded(d, w, a, &text(body))
}

/// The part a date call's first argument names (the base's `part`, its message the record's whole text).
pub(crate) fn part(e: &SqlExpr) -> Result<DatePart, Refusal> {
    match e {
        SqlExpr::DatePartLit { part, .. } => Ok(*part),
        other => Err(Refusal::illegal_state(format!(
            "a date call's first argument is its part, not {}: the lowering's bug",
            java_text::expr(other)
        ))),
    }
}

/// A date part: MongoDB's operator for it; a part finer than a millisecond has none.
fn extract(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let p = part(arg(a, 0)?)?;
    let date = arg(a, 1)?;
    match p {
        DatePart::Year => op1(d, w, "$year", date),
        DatePart::Quarter => {
            w.append("{\"$toLong\": {\"$ceil\": {\"$divide\": [{\"$month\": ").expr(d, date, 0)?.append("}, 3]}}}");
            Ok(())
        }
        DatePart::Month => op1(d, w, "$month", date),
        DatePart::Week => op1(d, w, "$isoWeek", date),
        DatePart::Day => op1(d, w, "$dayOfMonth", date),
        DatePart::Hour => op1(d, w, "$hour", date),
        DatePart::Minute => op1(d, w, "$minute", date),
        DatePart::Second => op1(d, w, "$second", date),
        DatePart::Millisecond => op1(d, w, "$millisecond", date),
        DatePart::DayOfYear => op1(d, w, "$dayOfYear", date),
        DatePart::IsoDayOfWeek => op1(d, w, "$isoDayOfWeek", date),
        DatePart::Microsecond | DatePart::Nanosecond => {
            Err(Refusal::capability(format!("a {p} reached MongoDb, whose date holds milliseconds")))
        }
    }
}

/// A date part as `$dateTrunc`'s, `$dateAdd`'s and `$dateDiff`'s unit; weeks start Monday.
fn unit(p: DatePart) -> Result<&'static str, Refusal> {
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
        DatePart::Microsecond | DatePart::Nanosecond | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
            return Err(Refusal::capability(format!("a {p} unit reached MongoDb, which has no such date unit")));
        }
    })
}

fn date_trunc(d: &MongoDb, w: &mut SqlWriter, date: &SqlExpr, p: DatePart) -> Result<(), Refusal> {
    w.append("{\"$dateTrunc\": {\"date\": ").expr(d, date, 0)?.append(", \"unit\": \"");
    w.append(unit(p)?).append("\", \"startOfWeek\": \"monday\"}}");
    Ok(())
}

/// `(unit, amount, date)`: `$dateAdd`.
fn date_add(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$dateAdd\": {\"startDate\": ").expr(d, arg(a, 2)?, 0)?.append(", \"unit\": \"");
    w.append(unit(part(arg(a, 0)?)?)?).append("\", \"amount\": {\"$toLong\": ").expr(d, arg(a, 1)?, 0)?;
    w.append("}}}");
    Ok(())
}

/// `(unit, start, end)`: `$dateDiff`, the unit boundaries crossed.
fn date_diff(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$dateDiff\": {\"startDate\": ").expr(d, arg(a, 1)?, 0)?.append(", \"endDate\": ");
    w.expr(d, arg(a, 2)?, 0)?.append(", \"unit\": \"").append(unit(part(arg(a, 0)?)?)?);
    w.append("\", \"startOfWeek\": \"monday\"}}");
    Ok(())
}

/// `(year, month, day[, hour, minute, second])`: `$dateFromParts`, a fractional second split into its milliseconds.
fn date_from_parts(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$dateFromParts\": {\"year\": ").expr(d, arg(a, 0)?, 0)?.append(", \"month\": ");
    w.expr(d, arg(a, 1)?, 0)?.append(", \"day\": ").expr(d, arg(a, 2)?, 0)?;
    if a.len() == 6 {
        w.append(", \"hour\": ").expr(d, &a[3], 0)?.append(", \"minute\": ").expr(d, &a[4], 0)?;
        w.append(", \"second\": {\"$toLong\": {\"$floor\": ").expr(d, &a[5], 0)?;
        w.append("}}, \"millisecond\": {\"$toLong\": {\"$round\": [{\"$multiply\": [{\"$mod\": [");
        w.expr(d, &a[5], 0)?.append(", 1]}, 1000]}, 0]}}");
    } else if a.len() != 3 {
        return Err(Refusal::illegal_state(format!("a date from {} parts", a.len())));
    }
    w.append("}}");
    Ok(())
}

/// A day's or a month's name, from the calendar operator's 1-based number.
fn calendar_name(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr], names: &[&str], op: &str) -> Result<(), Refusal> {
    w.append("{\"$arrayElemAt\": [[");
    for (i, n) in names.iter().enumerate() {
        w.append(if i > 0 { ", " } else { "" }).append(&json::string(n));
    }
    w.append("], {\"$subtract\": [{\"").append(op).append("\": ").expr(d, arg(a, 0)?, 0)?.append("}, 1]}]}");
    Ok(())
}

/// A list operator's lambda.
fn lambda_of(e: &SqlExpr) -> Result<&Lambda, Refusal> {
    match e {
        SqlExpr::Lambda(l) => Ok(l),
        other => Err(Refusal::illegal_state(format!(
            "a list operator's second argument is a lambda, not {}",
            java_text::expr(other)
        ))),
    }
}

/// The lambda's first parameter (Java's `l.params().get(0)`).
fn param(l: &Lambda, i: usize) -> Result<&str, Refusal> {
    l.params.get(i).map(String::as_str).ok_or_else(|| Refusal {
        class: Refusal::INDEX_OUT_OF_BOUNDS,
        message: format!("Index {i} out of bounds for length {}", l.params.len()),
    })
}

/// `$filter` over the list, the lambda's parameter its variable.
fn list_filter(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let l = lambda_of(arg(a, 1)?)?;
    if l.params.len() == 2 {
        // the pairs kept, each read back to its element (a null list staying null)
        w.append("{\"$let\": {\"vars\": {\"k\": ");
        indexed(d, w, "$filter", "cond", arg(a, 0)?, l)?;
        w.append(concat!(
            "}, \"in\": {\"$cond\": [{\"$eq\": [\"$$k\", null]}, null, {\"$map\": {\"input\":",
            " \"$$k\", \"in\": {\"$arrayElemAt\": [\"$$this\", 0]}}}]}}}"
        ));
        return Ok(());
    }
    let v = json::variable(param(l, 0)?);
    w.append("{\"$filter\": {\"input\": ").expr(d, arg(a, 0)?, 0)?.append(", \"as\": ").append(&json::string(&v));
    w.append(", \"cond\": ");
    d.with_lambda(w, l, &[v], &of(&l.body))?;
    w.append("}}");
    Ok(())
}

/// A list operator over a two-parameter lambda (the element, its 1-based position): the list zipped with its
/// positions, each pair's sides bound to the parameters.
fn indexed(
    d: &MongoDb,
    w: &mut SqlWriter,
    op: &str,
    body_key: &str,
    list: &SqlExpr,
    l: &Lambda,
) -> Result<(), Refusal> {
    let e = json::variable(param(l, 0)?);
    let i = json::variable(param(l, 1)?);
    // a null list stays null, as the SQL engines' list lambdas answer
    w.append("{\"$let\": {\"vars\": {\"l\": ").expr(d, list, 0)?.append("}, \"in\": {\"$cond\": [{\"$eq\":");
    w.append(" [\"$$l\", null]}, null, {\"").append(op).append("\": {\"input\": {\"$zip\": {\"inputs\": [\"$$l\",");
    w.append(" {\"$range\": [1, {\"$add\": [{\"$size\": \"$$l\"}, 1]}]}]}}, \"as\": \"p\", \"").append(body_key);
    w.append("\": {\"$let\": {\"vars\": {").append(&json::string(&e));
    w.append(": {\"$arrayElemAt\": [\"$$p\", 0]}, ").append(&json::string(&i));
    w.append(": {\"$arrayElemAt\": [\"$$p\", 1]}}, \"in\": ");
    d.with_lambda(w, l, &[e, i], &of(&l.body))?;
    w.append("}}}}]}}}");
    Ok(())
}

/// `$map` over the list.
fn list_map(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let l = lambda_of(arg(a, 1)?)?;
    if l.params.len() == 2 {
        return indexed(d, w, "$map", "in", arg(a, 0)?, l);
    }
    let v = json::variable(param(l, 0)?);
    w.append("{\"$map\": {\"input\": ").expr(d, arg(a, 0)?, 0)?.append(", \"as\": ").append(&json::string(&v));
    w.append(", \"in\": ");
    d.with_lambda(w, l, &[v], &of(&l.body))?;
    w.append("}}");
    Ok(())
}

/// `exists` and `forAll` over the list (Java's `quantified`): the lambda mapped, `op` over the booleans.
pub fn quantified(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr], op: &str) -> Result<(), Refusal> {
    let l = lambda_of(arg(a, 1)?)?;
    let v = json::variable(param(l, 0)?);
    w.append("{\"").append(op).append("\": [{\"$map\": {\"input\": {\"$ifNull\": [").expr(d, arg(a, 0)?, 0)?;
    w.append(", []]}, \"as\": ").append(&json::string(&v)).append(", \"in\": ");
    d.with_lambda(w, l, &[v], &of(&l.body))?;
    w.append("}}]}");
    Ok(())
}

/// The list functions beyond the lambda family, each over BSON's array operators (Java's `listCall` override).
pub fn list_call(d: &MongoDb, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
    match f {
        SqlFn::ListZip => {
            w.append("{\"$zip\": {\"inputs\": [").list(d, a)?.append("]}}");
            Ok(())
        }
        SqlFn::ListDistinct => {
            w.append("{\"$setUnion\": [").expr(d, arg(a, 0)?, 0)?.append("]}");
            Ok(())
        }
        SqlFn::ListAppend => {
            w.append("{\"$concatArrays\": [").expr(d, arg(a, 0)?, 0)?.append(", [").expr(d, arg(a, 1)?, 0)?;
            w.append("]]}");
            Ok(())
        }
        SqlFn::ListSum => op1(d, w, "$sum", arg(a, 0)?),
        SqlFn::ListMin => op1(d, w, "$min", arg(a, 0)?),
        SqlFn::ListMax => op1(d, w, "$max", arg(a, 0)?),
        SqlFn::ListAvg => op1(d, w, "$avg", arg(a, 0)?),
        SqlFn::ListMedian => list_reduced(d, w, a, AggFn::Median),
        SqlFn::ListMode => list_reduced(d, w, a, AggFn::Mode),
        SqlFn::ListSort => {
            w.append("{\"$sortArray\": {\"input\": ").expr(d, arg(a, 0)?, 0)?.append(", \"sortBy\": 1}}");
            Ok(())
        }
        SqlFn::ListSortDesc => {
            w.append("{\"$sortArray\": {\"input\": ").expr(d, arg(a, 0)?, 0)?.append(", \"sortBy\": -1}}");
            Ok(())
        }
        SqlFn::ListTail => {
            let body = [
                "{\"$cond\": [{\"$gt\":",
                " [{\"$size\": ",
                G0,
                "}, 1]}, {\"$slice\": [",
                G0,
                ", 1, {\"$subtract\": [{\"$size\": ",
                G0,
                "}, 1]}]}, []]}",
            ]
            .concat();
            guarded(d, w, std::slice::from_ref(arg(a, 0)?), &text(body))
        }
        SqlFn::ListInit => {
            let body = [
                "{\"$cond\": [{\"$gt\":",
                " [{\"$size\": ",
                G0,
                "}, 1]}, {\"$slice\": [",
                G0,
                ", 0, {\"$subtract\": [{\"$size\": ",
                G0,
                "}, 1]}]}, []]}",
            ]
            .concat();
            guarded(d, w, std::slice::from_ref(arg(a, 0)?), &text(body))
        }
        // range(n) counts from 0 ($range takes a start)
        SqlFn::RangeFn => {
            if a.len() == 1 {
                w.append("{\"$range\": [0, ").expr(d, &a[0], 0)?.append("]}");
                Ok(())
            } else if a.len() == 3 {
                w.append("{\"$range\": [").expr(d, &a[0], 0)?.append(", ").expr(d, &a[1], 0)?.append(", ");
                unfolded(&a[2])(d, w)?;
                w.append("]}");
                Ok(())
            } else {
                op(d, w, "$range", a)
            }
        }
        SqlFn::ListProduct => {
            w.append("{\"$reduce\": {\"input\": ").expr(d, arg(a, 0)?, 0)?;
            w.append(&[", \"initialValue\": 1, \"in\": {\"$multiply\": [", VALUE, ", ", THIS, "]}}}"].concat());
            Ok(())
        }
        SqlFn::ListReduce => list_reduce(d, w, a),
        SqlFn::ListSlice => list_slice(d, w, a),
        SqlFn::RepeatValue => {
            w.append("{\"$map\": {\"input\": {\"$range\": [0, ").expr(d, arg(a, 1)?, 0)?.append("]}, \"in\": ");
            w.expr(d, arg(a, 0)?, 0)?.append("}}");
            Ok(())
        }
        SqlFn::ListBoolAnd => {
            w.append("{\"$allElementsTrue\": [").expr(d, arg(a, 0)?, 0)?.append("]}");
            Ok(())
        }
        SqlFn::ListBoolOr => {
            w.append("{\"$anyElementTrue\": [").expr(d, arg(a, 0)?, 0)?.append("]}");
            Ok(())
        }
        SqlFn::ListReverse => op1(d, w, "$reverseArray", arg(a, 0)?),
        SqlFn::Typeof => op1(d, w, "$type", arg(a, 0)?),
        other => Err(Refusal::illegal_state(format!("not a list call: {other}"))),
    }
}

/// A median or a mode of a list value: its non-null elements, reduced as a group's pushed values are.
fn list_reduced(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr], f: AggFn) -> Result<(), Refusal> {
    let values = pc(move |d, x| {
        non_null(
            d,
            x,
            &pc(move |d, y| {
                y.expr(d, arg(a, 0)?, 0)?;
                Ok(())
            }),
        )
    });
    array_reduction(d, w, f, &values, &[], false)
}

/// `(list, (accumulator, element) -> ...[, initial])`: `$reduce`; without an initial value the first element is it
/// and the rest are reduced.
fn list_reduce(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let l = lambda_of(arg(a, 1)?)?;
    if l.params.len() != 2 {
        return Err(Refusal::illegal_state(format!(
            "a reduce's lambda takes (accumulator, element): {}",
            java_text::strings(&l.params)
        )));
    }
    let variables = ["value".to_owned(), "this".to_owned()];
    if a.len() > 2 {
        w.append("{\"$reduce\": {\"input\": ").expr(d, &a[0], 0)?.append(", \"initialValue\": ");
        w.expr(d, &a[2], 0)?.append(", \"in\": ");
        d.with_lambda(w, l, &variables, &of(&l.body))?;
        w.append("}}");
        return Ok(());
    }
    w.append("{\"$let\": {\"vars\": {\"l\": ").expr(d, arg(a, 0)?, 0)?;
    w.append(concat!(
        "}, \"in\": {\"$cond\": [{\"$lte\":",
        " [{\"$size\": \"$$l\"}, 1]}, {\"$arrayElemAt\": [\"$$l\", 0]}, {\"$reduce\": {\"input\":",
        " {\"$slice\": [\"$$l\", 1, {\"$subtract\": [{\"$size\": \"$$l\"}, 1]}]}, \"initialValue\":",
        " {\"$arrayElemAt\": [\"$$l\", 0]}, \"in\": "
    ));
    d.with_lambda(w, l, &variables, &of(&l.body))?;
    w.append("}}]}}}");
    Ok(())
}

/// The values under a key: a list of one, or none (the map-extract contract).
fn map_extract(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$map\": {\"input\": {\"$filter\": {\"input\": {\"$objectToArray\": ").expr(d, arg(a, 0)?, 0)?;
    w.append("}, \"cond\": {\"$eq\": [\"$$this.k\", ").expr(d, arg(a, 1)?, 0)?;
    w.append("]}}}, \"in\": \"$$this.v\"}}");
    Ok(())
}

fn map_side(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr], side: &str) -> Result<(), Refusal> {
    w.append("{\"$map\": {\"input\": {\"$objectToArray\": ").expr(d, arg(a, 0)?, 0)?;
    w.append("}, \"in\": \"$$this.").append(side).append("\"}}");
    Ok(())
}

/// `(value, key)`: the value itself under the JSON path root (`$`), a field by its literal name, an element by its
/// 0-based index, or a field by a computed name through the document's entries (Java's `variantGet` override).
pub fn variant_get(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let a1 = arg(a, 1)?;
    if let SqlExpr::StringLit { value, .. } = a1
        && value == "$"
    {
        w.expr(d, arg(a, 0)?, 0)?;
        return Ok(());
    }
    if let SqlExpr::StringLit { value, .. } = a1 {
        w.append("{\"$getField\": {\"field\": ").append(&json::key(value)?).append(", \"input\": ");
        w.expr(d, arg(a, 0)?, 0)?.append("}}");
        return Ok(());
    }
    if matches!(a1, SqlExpr::IntLit { .. }) {
        w.append("{\"$arrayElemAt\": [").expr(d, arg(a, 0)?, 0)?.append(", ").expr(d, a1, 0)?.append("]}");
        return Ok(());
    }
    w.append("{\"$arrayElemAt\": [");
    map_extract(d, w, a)?;
    w.append(", 0]}");
    Ok(())
}

/// The JSON kind of a value, by the names `jsonKindName` answers.
fn json_type(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$let\": {\"vars\": {\"t\": {\"$type\": ").expr(d, arg(a, 0)?, 0)?;
    w.append(concat!(
        "}}, \"in\":",
        " {\"$switch\": {\"branches\": [{\"case\": {\"$in\": [\"$$t\", [\"int\", \"long\"]]}, \"then\":",
        " \"INTEGER\"}, {\"case\": {\"$in\": [\"$$t\", [\"double\", \"decimal\"]]}, \"then\": \"FLOAT\"},",
        " {\"case\": {\"$eq\": [\"$$t\", \"bool\"]}, \"then\": \"BOOLEAN\"}, {\"case\": {\"$eq\": [\"$$t\",",
        " \"string\"]}, \"then\": \"STRING\"}, {\"case\": {\"$eq\": [\"$$t\", \"object\"]}, \"then\":",
        " \"OBJECT\"}, {\"case\": {\"$eq\": [\"$$t\", \"array\"]}, \"then\": \"ARRAY\"}], \"default\":",
        " \"NULL\"}}}}"
    ));
    Ok(())
}

/// A raise: a conversion that cannot succeed, whose error carries its input -- the message between the platform's
/// U+001F sentinels, an optional source position ahead of it behind U+001E.
fn raise(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$convert\": {\"input\": {\"$concat\": [").append(&json::string(RAISE_SENTINEL)).append(", ");
    if a.len() > 1 {
        w.expr(d, &a[1], 0)?.append(", ").append(&json::string(POSITION_DIVIDER)).append(", ");
    }
    w.append("{\"$toString\": ").expr(d, arg(a, 0)?, 0)?.append("}, ").append(&json::string(RAISE_SENTINEL));
    w.append(", ").append(UNFOLDED).append("]}, \"to\": \"int\"}}");
    Ok(())
}
