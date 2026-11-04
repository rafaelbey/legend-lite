// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `MongoDb` compositions that spell long texts: a cast (`variantAwareCast`), a number's text as Pure spells it
//! (`numberText`), a value's JSON text by its kind at run time (`jsonText`, `cellText`, `jsonLevel`), the raise's
//! text, the string distances (`levenshtein`, `jaroWinkler`), printf (`format`), and a date's text (`strftime`,
//! `formatText`). Each is Java's method of the same name, its text Java's.

use super::json::{self, guarded};
use super::{G0, G1, MongoDb, Pc, RAISE_SENTINEL, UNFOLDED, VALUE, pc, temporal, text, typed_as};
use crate::refusal::{Refusal, arg};
use crate::renderer::AnsiSqlRenderer;
use crate::writer::SqlWriter;
use lite_sql::{DateFmt, DateFmtPart, Scalar, SqlExpr, SqlFn, SqlType, TypeFact};

/// How deep a nested document's JSON text goes: one level of text per level of nesting, a document nested deeper
/// refused by a raise.
pub const JSON_DEPTH: usize = 6;

/// A date-time's text in a wire: Pure's date-time to the millisecond.
const DATE_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S.%L+0000";

/// `String.format(Locale.ROOT, template, args...)` for a template whose only directives are `%s`.
fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = template;
    let mut args = args.iter();
    while let Some(at) = rest.find("%s") {
        out.push_str(&rest[..at]);
        out.push_str(args.next().expect("a %s argument"));
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

/// A raise of a fixed message: the message depends on the row, so the optimizer cannot fold it (Java's `raiseText`).
pub fn raise_text(message: &str) -> String {
    [
        "{\"$convert\": {\"input\": {\"$concat\": [",
        &json::string(&format!("{RAISE_SENTINEL}{message}{RAISE_SENTINEL}")),
        ", ",
        UNFOLDED,
        "]}, \"to\": \"int\"}}",
    ]
    .concat()
}

// ==================================================================
// Casts
// ==================================================================

/// A cast as `$convert` to the BSON type the target names; a date to text in Pure's date spelling; a decimal rounded
/// to its scale; a Variant target the value itself (Java's `variantAwareCast` override).
pub fn variant_aware_cast(d: &MongoDb, w: &mut SqlWriter, value: &SqlExpr, target: &SqlType) -> Result<(), Refusal> {
    if *target == SqlType::Scalar(Scalar::Json) {
        if let SqlExpr::StringLit { value: json, .. } = value {
            w.append("{\"$literal\": ").append(crate::java::strip(json)).append("}");
            return Ok(());
        }
        if typed_as(value, Scalar::Varchar) {
            return Err(Refusal::capability(
                "a text parsed as a Variant reached MongoDb, whose pipeline parses no JSON",
            ));
        }
        w.expr(d, value, 0)?;
        return Ok(());
    }
    if let SqlType::Decimal { scale, .. } = target {
        w.append("{\"$round\": [{\"$convert\": {\"input\": ").expr(d, value, 0)?;
        w.append(", \"to\": \"decimal\"}}, ").append_long(i64::from(*scale)).append("]}");
        return Ok(());
    }
    if *target == SqlType::Scalar(Scalar::Timestamptz) {
        // a zoned timestamp is the instant: a text with its offset parses to it (a single-digit month or day padded
        // first), a date is one already
        let from_text = matches!(value, SqlExpr::StringLit { .. }) || typed_as(value, Scalar::Varchar);
        if !from_text {
            w.append("{\"$convert\": {\"input\": ").expr(d, value, 0)?.append(", \"to\": \"date\"}}");
            return Ok(());
        }
        let pad = "{\"$cond\": [{\"$eq\": [{\"$strLenCP\": %s}, 1]}, {\"$concat\": [\"0\", %s]}, %s]}";
        let cap1 = "{\"$arrayElemAt\": [\"$$m.captures\", 1]}";
        let cap2 = "{\"$arrayElemAt\": [\"$$m.captures\", 2]}";
        w.append("{\"$let\": {\"vars\": {\"s\": ").expr(d, value, 0)?;
        w.append(
            &[
                "}, \"in\": {\"$let\":",
                " {\"vars\": {\"m\": {\"$regexFind\": {\"input\": \"$$s\", \"regex\":",
                " \"^([0-9]{4})-([0-9]{1,2})-([0-9]{1,2})(.*)$\"}}}, \"in\": {\"$convert\": {\"input\": {\"$cond\":",
                " [{\"$eq\": [\"$$m\", null]}, \"$$s\", {\"$concat\": [{\"$arrayElemAt\": [\"$$m.captures\", 0]},",
                " \"-\", ",
                &fill(pad, &[cap1, cap1, cap1]),
                ", \"-\", ",
                &fill(pad, &[cap2, cap2, cap2]),
                ", {\"$arrayElemAt\":",
                " [\"$$m.captures\", 3]}]}]}, \"to\": \"date\"}}}}}}",
            ]
            .concat(),
        );
        return Ok(());
    }
    if matches!(target, SqlType::Array { .. } | SqlType::Map { .. }) {
        // a BSON array or document is untyped: the cast is the lowering's typing of the value, which is itself here
        w.expr(d, value, 0)?;
        return Ok(());
    }
    let SqlType::Scalar(s) = target else {
        return Err(Refusal::capability(format!(
            "a cast to {} reached MongoDb: only scalar and decimal targets convert",
            crate::java::sql_type_text(target)
        )));
    };
    let is_text = *s == Scalar::Varchar;
    if is_text && matches!(value, SqlExpr::JsonObject { .. } | SqlExpr::JsonArray { .. }) {
        // the wire's JSON object as TEXT: written by the pipeline (D10), MongoDB having no JSON writer of its own
        return json_text(d, w, value);
    }
    if is_text && typed_as(value, Scalar::Json) {
        if matches!(value, SqlExpr::Call { r#fn: SqlFn::VariantGet, .. }) {
            // a Variant element read as text is the element's own string, any other kind its JSON text
            w.append("{\"$let\": {\"vars\": {\"e\": ").expr(d, value, 0)?;
            w.append("}, \"in\": {\"$cond\": [{\"$eq\": [{\"$type\": \"$$e\"}, \"string\"]}, \"$$e\", ");
            let e = crate::build::derived(None, "__e");
            cell_text_of(d, w, &e, &text("\"$$e\""))?;
            w.append("]}}}");
            return Ok(());
        }
        // a Variant's JSON text: the BSON value written as JSON by its kind at run time (D10)
        return cell_text(d, w, value);
    }
    if is_text && let Some(t) = temporal(value) {
        w.append("{\"$dateToString\": {\"date\": ").expr(d, value, 0)?.append(", \"format\": ");
        w.append(&json::string(if t == Scalar::Date { "%Y-%m-%d" } else { DATE_TIME_FORMAT })).append("}}");
        return Ok(());
    }
    if is_text && may_be_number(value) {
        return number_text(d, w, value);
    }
    if *s == Scalar::Boolean {
        // a string converts to true whatever it says, so a boolean's text is read by its word
        let body = [
            "{\"$cond\": [{\"$eq\":",
            " [{\"$type\": ",
            G0,
            "}, \"string\"]}, {\"$switch\": {\"branches\": [{\"case\":",
            " {\"$eq\": [{\"$toLower\": ",
            G0,
            "}, \"true\"]}, \"then\": true}, {\"case\": {\"$eq\":",
            " [{\"$toLower\": ",
            G0,
            "}, \"false\"]}, \"then\": false}], \"default\": ",
            &raise_text("a text that is no boolean reached a cast to Boolean on MongoDb"),
            "}},",
            " {\"$convert\": {\"input\": ",
            G0,
            ", \"to\": \"bool\"}}]}",
        ]
        .concat();
        return guarded(d, w, std::slice::from_ref(value), &text(body));
    }
    w.append("{\"$convert\": {\"input\": ").expr(d, value, 0)?.append(", \"to\": ");
    let name = d.scalar_type_name(*s)?;
    w.append(&json::string(&name)).append("}}");
    Ok(())
}

/// Whether the value may be a double or a decimal at run time, so the `$type` test decides: typed so, or of no type
/// fact.
fn may_be_number(e: &SqlExpr) -> bool {
    match e.r#type() {
        TypeFact::Typed { r#type, .. } => matches!(
            r#type,
            SqlType::Scalar(Scalar::Double) | SqlType::Scalar(Scalar::Hugeint) | SqlType::Decimal { .. }
        ),
        _ => true,
    }
}

/// A number's text as Pure spells it: the text bound once and the portable spelling written over it (Java's
/// `numberText`).
fn number_text(d: &MongoDb, w: &mut SqlWriter, value: &SqlExpr) -> Result<(), Refusal> {
    let huge_int = typed_as(value, Scalar::Hugeint);
    let body = pc(move |d, x| number_text_of(d, x, &json::var("g0"), huge_int));
    guarded(d, w, std::slice::from_ref(value), &body)
}

/// [`number_text`] over a value already bound to a variable, which `bound` writes: the text's sign, digits and
/// exponent found by one `$regexFind` and bound, the point moved as text (Java's `numberTextOf`).
pub fn number_text_of(d: &MongoDb, w: &mut SqlWriter, bound: &Pc<'_>, huge_int: bool) -> Result<(), Refusal> {
    // the shapes over the bound parts: $$s the text, $$g the sign, $$d the digits (the point removed), $$p the point's
    // position among them, $$n their count
    let zeros = concat!(
        "{\"$reduce\": {\"input\": {\"$range\": [0, %s]}, \"initialValue\": \"\", \"in\":",
        " {\"$concat\": [\"$$value\", \"0\"]}}}"
    );
    let unfolded = [
        "{\"$let\": {\"vars\": {\"g\": {\"$arrayElemAt\": [\"$$m.captures\", 0]}, \"d\":",
        " {\"$concat\": [{\"$arrayElemAt\": [\"$$m.captures\", 1]}, {\"$ifNull\": [{\"$arrayElemAt\":",
        " [\"$$m.captures\", 2]}, \"\"]}]}, \"p\": {\"$add\": [{\"$strLenCP\": {\"$arrayElemAt\":",
        " [\"$$m.captures\", 1]}}, {\"$toInt\": {\"$arrayElemAt\": [\"$$m.captures\", 3]}}]}},",
        " \"in\": {\"$let\": {\"vars\": {\"n\": {\"$strLenCP\": \"$$d\"}}, \"in\": {\"$concat\": [\"$$g\",",
        " {\"$switch\": {\"branches\": [",
        "{\"case\": {\"$lte\": [\"$$p\", 0]}, \"then\": {\"$concat\": [\"0.\", ",
        &fill(zeros, &["{\"$subtract\": [0, \"$$p\"]}"]),
        ", \"$$d\"]}},",
        " {\"case\": {\"$gte\": [\"$$p\", \"$$n\"]}, \"then\": {\"$concat\": [\"$$d\", ",
        &fill(zeros, &["{\"$subtract\": [\"$$p\", \"$$n\"]}"]),
        ", \".0\"]}}],",
        " \"default\": {\"$concat\": [{\"$substrCP\": [\"$$d\", 0, \"$$p\"]}, \".\", {\"$substrCP\": [\"$$d\",",
        " \"$$p\", {\"$subtract\": [\"$$n\", \"$$p\"]}]}]}}}]}}}}}",
    ]
    .concat();
    // the text without an exponent: a double gets its fraction digit (and every zero is 0.0); a decimal is as it is
    let plain = [
        "{\"$cond\": [{\"$eq\": [{\"$type\": ",
        "BOUND",
        "}, \"double\"]}, {\"$cond\": [{\"$regexMatch\":",
        " {\"input\": \"$$s\", \"regex\": \"^-?0+(\\\\.0+)?$\"}}, \"0.0\", {\"$cond\": [{\"$regexMatch\": {\"input\":",
        " \"$$s\", \"regex\": \"^-?[0-9]+$\"}}, {\"$concat\": [\"$$s\", \".0\"]}, \"$$s\"]}]}, \"$$s\"]}",
    ]
    .concat();
    let mut spelled = [
        "{\"$let\": {\"vars\": {\"m\": {\"$regexFind\": {\"input\": \"$$s\", \"regex\":",
        " \"^(-?)([0-9]+)(?:\\\\.([0-9]+))?[eE]([+-]?[0-9]+)$\"}}}, \"in\": {\"$cond\": [{\"$eq\": [\"$$m\",",
        " null]}, ",
        &plain,
        ", ",
        &unfolded,
        "]}}}",
    ]
    .concat();
    if huge_int {
        // an integer's digits: the unfolded form of a scientific Decimal128 ends in ".0", which an integer's text
        // has not -- a double under a HUGEINT fact keeps its Float spelling
        spelled = [
            "{\"$let\": {\"vars\": {\"t\": ",
            &spelled,
            "}, \"in\": {\"$cond\": [{\"$or\": [{\"$eq\":",
            " [{\"$type\": BOUND}, \"double\"]}, {\"$not\": {\"$regexMatch\": {\"input\": \"$$t\", \"regex\":",
            " \"\\\\.0$\"}}}]}, \"$$t\", {\"$substrCP\": [\"$$t\", 0, {\"$subtract\": [{\"$strLenCP\": \"$$t\"},",
            " 2]}]}]}}}",
        ]
        .concat();
    }
    w.append("{\"$let\": {\"vars\": {\"s\": {\"$toString\": ");
    bound(d, w)?;
    w.append("}}, \"in\": ");
    // the bound value is read again wherever its run-time type decides
    let mut parts = spelled.split("BOUND");
    w.append(parts.next().expect("a first part"));
    for part in parts {
        bound(d, w)?;
        w.append(part);
    }
    w.append("}}");
    Ok(())
}

// ==================================================================
// JSON text
// ==================================================================

/// The format a date's text takes in a wire: a date alone, or Pure's date-time to the millisecond.
fn date_format(e: &SqlExpr) -> &'static str {
    if temporal(e) == Some(Scalar::Date) { "%Y-%m-%d" } else { DATE_TIME_FORMAT }
}

/// The JSON text of an object or an array the wire builds (`CAST(json_object(...) AS VARCHAR)`), key by key, each
/// value's text by its kind at run time.
pub(super) fn json_text(d: &MongoDb, w: &mut SqlWriter, value: &SqlExpr) -> Result<(), Refusal> {
    match value {
        SqlExpr::JsonObject { kv, .. } => object_text(d, w, kv),
        SqlExpr::JsonArray { elements, .. } => array_text(d, w, elements),
        other => Err(Refusal::illegal_state(format!("not a JSON constructor: {}", super::java_text::expr(other)))),
    }
}

fn object_text(d: &MongoDb, w: &mut SqlWriter, kv: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$concat\": [\"{\"");
    let mut i = 0;
    while i + 1 < kv.len() {
        let SqlExpr::StringLit { value: key, .. } = &kv[i] else {
            return Err(Refusal::capability(
                "a JSON object with a computed key reached MongoDb: a document's field names are spelled",
            ));
        };
        let field = format!("{}{}:", if i > 0 { "," } else { "" }, json::string(key));
        w.append(", ").append(&json::string(&field)).append(", ");
        cell_text(d, w, &kv[i + 1])?;
        i += 2;
    }
    w.append(", \"}\"]}");
    Ok(())
}

fn array_text(d: &MongoDb, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"$concat\": [\"[\"");
    for (i, e) in elements.iter().enumerate() {
        w.append(if i > 0 { ", \",\", " } else { ", " });
        cell_text(d, w, e)?;
    }
    w.append(", \"]\"]}");
    Ok(())
}

/// One value's JSON text, by its kind at run time (Java's `cellText(writer, value)`).
fn cell_text(d: &MongoDb, w: &mut SqlWriter, value: &SqlExpr) -> Result<(), Refusal> {
    let bound = pc(move |d, x| {
        x.expr(d, value, 0)?;
        Ok(())
    });
    cell_text_of(d, w, value, &bound)
}

/// [`cell_text`] over the value `bound` writes, whose kind `value` types.
fn cell_text_of(d: &MongoDb, w: &mut SqlWriter, value: &SqlExpr, bound: &Pc<'_>) -> Result<(), Refusal> {
    w.append("{\"$let\": {\"vars\": {\"v\": ");
    bound(d, w)?;
    w.append("}, \"in\": ");
    json_level(d, w, date_format(value), JSON_DEPTH)?;
    w.append("}}");
    Ok(())
}

/// The JSON text of `$$v` at one level of nesting.
fn json_level(d: &MongoDb, w: &mut SqlWriter, date_format: &str, depth: usize) -> Result<(), Refusal> {
    w.append(
        &[
            "{\"$switch\":",
            " {\"branches\": [{\"case\": {\"$eq\": [\"$$v\", null]}, \"then\": \"null\"},",
            " {\"case\": {\"$eq\": [{\"$type\": \"$$v\"}, \"string\"]}, \"then\": {\"$concat\": [\"\\\"\", ",
            &escaped_json_string("\"$$v\""),
            ", \"\\\"\"]}},",
            " {\"case\": {\"$eq\": [{\"$type\": \"$$v\"}, \"date\"]}, \"then\": {\"$concat\": [\"\\\"\",",
            " {\"$dateToString\": {\"date\": \"$$v\", \"format\": ",
        ]
        .concat(),
    );
    w.append(&json::string(date_format));
    w.append(concat!(
        "}}, \"\\\"\"]}},",
        " {\"case\": {\"$in\": [{\"$type\": \"$$v\"}, [\"object\", \"array\"]]}, \"then\": "
    ));
    if depth == 0 {
        w.append(&raise_text(&format!("a document nested beyond {JSON_DEPTH} levels reached MongoDb's JSON writer")));
    } else {
        // the elements -- a document's entries, an array's values -- folded into one text, each written at the next
        // level, a document's keyed, commas between, the kind's brackets around
        let is_object = "{\"$eq\": [{\"$type\": \"$$v\"}, \"object\"]}";
        w.append(
            &[
                "{\"$concat\": [{\"$cond\": [",
                is_object,
                ", \"{\", \"[\"]}, {\"$reduce\": {\"input\":",
                " {\"$cond\": [",
                is_object,
                ", {\"$objectToArray\": \"$$v\"}, \"$$v\"]}, \"initialValue\": \"\",",
                " \"in\": {\"$concat\": [\"$$value\", {\"$cond\": [{\"$eq\": [\"$$value\", \"\"]}, \"\", \",\"]},",
                " {\"$cond\": [",
                is_object,
                ", {\"$concat\": [\"\\\"\", ",
                &escaped_json_string("\"$$this.k\""),
                ", \"\\\":\"]}, \"\"]}, {\"$let\": {\"vars\": {\"v\": {\"$cond\": [",
                is_object,
                ", \"$$this.v\", \"$$this\"]}}, \"in\": ",
            ]
            .concat(),
        );
        json_level(d, w, DATE_TIME_FORMAT, depth - 1)?;
        w.append(&["}}]}}}, {\"$cond\": [", is_object, ", \"}\", \"]\"]}]}"].concat());
    }
    w.append("}, {\"case\": {\"$in\": [{\"$type\": \"$$v\"}, [\"double\", \"decimal\"]]}, \"then\": ");
    // a number as Pure spells it
    number_text_of(d, w, &json::var("v"), false)?;
    w.append("}], \"default\": {\"$toString\": \"$$v\"}}}");
    Ok(())
}

/// The string `s`, with JSON's escapes: a backslash, a quote, a newline, a return, a tab.
fn escaped_json_string(s: &str) -> String {
    let mut s = s.to_owned();
    for (find, replacement) in [("\\", "\\\\"), ("\"", "\\\""), ("\n", "\\n"), ("\r", "\\r"), ("\t", "\\t")] {
        s = [
            "{\"$replaceAll\": {\"input\": ",
            &s,
            ", \"find\": ",
            &json::string(find),
            ", \"replacement\": ",
            &json::string(replacement),
            "}}",
        ]
        .concat();
    }
    s
}

// ==================================================================
// String distances
// ==================================================================

/// The Levenshtein distance of two texts (`$$g0`, `$$g1`), by characters: the edit-distance table a row at a time.
pub fn levenshtein() -> String {
    let lb = ["{\"$strLenCP\": ", G1, "}"].concat();
    let char_a = ["{\"$substrCP\": [", G0, ", {\"$subtract\": [\"$$i\", 1]}, 1]}"].concat();
    let char_b = ["{\"$substrCP\": [", G1, ", {\"$subtract\": [\"$$this\", 1]}, 1]}"].concat();
    let cell = [
        "{\"$min\": [{\"$add\": [{\"$arrayElemAt\": [\"$$p\", \"$$this\"]}, 1]}, {\"$add\":",
        " [{\"$arrayElemAt\": [",
        VALUE,
        ", {\"$subtract\": [\"$$this\", 1]}]}, 1]}, {\"$add\":",
        " [{\"$arrayElemAt\": [\"$$p\", {\"$subtract\": [\"$$this\", 1]}]}, {\"$cond\": [{\"$eq\": [",
        &char_a,
        ", ",
        &char_b,
        "]}, 0, 1]}]}]}",
    ]
    .concat();
    let row = [
        "{\"$reduce\": {\"input\": {\"$range\": [1, {\"$add\": [",
        &lb,
        ", 1]}]}, \"initialValue\":",
        " [\"$$i\"], \"in\": {\"$concatArrays\": [",
        VALUE,
        ", [",
        &cell,
        "]]}}}",
    ]
    .concat();
    let table = [
        "{\"$reduce\": {\"input\": {\"$range\": [1, {\"$add\": [{\"$strLenCP\": ",
        G0,
        "}, 1]}]},",
        " \"initialValue\": {\"$range\": [0, {\"$add\": [",
        &lb,
        ", 1]}]}, \"in\": {\"$let\": {\"vars\":",
        " {\"p\": ",
        VALUE,
        ", \"i\": \"$$this\"}, \"in\": ",
        &row,
        "}}}}",
    ]
    .concat();
    ["{\"$arrayElemAt\": [", &table, ", ", &lb, "]}"].concat()
}

/// Jaro-Winkler similarity of `$$g0` and `$$g1`, exactly DuckDB's.
pub fn jaro_winkler() -> String {
    let lp = ["{\"$strLenCP\": ", G0, "}"].concat();
    let lt = ["{\"$strLenCP\": ", G1, "}"].concat();
    let window = [
        "{\"$max\": [0, {\"$subtract\": [{\"$toInt\": {\"$floor\": {\"$divide\": [{\"$max\": [",
        &lp,
        ", ",
        &lt,
        "]}, 2]}}}, 1]}]}",
    ]
    .concat();
    // the flags with the window's character ($$this) set
    let flagged = [
        "{\"$let\": {\"vars\": {\"m\": \"$$this\", \"fv\": \"$$value.f\"}, \"in\": {\"$map\":",
        " {\"input\": {\"$range\": [0, ",
        &lp,
        "]}, \"in\": {\"$cond\": [{\"$eq\": [\"$$this\", \"$$m\"]}, 1,",
        " {\"$arrayElemAt\": [\"$$fv\", \"$$this\"]}]}}}}}",
    ]
    .concat();
    let t_char = ["{\"$substrCP\": [", G1, ", \"$$j\", 1]}"].concat();
    // one character of the second text (j, its state s): the window's first unmatched equal character taken
    let take = [
        "{\"$reduce\": {\"input\": {\"$range\": [{\"$max\": [0, {\"$subtract\": [\"$$j\", \"$$b\"]}]},",
        " {\"$min\": [",
        &lp,
        ", {\"$add\": [\"$$j\", \"$$b\", 1]}]}]}, \"initialValue\": {\"f\": \"$$s.f\",",
        " \"tm\": \"$$s.tm\", \"k\": false}, \"in\": {\"$cond\": [{\"$and\": [{\"$not\": [\"$$value.k\"]},",
        " {\"$eq\": [{\"$arrayElemAt\": [\"$$value.f\", \"$$this\"]}, 0]}, {\"$eq\": [{\"$substrCP\": [",
        G0,
        ", \"$$this\", 1]}, ",
        &t_char,
        "]}]}, {\"f\": ",
        &flagged,
        ", \"tm\": {\"$concat\": [\"$$value.tm\", ",
        &t_char,
        "]}, \"k\": true}, \"$$value\"]}}}",
    ]
    .concat();
    let matched = [
        "{\"$reduce\": {\"input\": {\"$range\": [0, ",
        &lt,
        "]}, \"initialValue\": {\"f\": {\"$map\":",
        " {\"input\": {\"$range\": [0, ",
        &lp,
        "]}, \"in\": 0}}, \"tm\": \"\"}, \"in\": {\"$let\": {\"vars\":",
        " {\"s\": \"$$value\", \"j\": \"$$this\"}, \"in\": ",
        &take,
        "}}}}",
    ]
    .concat();
    // along the first text: n matched so far, x of them unequal to tm's character in the same place
    let counted = [
        "{\"$reduce\": {\"input\": {\"$range\": [0, ",
        &lp,
        "]}, \"initialValue\": {\"n\": 0, \"x\": 0},",
        " \"in\": {\"$cond\": [{\"$eq\": [{\"$arrayElemAt\": [\"$$r.f\", \"$$this\"]}, 1]}, {\"n\": {\"$add\":",
        " [\"$$value.n\", 1]}, \"x\": {\"$add\": [\"$$value.x\", {\"$cond\": [{\"$eq\": [{\"$substrCP\": [",
        G0,
        ", \"$$this\", 1]}, {\"$substrCP\": [\"$$r.tm\", \"$$value.n\", 1]}]}, 0, 1]}]}}, \"$$value\"]}}}",
    ]
    .concat();
    let m = "{\"$toDouble\": \"$$c.n\"}";
    let jaro = [
        "{\"$divide\": [{\"$add\": [{\"$add\": [{\"$divide\": [",
        m,
        ", ",
        &lp,
        "]}, {\"$divide\": [",
        m,
        ", ",
        &lt,
        "]}]}, {\"$divide\": [{\"$subtract\": [",
        m,
        ", {\"$floor\": {\"$divide\": [\"$$c.x\",",
        " 2]}}]}, ",
        m,
        "]}]}, 3]}",
    ]
    .concat();
    let mut prefix = Vec::new();
    for k in 1..=4 {
        let k = k.to_string();
        prefix.push(
            [
                "{\"$cond\": [{\"$and\": [{\"$lte\": [",
                &k,
                ", {\"$min\": [",
                &lp,
                ", ",
                &lt,
                "]}]}, {\"$eq\":",
                " [{\"$substrCP\": [",
                G0,
                ", 0, ",
                &k,
                "]}, {\"$substrCP\": [",
                G1,
                ", 0, ",
                &k,
                "]}]}]}, 1, 0]}",
            ]
            .concat(),
        );
    }
    let bonus = [
        "{\"$multiply\": [{\"$multiply\": [{\"$toDouble\": {\"$add\": [",
        &prefix.join(", "),
        "]}}, 0.1]}, {\"$subtract\": [1.0, \"$$jw\"]}]}",
    ]
    .concat();
    let score = [
        "{\"$let\": {\"vars\": {\"jw\": ",
        &jaro,
        "}, \"in\": {\"$cond\": [{\"$gt\": [\"$$jw\", 0.7]},",
        " {\"$add\": [\"$$jw\", ",
        &bonus,
        "]}, \"$$jw\"]}}}",
    ]
    .concat();
    [
        "{\"$let\": {\"vars\": {\"b\": ",
        &window,
        "}, \"in\": {\"$let\": {\"vars\": {\"r\": ",
        &matched,
        "},",
        " \"in\": {\"$let\": {\"vars\": {\"c\": ",
        &counted,
        "}, \"in\": {\"$cond\": [{\"$eq\": [\"$$c.n\", 0]},",
        " 0.0, ",
        &score,
        "]}}}}}}}",
    ]
    .concat()
}

// ==================================================================
// printf
// ==================================================================

/// One directive of a format: `%s`, `%d`, or `%.Nf`'s N.
enum Directive {
    Text,
    Integer,
    Fixed(i32),
}

/// Pure's `format` over a literal format: the text between the directives concatenated with each argument's text;
/// `%%` is a percent sign; a directive other than `%s`, `%d` and `%.Nf` refused by name, as a format that is not a
/// literal is.
pub fn format(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let SqlExpr::StringLit { value: f, .. } = arg(a, 0)? else {
        return Err(Refusal::capability("a format that is not a literal reached MongoDb: no printf operator"));
    };
    let mut pieces: Vec<String> = Vec::new();
    let mut directives: Vec<Directive> = Vec::new();
    let mut piece = String::new();
    let b = f.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'%' {
            let c = f[i..].chars().next().expect("a character");
            piece.push(c);
            i += c.len_utf8();
            continue;
        }
        if i + 1 < b.len() && b[i + 1] == b'%' {
            piece.push('%');
            i += 2;
            continue;
        }
        // Java's `%(s|d|\.([0-9]+)f)` looking at the text from here
        let (directive, len) = match b.get(i + 1) {
            Some(b's') => (Directive::Text, 2),
            Some(b'd') => (Directive::Integer, 2),
            Some(b'.') => {
                let digits = b[i + 2..].iter().take_while(|c| c.is_ascii_digit()).count();
                if digits == 0 || b.get(i + 2 + digits) != Some(&b'f') {
                    return Err(no_printf(f));
                }
                let text = &f[i + 2..i + 2 + digits];
                let n = text.parse::<i32>().map_err(|_| Refusal {
                    class: Refusal::NUMBER_FORMAT,
                    message: format!("For input string: \"{text}\""),
                })?;
                (Directive::Fixed(n), 3 + digits)
            }
            _ => return Err(no_printf(f)),
        };
        pieces.push(std::mem::take(&mut piece));
        directives.push(directive);
        i += len;
    }
    pieces.push(piece);
    if pieces.len() != a.len() {
        return Err(Refusal::capability(format!(
            "format reached MongoDb with {} arguments for {} directives ({f})",
            a.len() - 1,
            pieces.len() - 1
        )));
    }
    w.append("{\"$concat\": [").append(&json::string(&pieces[0]));
    for i in 1..a.len() {
        w.append(", ");
        match directives[i - 1] {
            Directive::Text => format_argument(d, w, &a[i])?,
            Directive::Integer => {
                // %d: the integer's digits
                w.append("{\"$toString\": {\"$toLong\": ").expr(d, &a[i], 0)?.append("}}");
            }
            Directive::Fixed(digits) => fixed_fraction(d, w, &a[i], digits)?,
        }
        w.append(", ").append(&json::string(&pieces[i]));
    }
    w.append("]}");
    Ok(())
}

fn no_printf(f: &str) -> Refusal {
    Refusal::capability(format!(
        "a format directive other than %s, %d and %.Nf reached MongoDb ({f}): no printf operator"
    ))
}

/// `%.Nf`: the number rounded to N fraction digits and spelled with exactly N; a negative argument rounded to zero
/// keeps its sign.
fn fixed_fraction(d: &MongoDb, w: &mut SqlWriter, value: &SqlExpr, digits: i32) -> Result<(), Refusal> {
    w.append("{\"$let\": {\"vars\": {\"f\": ");
    fixed_fraction_text(d, w, value, digits)?;
    w.append("}, \"in\": {\"$cond\": [{\"$and\": [{\"$lt\": [").expr(d, value, 0)?;
    w.append(concat!(
        ", 0]}, {\"$ne\":",
        " [{\"$substrCP\": [\"$$f\", 0, 1]}, \"-\"]}]}, {\"$concat\": [\"-\", \"$$f\"]}, \"$$f\"]}}}"
    ));
    Ok(())
}

fn fixed_fraction_text(d: &MongoDb, w: &mut SqlWriter, value: &SqlExpr, digits: i32) -> Result<(), Refusal> {
    w.append("{\"$let\": {\"vars\": {\"r\": {\"$round\": [{\"$toDouble\": ").expr(d, value, 0)?.append("}, ");
    w.append_long(i64::from(digits)).append("]}}, \"in\": {\"$let\": {\"vars\": {\"t\": ");
    number_text_of(d, w, &json::var("r"), false)?;
    w.append("}, \"in\": {\"$let\": {\"vars\": {\"dot\": {\"$indexOfCP\": [\"$$t\", \".\"]}}, \"in\": ");
    if digits == 0 {
        w.append("{\"$substrCP\": [\"$$t\", 0, \"$$dot\"]}");
    } else {
        w.append(concat!(
            "{\"$concat\": [{\"$substrCP\": [\"$$t\", 0, \"$$dot\"]}, \".\", {\"$substrCP\": [{\"$concat\":",
            " [{\"$substrCP\": [\"$$t\", {\"$add\": [\"$$dot\", 1]}, {\"$strLenCP\": \"$$t\"}]}, "
        ));
        w.append(&json::string(&"0".repeat(usize::try_from(digits).unwrap_or(0)))).append("]}, 0, ");
        w.append_long(i64::from(digits)).append("]}]}");
    }
    w.append("}}}}}}");
    Ok(())
}

/// One `%s` argument's text, by its kind at run time.
fn format_argument(d: &MongoDb, w: &mut SqlWriter, value: &SqlExpr) -> Result<(), Refusal> {
    w.append("{\"$let\": {\"vars\": {\"g0\": ").expr(d, value, 0)?;
    w.append(concat!(
        "}, \"in\": {\"$switch\":",
        " {\"branches\": [{\"case\": {\"$eq\": [\"$$g0\", null]}, \"then\": \"\"},",
        " {\"case\": {\"$eq\": [{\"$type\": \"$$g0\"}, \"string\"]}, \"then\": \"$$g0\"},",
        " {\"case\": {\"$eq\": [{\"$type\": \"$$g0\"}, \"date\"]}, \"then\": {\"$dateToString\":",
        " {\"date\": \"$$g0\", \"format\": \"%Y-%m-%dT%H:%M:%S.%L+0000\"}}},",
        " {\"case\": {\"$in\": [{\"$type\": \"$$g0\"}, [\"double\", \"decimal\"]]}, \"then\": "
    ));
    number_text_of(d, w, &json::var("g0"), false)?;
    w.append("}], \"default\": {\"$toString\": \"$$g0\"}}}}}");
    Ok(())
}

// ==================================================================
// Dates' texts
// ==================================================================

/// A part `$dateToString` has no code for: a name, the 12-hour clock, or the minimal fraction.
fn composed(p: &DateFmt) -> bool {
    matches!(
        p,
        DateFmt::Part(
            DateFmtPart::MonthAbbrev
                | DateFmtPart::MonthName
                | DateFmtPart::WeekdayName
                | DateFmtPart::Hour12
                | DateFmtPart::Hour12Nopad
                | DateFmtPart::Ampm
                | DateFmtPart::SubsecMin
        )
    )
}

/// A date's text by a format: one `$dateToString` where every part has a code; a name or a 12-hour part composed
/// beside the coded runs.
pub fn strftime(d: &MongoDb, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
    let a1 = arg(a, 1)?;
    let parts = match a1 {
        SqlExpr::FormatLit { parts, .. } if parts.iter().any(composed) => parts,
        _ => {
            w.append("{\"$dateToString\": {\"date\": ").expr(d, arg(a, 0)?, 0)?.append(", \"format\": ");
            w.expr(d, a1, 0)?.append("}}");
            return Ok(());
        }
    };
    w.append("{\"$let\": {\"vars\": {\"d\": ").expr(d, arg(a, 0)?, 0)?.append("}, \"in\": {\"$concat\": [");
    let mut run: Vec<DateFmt> = Vec::new();
    let mut first = true;
    for p in parts {
        if !composed(p) {
            run.push(p.clone());
            continue;
        }
        first = coded_run(w, &mut run, first)?;
        w.append(if first { "" } else { ", " });
        first = false;
        let DateFmt::Part(part) = p else {
            return Err(Refusal::illegal_state("a composed part is a part"));
        };
        w.append(match part {
            DateFmtPart::WeekdayName => concat!(
                "{\"$arrayElemAt\": [[\"Sunday\", \"Monday\", \"Tuesday\", \"Wednesday\",",
                " \"Thursday\", \"Friday\", \"Saturday\"], {\"$subtract\": [{\"$dayOfWeek\": \"$$d\"}, 1]}]}"
            ),
            DateFmtPart::MonthName => concat!(
                "{\"$arrayElemAt\": [[\"January\", \"February\", \"March\", \"April\", \"May\",",
                " \"June\", \"July\", \"August\", \"September\", \"October\", \"November\",",
                " \"December\"], {\"$subtract\": [{\"$month\": \"$$d\"}, 1]}]}"
            ),
            DateFmtPart::MonthAbbrev => concat!(
                "{\"$arrayElemAt\": [[\"Jan\", \"Feb\", \"Mar\", \"Apr\", \"May\", \"Jun\",",
                " \"Jul\", \"Aug\", \"Sep\", \"Oct\", \"Nov\", \"Dec\"], {\"$subtract\": [{\"$month\":",
                " \"$$d\"}, 1]}]}"
            ),
            DateFmtPart::Hour12 => concat!(
                "{\"$let\": {\"vars\": {\"h\": {\"$toString\": {\"$add\": [{\"$mod\": [{\"$add\":",
                " [{\"$hour\": \"$$d\"}, 11]}, 12]}, 1]}}}, \"in\": {\"$cond\": [{\"$eq\": [{\"$strLenCP\":",
                " \"$$h\"}, 1]}, {\"$concat\": [\"0\", \"$$h\"]}, \"$$h\"]}}}"
            ),
            DateFmtPart::Hour12Nopad => concat!(
                "{\"$toString\": {\"$add\": [{\"$mod\": [{\"$add\": [{\"$hour\": \"$$d\"}, 11]},",
                " 12]}, 1]}}"
            ),
            DateFmtPart::Ampm => "{\"$cond\": [{\"$lt\": [{\"$hour\": \"$$d\"}, 12]}, \"AM\", \"PM\"]}",
            // the minimal fraction: the milliseconds' trailing zeros trimmed, one digit kept
            DateFmtPart::SubsecMin => concat!(
                "{\"$let\": {\"vars\": {\"f\": {\"$rtrim\": {\"input\": {\"$dateToString\": {\"date\":",
                " \"$$d\", \"format\": \"%L\"}}, \"chars\": \"0\"}}}, \"in\": {\"$cond\": [{\"$eq\": [\"$$f\",",
                " \"\"]}, \"0\", \"$$f\"]}}}"
            ),
            other => return Err(Refusal::illegal_state(format!("not a composed date part: {other}"))),
        });
    }
    coded_run(w, &mut run, first)?;
    w.append("]}}}");
    Ok(())
}

/// The run of coded parts as one `$dateToString`, emptied; whether nothing has been written yet.
fn coded_run(w: &mut SqlWriter, run: &mut Vec<DateFmt>, first: bool) -> Result<bool, Refusal> {
    if run.is_empty() {
        return Ok(first);
    }
    let codes = format_text(run)?;
    w.append(if first { "" } else { ", " }).append("{\"$dateToString\": {\"date\": \"$$d\", \"format\": ");
    w.append(&json::string(&codes)).append("}}");
    run.clear();
    Ok(false)
}

/// `$dateToString`'s codes: the date and time parts to the millisecond; a name or a 12-hour part has none (Java's
/// `formatText` override).
pub fn format_text(parts: &[DateFmt]) -> Result<String, Refusal> {
    let mut out = String::new();
    for p in parts {
        match p {
            DateFmt::Text { s } => out.push_str(&s.replace('%', "%%")),
            DateFmt::Part(part) => out.push_str(match part {
                DateFmtPart::Year4 => "%Y",
                DateFmtPart::Month2 => "%m",
                DateFmtPart::Day2 => "%d",
                DateFmtPart::Hour2 => "%H",
                DateFmtPart::Min2 => "%M",
                DateFmtPart::Sec2 => "%S",
                // a BSON date holds milliseconds, so its microsecond and nanosecond digits are zeros
                DateFmtPart::SubsecMicro => "%L000",
                DateFmtPart::SubsecNano => "%L000000",
                DateFmtPart::SubsecMin
                | DateFmtPart::MonthAbbrev
                | DateFmtPart::MonthName
                | DateFmtPart::WeekdayName
                | DateFmtPart::Hour12
                | DateFmtPart::Hour12Nopad
                | DateFmtPart::Ampm => {
                    return Err(Refusal::illegal_state(format!(
                        "a date format's {part} part reached $dateToString's codes outside its composition \
                         (MongoDb.strftime)"
                    )));
                }
            }),
        }
    }
    Ok(out)
}
