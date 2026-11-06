// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The JDK's number readers the parser calls, as they read: `Character.digit` (any script's decimal digit, the Latin
//! and fullwidth letters above 9), `Long.parseLong` and `Integer.parseInt` (through `Character.digit`, so a Devanagari
//! `५` is 5), `Double.parseDouble` (its decimal grammar: ASCII digits only, an optional `fFdD` suffix), and their
//! `NumberFormatException` messages.

use crate::jcat;
use crate::jtypes::JStr;
use lite_javacompat::character::is_digit;

/// Java's `Character.digit(ch, radix)`: the digit's value when below `radix`.
pub fn digit(c: u16, radix: u32) -> Option<u32> {
    let v = if is_digit(c) {
        // a decimal digit's value: Unicode encodes every DECIMAL_DIGIT_NUMBER in runs of 0..9, so the digits
        // contiguously before it, modulo ten
        let mut back = 0u32;
        while back < u32::from(c) && is_digit(u32::from(c) - back - 1) {
            back += 1;
        }
        back % 10
    } else {
        match c {
            0x41..=0x5A => u32::from(c) - 0x41 + 10,
            0x61..=0x7A => u32::from(c) - 0x61 + 10,
            0xFF21..=0xFF3A => u32::from(c) - 0xFF21 + 10,
            0xFF41..=0xFF5A => u32::from(c) - 0xFF41 + 10,
            _ => return None,
        }
    };
    (v < radix).then_some(v)
}

/// `NumberFormatException.forInputString(s, 10)`'s message.
fn for_input_string(s: &[u16]) -> JStr {
    jcat!("For input string: \"", s, "\"")
}

/// Java's `Long.parseLong(s)` (radix 10), or its `NumberFormatException` message.
pub fn parse_long(s: &[u16]) -> Result<i64, JStr> {
    parse_signed(s, i64::MIN, i64::MAX)
}

/// Java's `Integer.parseInt(s)` (radix 10), or its `NumberFormatException` message.
pub fn parse_int(s: &[u16]) -> Result<i32, JStr> {
    parse_signed(s, i64::from(i32::MIN), i64::from(i32::MAX)).map(|v| v as i32)
}

/// The JDK's `parseLong`/`parseInt` loop: accumulated negatively against the type's limit.
fn parse_signed(s: &[u16], min: i64, max: i64) -> Result<i64, JStr> {
    if s.is_empty() {
        return Err(for_input_string(s));
    }
    let mut i = 0;
    let mut negative = false;
    let mut limit = -max;
    let first = s[0];
    if first < u16::from(b'0') {
        if first == u16::from(b'-') {
            negative = true;
            limit = min;
        } else if first != u16::from(b'+') {
            return Err(for_input_string(s));
        }
        if s.len() == 1 {
            return Err(for_input_string(s));
        }
        i += 1;
    }
    let multmin = limit / 10;
    let mut result: i64 = 0;
    while i < s.len() {
        let d = match digit(s[i], 10) {
            Some(d) => i64::from(d),
            None => return Err(for_input_string(s)),
        };
        i += 1;
        if result < multmin {
            return Err(for_input_string(s));
        }
        result *= 10;
        if result < limit + d {
            return Err(for_input_string(s));
        }
        result -= d;
    }
    Ok(if negative { result } else { -result })
}

/// Java's `Double.parseDouble(s)` over its decimal grammar (`FloatingDecimal.readJavaFormatString`): trimmed, a
/// sign, `NaN` or `Infinity`, or ASCII digits with at most one point, an exponent, an `fFdD` suffix -- the shapes the
/// lexer and the GraphQL reader hand it -- correctly rounded; anything else its `NumberFormatException` message.
pub fn parse_double(s: &[u16]) -> Result<f64, JStr> {
    let t = crate::ustr::trim(s);
    if t.is_empty() {
        return Err(JStr::from("empty String"));
    }
    let bad = || jcat!("For input string: \"", t, "\"");
    let ascii: Option<String> = t.iter().map(|&c| char::from_u32(u32::from(c)).filter(char::is_ascii)).collect();
    let Some(text) = ascii else { return Err(bad()) };
    let b = text.as_bytes();
    let mut i = 0;
    let negative = b[0] == b'-';
    if b[0] == b'-' || b[0] == b'+' {
        i = 1;
    }
    let rest = &text[i..];
    if rest == "NaN" {
        return Ok(f64::NAN);
    }
    if rest == "Infinity" {
        return Ok(if negative { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    let mut j = i;
    let mut digits = 0;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
        digits += 1;
    }
    if j < b.len() && b[j] == b'.' {
        j += 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return Err(bad());
    }
    if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
        j += 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let es = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == es {
            return Err(bad());
        }
    }
    let number_end = j;
    if j < b.len() && matches!(b[j], b'f' | b'F' | b'd' | b'D') {
        j += 1;
    }
    if j != b.len() {
        return Err(bad());
    }
    text[..number_end].parse::<f64>().map_err(|_| bad())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ustr::u;

    #[test]
    fn numbers_read_as_the_jdk_reads_them() {
        assert_eq!(parse_long(&u("9223372036854775807")), Ok(i64::MAX));
        assert_eq!(parse_long(&u("-9223372036854775808")), Ok(i64::MIN));
        assert_eq!(parse_long(&u("9223372036854775808")), Err(JStr::from("For input string: \"9223372036854775808\"")));
        assert_eq!(parse_long(&u("\u{0967}\u{0968}")), Ok(12));
        assert!(parse_int(&u("2147483648")).is_err());
        assert_eq!(digit(0xFF21, 16), Some(10));
        assert_eq!(digit(u16::from(b'g'), 16), None);
        assert_eq!(parse_double(&u("1.5e3f")), Ok(1500.0));
        assert_eq!(parse_double(&u("1.")), Ok(1.0));
        assert!(parse_double(&u("1\u{0967}")).is_err());
    }
}
