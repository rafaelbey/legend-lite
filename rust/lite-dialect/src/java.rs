// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Java library behaviour the renderers' text carries (docs/rust-port/TRIAGE.md B and C): `BigDecimal`'s plain
//! text, the numeric charter's Float spelling (`SqlTyping.floatDecimal`), record `toString`s that refusal messages
//! embed, `Character.isWhitespace`, and `String.split`'s dropped trailing empties.

use crate::refusal::Refusal;
use lite_sql::{ColumnType, OutputCol, SqlExpr, SqlType};

/// `new BigDecimal(text).toPlainString()`, `text` in `BigDecimal`'s grammar (lite-sql keeps a BigDecimal as the
/// text `BigDecimal.toString` wrote, which may carry an exponent: `1E+3`, `1.5E-7`).
pub fn big_decimal_plain(text: &str) -> String {
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
    let digits = format!("{int}{frac}");
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    plain(negative && digits != "0", digits, frac.len() as i64 - exponent)
}

/// A BigDecimal's plain text from its unscaled digits (no leading zero but a lone `0`) and its scale.
pub fn plain(negative: bool, digits: &str, scale: i64) -> String {
    let sign = if negative { "-" } else { "" };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    if scale < 0 {
        if digits == "0" {
            return "0".to_owned();
        }
        return format!("{sign}{digits}{}", "0".repeat((-scale) as usize));
    }
    let scale = scale as usize;
    if digits.len() > scale {
        let point = digits.len() - scale;
        format!("{sign}{}.{}", &digits[..point], &digits[point..])
    } else {
        format!("{sign}0.{}{digits}", "0".repeat(scale - digits.len()))
    }
}

/// `SqlTyping.floatDecimal(v)`'s plain text: a Float's plain digits (a point always present), or none for an extreme
/// magnitude (at least 10^15, or below 10^-6 and not zero). Java's `BigDecimal.valueOf(NaN)` refuses NaN.
pub fn float_decimal(v: f64) -> Result<Option<String>, Refusal> {
    let a = v.abs();
    if a >= 1e15 || (a > 0.0 && a < 1e-6) {
        return Ok(None);
    }
    if v.is_nan() {
        return Err(Refusal {
            class: Refusal::NUMBER_FORMAT,
            message:
                "Character N is neither a decimal digit number, decimal point, nor \"e\" notation exponential mark."
                    .to_owned(),
        });
    }
    let s = big_decimal_plain(&lite_javacompat::double_to_string(v));
    Ok(Some(if s.contains('.') { s } else { s + ".0" }))
}

/// `AnsiSqlRenderer.plainFloat`: the numeric charter's Rule 1 -- plain digits, or an extreme magnitude in
/// `Double.toString`'s exponent form.
pub fn plain_float(v: f64) -> Result<String, Refusal> {
    Ok(float_decimal(v)?.unwrap_or_else(|| lite_javacompat::double_to_string(v)))
}

/// `SqlType`'s `toString` (an enum constant's name, a record's `Name[component=value, ...]`).
pub fn sql_type_text(t: &SqlType) -> String {
    match t {
        SqlType::Scalar(s) => s.as_str().to_owned(),
        SqlType::Decimal { precision, scale } => format!("Decimal[precision={precision}, scale={scale}]"),
        SqlType::Array { element } => format!("Array[element={}]", sql_type_text(element)),
        SqlType::Map { key, value } => format!("Map[key={}, value={}]", sql_type_text(key), sql_type_text(value)),
        SqlType::Struct { fields } => {
            let fs: Vec<String> =
                fields.iter().map(|f| format!("Field[name={}, type={}]", f.name, sql_type_text(&f.r#type))).collect();
            format!("Struct[fields=[{}]]", fs.join(", "))
        }
        SqlType::Tagged { slots } => {
            let ss: Vec<String> =
                slots.iter().map(|s| format!("Slot[kind={}, type={}]", s.kind, sql_type_text(&s.r#type))).collect();
            format!("Tagged[slots=[{}]]", ss.join(", "))
        }
    }
}

/// `OutputCol`'s record `toString`.
pub fn output_col_text(o: &OutputCol) -> String {
    format!(
        "OutputCol[name={}, type={}, nullable={}, origin={}]",
        o.name,
        sql_type_text(&o.r#type),
        o.nullable,
        o.origin
    )
}

/// A list of `OutputCol`s as Java's `List.toString` writes it.
pub fn output_cols_text(os: &[OutputCol]) -> String {
    let parts: Vec<String> = os.iter().map(output_col_text).collect();
    format!("[{}]", parts.join(", "))
}

/// `SqlDdl.ColumnType`'s record `toString`.
pub fn column_type_text(t: &ColumnType) -> String {
    match t {
        ColumnType::Plain { kind } => format!("Plain[kind={kind}]"),
        ColumnType::Sized { kind, size } => format!("Sized[kind={kind}, size={size}]"),
        ColumnType::Scaled { kind, precision, scale } => {
            format!("Scaled[kind={kind}, precision={precision}, scale={scale}]")
        }
    }
}

/// The simple name of the Java record a node is (`getClass().getSimpleName()`).
pub fn simple_name(e: &SqlExpr) -> &'static str {
    match e {
        SqlExpr::RowOrder { .. } => "RowOrder",
        SqlExpr::StoredRead { .. } => "StoredRead",
        SqlExpr::ReduceCollection { .. } => "ReduceCollection",
        SqlExpr::Membership { .. } => "Membership",
        SqlExpr::Column(_) => "Column",
        SqlExpr::StarExcept { .. } => "StarExcept",
        SqlExpr::Star { .. } => "Star",
        SqlExpr::StringLit { .. } => "StringLit",
        SqlExpr::IntLit { .. } => "IntLit",
        SqlExpr::FloatLit { .. } => "FloatLit",
        SqlExpr::DecimalLit { .. } => "DecimalLit",
        SqlExpr::BoolLit { .. } => "BoolLit",
        SqlExpr::NullLit { .. } => "NullLit",
        SqlExpr::DateLit { .. } => "DateLit",
        SqlExpr::Group { .. } => "Group",
        SqlExpr::KeyCell { .. } => "KeyCell",
        SqlExpr::PlanParam { .. } => "PlanParam",
        SqlExpr::FormatLit { .. } => "FormatLit",
        SqlExpr::DatePartLit { .. } => "DatePartLit",
        SqlExpr::JsonKindLit { .. } => "JsonKindLit",
        SqlExpr::TypeNameLit { .. } => "TypeNameLit",
        SqlExpr::TimestampLit { .. } => "TimestampLit",
        SqlExpr::TimeLit { .. } => "TimeLit",
        SqlExpr::OrderedListAgg { .. } => "OrderedListAgg",
        SqlExpr::ArrayLit { .. } => "ArrayLit",
        SqlExpr::StructLit { .. } => "StructLit",
        SqlExpr::StructGet { .. } => "StructGet",
        SqlExpr::Call { .. } => "Call",
        SqlExpr::Case { .. } => "Case",
        SqlExpr::Exists { .. } => "Exists",
        SqlExpr::InSubquery { .. } => "InSubquery",
        SqlExpr::CheckedDefects { .. } => "CheckedDefects",
        SqlExpr::CheckedChildValue { .. } => "CheckedChildValue",
        SqlExpr::Quantified { .. } => "Quantified",
        SqlExpr::ScalarSubquery { .. } => "ScalarSubquery",
        SqlExpr::CheckedOne { .. } => "CheckedOne",
        SqlExpr::CompactList { .. } => "CompactList",
        SqlExpr::DeferredTdsString { .. } => "DeferredTdsString",
        SqlExpr::TempTableInSplice { .. } => "TempTableInSplice",
        SqlExpr::JsonObject { .. } => "JsonObject",
        SqlExpr::JsonArray { .. } => "JsonArray",
        SqlExpr::JsonArrayAgg { .. } => "JsonArrayAgg",
        SqlExpr::WindowCall { .. } => "WindowCall",
        SqlExpr::Lambda(_) => "Lambda",
        SqlExpr::Cast { .. } => "Cast",
        SqlExpr::FoldCall { .. } => "FoldCall",
        SqlExpr::Reducer(_) => "Reducer",
    }
}

/// An expression as a refusal message names it. Java writes the record's whole `toString`; this writes its simple
/// name, and a call its function (the messages that embed a node are the lowering's bugs, never in the dump).
pub fn expr_text(e: &SqlExpr) -> String {
    match e {
        SqlExpr::Call { r#fn, .. } => format!("Call[fn={fn}, ...]"),
        other => format!("{}[...]", simple_name(other)),
    }
}

/// Java's `Character.isWhitespace` (a space separator but the no-break ones, a line or paragraph separator, or one of
/// the ASCII controls `\t \n \u000B \f \r` and `\u001C`-`\u001F`).
pub fn is_java_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t'..='\r'
            | '\u{1C}'..='\u{1F}'
            | ' '
            | '\u{1680}'
            | '\u{2000}'..='\u{2006}'
            | '\u{2008}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

/// Java's `String.strip()`: `Character.isWhitespace` trimmed from both ends.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_java_whitespace)
}

/// Java's `s.split(sep)` for a one-character literal separator: trailing empty strings dropped, and the whole string
/// when the separator is not in it.
pub fn split(s: &str, sep: char) -> Vec<&str> {
    if !s.contains(sep) {
        return vec![s];
    }
    let mut parts: Vec<&str> = s.split(sep).collect();
    while parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}

/// Java's `String.hashCode()`: `s[0]*31^(n-1) + ... + s[n-1]` over the UTF-16 code units, wrapping.
pub fn string_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, u| h.wrapping_mul(31).wrapping_add(i32::from(u)))
}

/// The order a `java.util.HashMap<String, V>` built by putting `keys` in turn (a repeated key kept where it first
/// went) iterates them: by bucket under the final table size (16, doubled whenever the size passes three quarters of
/// it), a bucket's keys in insertion order (`resize` splits a bucket keeping its order). A bucket of eight or more
/// keys is a tree in Java, whose iteration this does not model.
pub fn hash_map_order<S: AsRef<str>>(keys: &[S]) -> Vec<&str> {
    let mut distinct: Vec<&str> = Vec::new();
    for k in keys {
        if !distinct.contains(&k.as_ref()) {
            distinct.push(k.as_ref());
        }
    }
    let mut capacity: usize = 16;
    while distinct.len() > capacity * 3 / 4 {
        capacity *= 2;
    }
    let bucket = |k: &str| {
        let h = string_hash(k) as u32;
        ((h ^ (h >> 16)) as usize) & (capacity - 1)
    };
    let mut ordered: Vec<(usize, usize, &str)> = distinct.iter().enumerate().map(|(i, k)| (bucket(k), i, *k)).collect();
    ordered.sort();
    ordered.into_iter().map(|(_, _, k)| k).collect()
}
