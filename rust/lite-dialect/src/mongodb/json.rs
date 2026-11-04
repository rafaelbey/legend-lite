// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `MongoJson` (`core/src/main/java/com/legend/sql/dialect/MongoJson.java`): the text of a MongoDB command as
//! [`MongoDb`] writes it -- JSON in MongoDB's Extended JSON spelling, written into the one [`SqlWriter`] like any
//! dialect's SQL, each bound parameter at its place. The helpers spell the shapes every arm composes: a string literal
//! (always `$literal`, since a string beginning with `$` would read as a field path), an operator over its arguments,
//! a `$let` binding each argument once, and the null guard that gives an operator SQL's three-valued answer (decision
//! D7: a null operand answers null, which BSON's total comparisons never do).

use super::{MongoDb, Pc, pc};
use crate::refusal::Refusal;
use crate::writer::SqlWriter;
use lite_sql::{SqlExpr, TypeFact};

/// `s` as a JSON string (Java's `string`): a quote, a backslash, a newline, a return and a tab escaped by their
/// letters, any other control character as `\u00xx`.
pub fn string(s: &str) -> String {
    let mut sb = String::with_capacity(s.len() + 2);
    sb.push('"');
    for c in s.chars() {
        match c {
            '"' => sb.push_str("\\\""),
            '\\' => sb.push_str("\\\\"),
            '\n' => sb.push_str("\\n"),
            '\r' => sb.push_str("\\r"),
            '\t' => sb.push_str("\\t"),
            c if (c as u32) < 0x20 => sb.push_str(&format!("\\u{:04x}", c as u32)),
            c => sb.push(c),
        }
    }
    sb.push('"');
    sb
}

/// A document's field name, as a JSON key: a name MongoDB refuses as a field (`$`-led, holding a dot or a NUL) is
/// refused here by name, never respelled.
pub fn key(name: &str) -> Result<String, Refusal> {
    if name.is_empty() || name.starts_with('$') || name.contains('.') || name.contains('\0') {
        return Err(Refusal::capability(format!(
            "'{name}' is no MongoDB field name (a name that is empty, begins with $, or holds a dot or a NUL): a \
             column or alias so named is refused, never respelled"
        )));
    }
    Ok(string(name))
}

/// A field path, `"$a.b"`, from the parts a reference names.
pub fn path(parts: &[&str]) -> String {
    string(&format!("${}", parts.join(".")))
}

/// A `$let` variable's name from an alias or a lambda parameter: `v_`, then each UTF-16 unit an ASCII letter or digit
/// kept and any other replaced by `_` (a character past the BMP is two units, so two `_`).
pub fn variable(name: &str) -> String {
    let mut sb = String::from("v_");
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            sb.push(c);
        } else {
            for _ in 0..c.len_utf16() {
                sb.push('_');
            }
        }
    }
    sb
}

/// `{"$literal": "<text>"}`: a string value, never a path.
pub fn literal(w: &mut SqlWriter, text: &str) {
    w.append("{\"$literal\": ").append(&string(text)).append("}");
}

/// `{"$op": [<args>]}`.
pub fn op(d: &MongoDb, w: &mut SqlWriter, op: &str, args: &[SqlExpr]) -> Result<(), Refusal> {
    w.append("{\"").append(op).append("\": [").list(d, args)?.append("]}");
    Ok(())
}

/// The four numeric reducers, which read a bare array and an argument list alike.
const REDUCERS: &[&str] = &["$sum", "$min", "$max", "$avg"];

/// `{"$op": <arg>}`: an operator of one argument; a literal array is written as the one element of an argument list
/// (`{"$op": [[...]]}`), the four numeric reducers keeping the bare one.
pub fn op1(d: &MongoDb, w: &mut SqlWriter, op: &str, arg: &SqlExpr) -> Result<(), Refusal> {
    let listed = matches!(arg, SqlExpr::ArrayLit { .. }) && !REDUCERS.contains(&op);
    w.append("{\"").append(op).append("\": ").append(if listed { "[" } else { "" }).expr(d, arg, 0)?;
    w.append(if listed { "]}" } else { "}" });
    Ok(())
}

/// `{"$op": [<pieces>]}`: an operator over pieces the caller writes.
pub fn op_of(d: &MongoDb, w: &mut SqlWriter, op: &str, pieces: &[Pc<'_>]) -> Result<(), Refusal> {
    w.append("{\"").append(op).append("\": [");
    for (i, p) in pieces.iter().enumerate() {
        if i > 0 {
            w.append(", ");
        }
        p(d, w)?;
    }
    w.append("]}");
    Ok(())
}

/// A piece writing the variable `"$$name"`.
pub fn var(name: &str) -> Pc<'static> {
    let text = string(&format!("$${name}"));
    pc(move |_, w| {
        w.append(&text);
        Ok(())
    })
}

/// Whether `e` may be null, as its type fact says: a proof of non-null skips the guard (D7's optimisation); anything
/// else is guarded.
pub fn nullable(e: &SqlExpr) -> bool {
    !matches!(e.r#type(), TypeFact::Typed { nullable: false, .. })
}

/// `args` bound once as `$$g0`, `$$g1`, ..., then `body` over them, answering null when any nullable argument is null:
/// SQL's three-valued operator over BSON's total one (D7). With no nullable argument the body is written over the bound
/// variables alone.
pub fn guarded(d: &MongoDb, w: &mut SqlWriter, args: &[SqlExpr], body: &Pc<'_>) -> Result<(), Refusal> {
    w.append("{\"$let\": {\"vars\": {");
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            w.append(", ");
        }
        w.append("\"g").append_long(i as i64).append("\": ").expr(d, a, 0)?;
    }
    w.append("}, \"in\": ");
    let guards: Vec<usize> = (0..args.len()).filter(|i| nullable(&args[*i])).collect();
    if guards.is_empty() {
        body(d, w)?;
    } else {
        w.append("{\"$cond\": [");
        any_null(w, &guards);
        w.append(", null, ");
        body(d, w)?;
        w.append("]}");
    }
    w.append("}}");
    Ok(())
}

/// `{"$or": [{"$eq": ["$$g<i>", null]}, ...]}` over the guarded indices, or the one test alone.
fn any_null(w: &mut SqlWriter, guards: &[usize]) {
    if guards.len() > 1 {
        w.append("{\"$or\": [");
    }
    for (i, g) in guards.iter().enumerate() {
        if i > 0 {
            w.append(", ");
        }
        w.append("{\"$eq\": [").append(&string(&format!("$$g{g}"))).append(", null]}");
    }
    if guards.len() > 1 {
        w.append("]}");
    }
}

/// `{"$op": ["$$g0", "$$g1", ...]}` over the guarded variables of `n` arguments.
pub fn op_over_vars(op: &str, n: usize) -> Pc<'static> {
    let op = op.to_owned();
    pc(move |_, w| {
        w.append("{\"").append(&op).append("\": [");
        for i in 0..n {
            if i > 0 {
                w.append(", ");
            }
            w.append(&string(&format!("$$g{i}")));
        }
        w.append("]}");
        Ok(())
    })
}

/// `{"$op": "$$g0"}`: an operator of one guarded argument.
pub fn op1_over_var(op: &str) -> Pc<'static> {
    let op = op.to_owned();
    pc(move |_, w| {
        w.append("{\"").append(&op).append("\": ").append(&string("$$g0")).append("}");
        Ok(())
    })
}

/// An operator of `args.len()` arguments, each bound once and null-guarded (D7).
pub fn guarded_op(d: &MongoDb, w: &mut SqlWriter, op: &str, args: &[SqlExpr]) -> Result<(), Refusal> {
    let body = if args.len() == 1 { op1_over_var(op) } else { op_over_vars(op, args.len()) };
    guarded(d, w, args, &body)
}

/// Whether a UTF-16 unit is a decimal digit as Java's `Character.isDigit(char)` answers (a surrogate is none).
fn is_digit_unit(u: u16) -> bool {
    lite_javacompat::character::is_digit(u)
}

/// Whether a UTF-16 unit is a line terminator Java's regular-expression `.` does not match.
fn is_line_terminator(u: u16) -> bool {
    matches!(u, 0x0A | 0x0D | 0x85 | 0x2028 | 0x2029)
}

/// Java's `t.matches(".*[+-]\\d\\d:?\\d\\d$")`: no line terminator anywhere (`.` takes none, and `matches` takes the
/// whole text), ending in a sign, two ASCII digits, an optional colon and two ASCII digits.
fn ends_in_offset(t: &[u16]) -> bool {
    if t.iter().any(|u| is_line_terminator(*u)) {
        return false;
    }
    let d = |u: u16| (u16::from(b'0')..=u16::from(b'9')).contains(&u);
    let sign = |u: u16| u == u16::from(b'+') || u == u16::from(b'-');
    let n = t.len();
    let plain = n >= 5 && sign(t[n - 5]) && d(t[n - 4]) && d(t[n - 3]) && d(t[n - 2]) && d(t[n - 1]);
    let colon = n >= 6
        && sign(t[n - 6])
        && d(t[n - 5])
        && d(t[n - 4])
        && t[n - 3] == u16::from(b':')
        && d(t[n - 2])
        && d(t[n - 1]);
    plain || colon
}

/// The text of a BSON date literal from an ISO date or date-time text (a space or a `T` between date and time; a
/// fraction of a second to the millisecond, BSON's precision; finer digits refused by name, never cut). Java's indexes
/// are UTF-16 units, so the text is read as Java reads it.
pub fn date_literal(iso: &str) -> Result<String, Refusal> {
    let trimmed: String = crate::java::trim(iso).chars().map(|c| if c == ' ' { 'T' } else { c }).collect();
    let mut t: Vec<u16> = trimmed.encode_utf16().collect();
    if t.len() == 10 {
        t.extend("T00:00:00".encode_utf16());
    }
    if let Some(dot) = t.iter().position(|u| *u == u16::from(b'.')) {
        let mut end = dot + 1;
        while end < t.len() && is_digit_unit(t[end]) {
            end += 1;
        }
        let fraction = &t[dot + 1..end];
        if fraction.len() > 3 && !fraction[3..].iter().all(|u| *u == u16::from(b'0')) {
            return Err(Refusal::capability(format!(
                "a date-time finer than a millisecond ({iso}) has no BSON date: refused, never cut"
            )));
        }
        let mut padded: Vec<u16> = fraction.to_vec();
        padded.extend("000".encode_utf16());
        let mut out: Vec<u16> = t[..dot].to_vec();
        out.push(u16::from(b'.'));
        out.extend_from_slice(&padded[..3]);
        out.extend_from_slice(&t[end..]);
        t = out;
    }
    if !(t.last() == Some(&u16::from(b'Z'))) && !ends_in_offset(&t) {
        t.push(u16::from(b'Z'));
    }
    let text = String::from_utf16(&t).map_err(|_| Refusal::illegal_state("a date literal's text is no UTF-16"))?;
    Ok(format!("{{\"$date\": {}}}", string(&text)))
}
