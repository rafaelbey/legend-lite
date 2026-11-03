// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `com.legend.sql.Json` (`core/src/main/java/com/legend/sql/Json.java`), the platform's JSON reader, as far as
//! the carrier strategies reach it: `parse` (strict: trailing text refused), `unescapeString` and `canonical`. Java's
//! value model is kept, since it reaches SQL text: an integer token is a `Long` (a `BigInteger` past a long's range),
//! a token with a point or an exponent a `BigDecimal` (never a double), an object a `LinkedHashMap` (insertion
//! order, a repeated key's value replaced in place).
//!
//! The reader walks UTF-16 code units, as Java's `charAt` does, so an index in a refusal is Java's.

use crate::java::is_java_whitespace;
use crate::refusal::Refusal;

/// A parsed JSON value, as Java's reader types it.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// A `Long`.
    Long(i64),
    /// A `BigInteger` past a long's range, as `BigInteger.toString` writes it.
    BigInteger(String),
    /// A `BigDecimal`: the token `new BigDecimal(token)` read.
    Decimal(String),
    Str(String),
    List(Vec<Json>),
    /// A `LinkedHashMap<String, Object>`, its entries in insertion order.
    Map(Vec<(String, Json)>),
}

const STRING_INDEX: &str = "java.lang.StringIndexOutOfBoundsException";

struct Reader {
    s: Vec<u16>,
    i: usize,
}

/// `Json.parse(json)`: one value, whitespace around it, nothing after.
pub fn parse(json: &str) -> Result<Json, Refusal> {
    let mut p = Reader { s: json.encode_utf16().collect(), i: 0 };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i < p.s.len() {
        return Err(Refusal::illegal_state(format!("trailing JSON at {}", p.i)));
    }
    Ok(v)
}

impl Reader {
    fn char_at(&self, i: usize) -> Result<u16, Refusal> {
        self.s.get(i).copied().ok_or_else(|| Refusal {
            class: STRING_INDEX,
            message: format!("Index {i} out of bounds for length {}", self.s.len()),
        })
    }

    fn value(&mut self) -> Result<Json, Refusal> {
        let c = self.char_at(self.i)?;
        Ok(match c {
            0x7B => self.obj()?,
            0x5B => self.arr()?,
            0x22 => Json::Str(self.str()?),
            0x74 => {
                self.i += 4;
                Json::Bool(true)
            }
            0x66 => {
                self.i += 5;
                Json::Bool(false)
            }
            0x6E => {
                self.i += 4;
                Json::Null
            }
            _ => self.num()?,
        })
    }

    fn obj(&mut self) -> Result<Json, Refusal> {
        let mut out: Vec<(String, Json)> = Vec::new();
        self.i += 1;
        self.ws();
        if self.char_at(self.i)? == 0x7D {
            self.i += 1;
            return Ok(Json::Map(out));
        }
        loop {
            self.ws();
            let k = self.str()?;
            self.ws();
            self.i += 1; // ':'
            self.ws();
            let v = self.value()?;
            match out.iter_mut().find(|(key, _)| *key == k) {
                Some(entry) => entry.1 = v,
                None => out.push((k, v)),
            }
            self.ws();
            if self.char_at(self.i)? == 0x2C {
                self.i += 1;
                continue;
            }
            self.i += 1; // '}'
            return Ok(Json::Map(out));
        }
    }

    fn arr(&mut self) -> Result<Json, Refusal> {
        let mut out = Vec::new();
        self.i += 1;
        self.ws();
        if self.char_at(self.i)? == 0x5D {
            self.i += 1;
            return Ok(Json::List(out));
        }
        loop {
            self.ws();
            out.push(self.value()?);
            self.ws();
            if self.char_at(self.i)? == 0x2C {
                self.i += 1;
                continue;
            }
            self.i += 1; // ']'
            return Ok(Json::List(out));
        }
    }

    fn str(&mut self) -> Result<String, Refusal> {
        self.i += 1;
        let start = self.i;
        while self.char_at(self.i)? != 0x22 {
            self.i += if self.char_at(self.i)? == 0x5C { 2 } else { 1 };
        }
        let body = self.s[start..self.i].to_vec();
        self.i += 1;
        unescape_units(&body)
    }

    fn num(&mut self) -> Result<Json, Refusal> {
        let start = self.i;
        while self.i < self.s.len() && b"+-0123456789.eE".iter().any(|b| u16::from(*b) == self.s[self.i]) {
            self.i += 1;
        }
        let t = String::from_utf16_lossy(&self.s[start..self.i]);
        if t.contains(['.', 'e', 'E']) {
            if lite_sql::BigDecimal::parse(&t).is_err() {
                return Err(Refusal {
                    class: Refusal::NUMBER_FORMAT,
                    message: format!("Character array is missing \"exponent\" mark 'e' or 'E': {t}"),
                });
            }
            return Ok(Json::Decimal(t));
        }
        integer(&t)
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && char::from_u32(u32::from(self.s[self.i])).is_some_and(is_java_whitespace) {
            self.i += 1;
        }
    }
}

/// `Json.integer`: a `Long`, or a `BigInteger` past a long's range.
fn integer(t: &str) -> Result<Json, Refusal> {
    let bad = || Refusal { class: Refusal::NUMBER_FORMAT, message: format!("For input string: \"{t}\"") };
    let unsigned = t.strip_prefix(['-', '+']).unwrap_or(t);
    if unsigned.is_empty() || !unsigned.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    if unsigned.len() <= 18 {
        return t.parse::<i64>().map(Json::Long).map_err(|_| bad());
    }
    if let Ok(v) = t.parse::<i64>() {
        return Ok(Json::Long(v));
    }
    let digits = unsigned.trim_start_matches('0');
    let negative = t.starts_with('-');
    Ok(Json::BigInteger(format!("{}{digits}", if negative { "-" } else { "" })))
}

/// `Json.unescapeString(s)`: the named escapes, `\uXXXX`, and drop-backslash for any other.
pub fn unescape_string(s: &str) -> Result<String, Refusal> {
    let units: Vec<u16> = s.encode_utf16().collect();
    unescape_units(&units)
}

fn unescape_units(s: &[u16]) -> Result<String, Refusal> {
    if !s.contains(&0x5C) {
        return Ok(String::from_utf16_lossy(s));
    }
    let mut b: Vec<u16> = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c != 0x5C || i + 1 >= s.len() {
            b.push(c);
            i += 1;
            continue;
        }
        let e = s[i + 1];
        match e {
            0x6E => b.push(0x0A),
            0x74 => b.push(0x09),
            0x72 => b.push(0x0D),
            0x62 => b.push(0x08),
            0x66 => b.push(0x0C),
            0x75 => {
                let end = i + 6;
                if end > s.len() {
                    return Err(Refusal {
                        class: STRING_INDEX,
                        message: format!("begin {}, end {end}, length {}", i + 2, s.len()),
                    });
                }
                let hex = String::from_utf16_lossy(&s[i + 2..end]);
                let unit = u16::from_str_radix(&hex, 16).map_err(|_| Refusal {
                    class: Refusal::NUMBER_FORMAT,
                    message: format!("For input string: \"{hex}\" under radix 16"),
                })?;
                b.push(unit);
                i += 4;
            }
            other => b.push(other),
        }
        i += 2;
    }
    Ok(String::from_utf16_lossy(&b))
}

/// `Json.canonical(v)`: compact, object keys sorted, numbers as parsed (a decimal's `toPlainString`).
pub fn canonical(v: &Json) -> String {
    let mut b = String::new();
    write_canonical(v, &mut b);
    b
}

fn write_canonical(v: &Json, b: &mut String) {
    match v {
        Json::Null => b.push_str("null"),
        Json::Map(m) => {
            let mut keys: Vec<&String> = m.iter().map(|(k, _)| k).collect();
            // Java's natural String order: UTF-16 code units
            keys.sort_by(|x, y| x.encode_utf16().cmp(y.encode_utf16()));
            b.push('{');
            for (n, k) in keys.into_iter().enumerate() {
                if n > 0 {
                    b.push(',');
                }
                write_string(k, b);
                b.push(':');
                let value = &m.iter().find(|(key, _)| key == k).expect("a key of the map").1;
                write_canonical(value, b);
            }
            b.push('}');
        }
        Json::List(l) => {
            b.push('[');
            for (n, e) in l.iter().enumerate() {
                if n > 0 {
                    b.push(',');
                }
                write_canonical(e, b);
            }
            b.push(']');
        }
        Json::Str(s) => write_string(s, b),
        Json::Decimal(t) => b.push_str(&crate::java::big_decimal_plain(t)),
        Json::Long(n) => b.push_str(&n.to_string()),
        Json::BigInteger(t) => b.push_str(t),
        Json::Bool(x) => b.push_str(if *x { "true" } else { "false" }),
    }
}

fn write_string(s: &str, b: &mut String) {
    b.push('"');
    for c in s.chars() {
        match c {
            '"' => b.push_str("\\\""),
            '\\' => b.push_str("\\\\"),
            '\n' => b.push_str("\\n"),
            '\t' => b.push_str("\\t"),
            '\r' => b.push_str("\\r"),
            '\u{8}' => b.push_str("\\b"),
            '\u{c}' => b.push_str("\\f"),
            c if (c as u32) < 0x20 => b.push_str(&format!("\\u{:04x}", c as u32)),
            c => b.push(c),
        }
    }
    b.push('"');
}

/// `BigDecimal.toString()` of `new BigDecimal(token)`: plain digits while the adjusted exponent is at least -6 and the
/// scale not negative, else scientific (`1E+5`, `1.5E-7`).
pub fn big_decimal_text(token: &str) -> String {
    let (negative, unsigned) = match token.as_bytes().first() {
        Some(b'-') => (true, &token[1..]),
        Some(b'+') => (false, &token[1..]),
        _ => (false, token),
    };
    let (significand, exponent) = match unsigned.find(['e', 'E']) {
        Some(i) => (&unsigned[..i], unsigned[i + 1..].parse::<i64>().unwrap_or(0)),
        None => (unsigned, 0),
    };
    let (int, frac) = significand.split_once('.').unwrap_or((significand, ""));
    let joined = format!("{int}{frac}");
    let trimmed = joined.trim_start_matches('0');
    let coeff = if trimmed.is_empty() { "0" } else { trimmed };
    let scale = frac.len() as i64 - exponent;
    let sign = if negative && coeff != "0" { "-" } else { "" };
    if scale == 0 {
        return format!("{sign}{coeff}");
    }
    let adjusted = -scale + (coeff.len() as i64 - 1);
    if scale > 0 && adjusted >= -6 {
        let scale = scale as usize;
        return if coeff.len() > scale {
            let point = coeff.len() - scale;
            format!("{sign}{}.{}", &coeff[..point], &coeff[point..])
        } else {
            format!("{sign}0.{}{coeff}", "0".repeat(scale - coeff.len()))
        };
    }
    let mut out = format!("{sign}{}", &coeff[..1]);
    if coeff.len() > 1 {
        out.push('.');
        out.push_str(&coeff[1..]);
    }
    if adjusted != 0 {
        out.push('E');
        if adjusted > 0 {
            out.push('+');
        }
        out.push_str(&adjusted.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_java_s_value_model() {
        assert_eq!(
            parse(" [1, 2.50, \"a\\u0041\\q\", true, null, {\"k\": 1, \"j\": [], \"k\": 2}, 12345678901234567890] ")
                .unwrap(),
            Json::List(vec![
                Json::Long(1),
                Json::Decimal("2.50".to_owned()),
                Json::Str("aAq".to_owned()),
                Json::Bool(true),
                Json::Null,
                Json::Map(vec![("k".to_owned(), Json::Long(2)), ("j".to_owned(), Json::List(vec![]))]),
                Json::BigInteger("12345678901234567890".to_owned()),
            ])
        );
        // nineteen digits that fit a long are a Long
        assert_eq!(parse("0000000000000000007").unwrap(), Json::Long(7));
    }

    #[test]
    fn refuses_as_java_does() {
        assert_eq!(parse("[1] x").unwrap_err().message, "trailing JSON at 4");
        assert_eq!(parse("[1").unwrap_err().class, STRING_INDEX);
        assert_eq!(parse("x").unwrap_err().message, "For input string: \"\"");
    }

    #[test]
    fn big_decimal_text_is_java_s_to_string() {
        assert_eq!(big_decimal_text("1e5"), "1E+5");
        assert_eq!(big_decimal_text("1.50"), "1.50");
        assert_eq!(big_decimal_text("0.0000001"), "1E-7");
        assert_eq!(big_decimal_text("0.000001"), "0.000001");
        assert_eq!(big_decimal_text("-0.0"), "0.0");
        assert_eq!(big_decimal_text("1.5E-3"), "0.0015");
        assert_eq!(big_decimal_text("12.5e1"), "125");
    }

    #[test]
    fn canonical_sorts_keys() {
        assert_eq!(canonical(&parse("{\"b\": 1.0, \"a\": [\"x\\n\"]}").unwrap()), "{\"a\":[\"x\\n\"],\"b\":1.0}");
    }
}
