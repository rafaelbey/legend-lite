// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The JDK's number readers the parser calls, as they read: `Character.digit` (any script's decimal digit, the Latin
//! and fullwidth letters above 9), `Long.parseLong` and `Integer.parseInt` (through `Character.digit`, so a Devanagari
//! `५` is 5), `Double.parseDouble` (JDK 25's reader: ASCII digits, one point at most, an optional `fFdD` suffix, the
//! hexadecimal form), and their `NumberFormatException` messages.

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

/// `FloatingDecimal.check`'s message: the input as given (untrimmed), its middle elided past 1,000 units.
fn fd_for_input_string(s: &[u16]) -> JStr {
    const MAX_OUT: usize = 1_000;
    const L_HALF: usize = (MAX_OUT - 5) / 2;
    const R_HALF: usize = MAX_OUT - (L_HALF + 5);
    if s.len() > MAX_OUT {
        let mut t = s[..L_HALF].to_vec();
        t.extend(" ... ".encode_utf16());
        t.extend_from_slice(&s[s.len() - R_HALF..]);
        return jcat!("For input string: \"", t, "\"");
    }
    jcat!("For input string: \"", s, "\"")
}

/// `skipWhitespaces`: every unit at most `' '`.
fn skip_ws(s: &[u16], mut i: usize) -> usize {
    while i < s.len() && s[i] <= 0x20 {
        i += 1;
    }
    i
}

/// `FloatingDecimal.isDigit(ch, isDec)`: an ASCII digit, or a hex letter.
fn fd_digit(ch: u16, is_dec: bool) -> bool {
    (0x30..=0x39).contains(&ch) || (!is_dec && (0x61..=0x66).contains(&(ch | 0x20)))
}

/// Java's `Double.parseDouble(s)` as JDK 25 reads it (`FloatingDecimal.readJavaFormatString`, its scan ported step by
/// step): leading and trailing units at most `' '` skipped, a sign, `Infinity` or `NaN`, a decimal or `0x` hexadecimal
/// significand (ASCII digits, at most one point), an exponent (`e`, or `p` for hex, required there), an `fFdD`
/// suffix; its `NumberFormatException` messages (`empty String`, `multiple points`, `For input string: "..."`). A
/// decimal value is correctly rounded (the JDK's and Rust's readers both are); a hexadecimal one is rounded as the
/// JDK's own hex path rounds it.
pub fn parse_double(s: &[u16]) -> Result<f64, JStr> {
    let len = s.len();
    let check = |ok: bool| if ok { Ok(()) } else { Err(fd_for_input_string(s)) };
    let mut i = skip_ws(s, 0);
    if i == len {
        return Err(JStr::from("empty String"));
    }
    let sign_at = i;
    let negative = s[i] == u16::from(b'-');
    if s[i] == u16::from(b'-') || s[i] == u16::from(b'+') {
        i += 1;
    }
    let mut is_dec = true;
    if i < len {
        let ch = s[i];
        let symbolic = |sub: &str| -> Result<(), JStr> {
            let w: Vec<u16> = sub.encode_utf16().collect();
            let high = i + w.len();
            check(high <= len && s[i..high] == w[..] && skip_ws(s, high) == len)
        };
        if ch == u16::from(b'I') {
            symbolic("Infinity")?;
            return Ok(if negative { f64::NEG_INFINITY } else { f64::INFINITY });
        }
        if ch == u16::from(b'N') {
            symbolic("NaN")?;
            return Ok(f64::NAN);
        }
        if ch == u16::from(b'0') && i + 1 < len && (s[i + 1] | 0x20) == u16::from(b'x') {
            is_dec = false;
            i += 2;
        }
    }
    let point = u16::from(b'.');
    let mut pt = 0usize;
    let start = i;
    let multiple_points = || JStr::from("multiple points");
    while i < len && (s[i] == u16::from(b'0') || s[i] == point) {
        let ch = s[i];
        i += 1;
        if ch == point {
            if pt != 0 {
                return Err(multiple_points());
            }
            pt = i;
        }
    }
    let lz = i;
    let mut tnz = 0usize;
    while i < len && (fd_digit(s[i], is_dec) || s[i] == point) {
        let ch = s[i];
        i += 1;
        if ch == point {
            if pt != 0 {
                return Err(multiple_points());
            }
            pt = i;
        } else if ch != u16::from(b'0') {
            tnz = i;
        }
    }
    check(i - start > usize::from(pt != 0))?;
    let stop = i;
    let mut ep: i64 = 0;
    let mut has_exp = false;
    if i < len && (((s[i] | 0x20) == u16::from(b'e') && is_dec) || ((s[i] | 0x20) == u16::from(b'p') && !is_dec)) {
        i += 1;
        let mut esign = 0u16;
        if i < len && (s[i] == u16::from(b'-') || s[i] == u16::from(b'+')) {
            esign = s[i];
            i += 1;
        }
        while i < len && fd_digit(s[i], true) {
            // appendDecDigit: clamped at 10^10
            ep = if ep < 1_000_000_000 {
                10 * ep + i64::from(s[i] - 0x30)
            } else {
                10_000_000_000
            };
            i += 1;
        }
        check(i - stop >= 3 || (i - stop == 2 && esign == 0))?;
        if esign == u16::from(b'-') {
            ep = -ep;
        }
        has_exp = true;
    }
    check(is_dec || has_exp)?;
    let number_end = i;
    if i < len && ((s[i] | 0x20) == u16::from(b'f') || (s[i] | 0x20) == u16::from(b'd')) {
        i += 1;
    }
    check(skip_ws(s, i) == len)?;
    if tnz == 0 {
        return Ok(if negative { -0.0 } else { 0.0 });
    }
    if is_dec {
        // the scanned sign, significand and exponent are ASCII
        let text: String = s[sign_at..number_end].iter().map(|&c| char::from(c as u8)).collect();
        return text.parse::<f64>().map_err(|_| fd_for_input_string(s));
    }
    Ok(hex_double(s, negative, lz, tnz, stop, pt, ep))
}

/// `readJavaFormatString`'s hexadecimal conversion for a double: the significant digits `[lz, tnz)` (a point skipped)
/// times `2^ep`, rounded half to even, as the JDK computes it.
fn hex_double(s: &[u16], negative: bool, lz: usize, tnz: usize, stop: usize, pt: usize, mut ep: i64) -> f64 {
    const P: i64 = 53;
    const E_MAX: i64 = 1023;
    const E_MIN: i64 = -1022;
    const Q_MIN: i64 = E_MIN - (P - 1);
    const HEX_COUNT: i64 = P / 4 + 2;
    let mut n = (tnz - lz) as i64;
    if pt == 0 {
        ep += 4 * (stop - tnz) as i64;
    } else {
        ep += 4 * (pt as i64 - tnz as i64);
        if pt > tnz {
            ep -= 4;
        } else if lz < pt {
            n -= 1;
        }
    }
    let le = n.min(HEX_COUNT);
    let (mut j, mut i, mut c) = (0i64, lz, 0i64);
    while j < le {
        let ch = s[i];
        i += 1;
        if ch != u16::from(b'.') {
            j += 1;
            let d = if ch <= u16::from(b'9') {
                ch - 0x30
            } else {
                (ch | 0x20) - 0x61 + 10
            };
            c = (c << 4) | i64::from(d);
        }
    }
    if n > le {
        c |= 1;
        ep += 4 * (n - le);
    }
    let bl = 64 - i64::from(c.leading_zeros());
    let signed = |v: f64| if negative { -v } else { v };
    if ep < Q_MIN - bl {
        return signed(0.0);
    }
    if ep > E_MAX - bl + 1 {
        return signed(f64::INFINITY);
    }
    let mut q = ep;
    let shr = if q > E_MIN - bl {
        let shr = bl - P;
        q += shr;
        shr
    } else {
        let shr = Q_MIN - q;
        q = Q_MIN;
        shr
    };
    if shr > 0 {
        let thr = 1i64 << shr;
        let tail = (c & (thr - 1)) << 1;
        c = ((c as u64) >> shr) as i64;
        if tail > thr || (tail == thr && (c & 1) != 0) {
            c += 1;
            if c >= 1i64 << P {
                c = ((c as u64) >> 1) as i64;
                q += 1;
            }
        }
    } else {
        c <<= -shr;
    }
    let be: i64 = if c < 1i64 << (P - 1) { 0 } else { q + (1023 - 1) + P };
    let bits = (u64::from(negative) << 63) | ((be as u64) << (P - 1)) | ((c as u64) & ((1u64 << 52) - 1));
    f64::from_bits(bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ustr::u;

    #[test]
    fn numbers_read_as_the_jdk_reads_them() {
        assert_eq!(parse_long(&u("9223372036854775807")), Ok(i64::MAX));
        assert_eq!(parse_long(&u("-9223372036854775808")), Ok(i64::MIN));
        assert_eq!(
            parse_long(&u("9223372036854775808")),
            Err(JStr::from("For input string: \"9223372036854775808\""))
        );
        assert_eq!(parse_long(&u("\u{0967}\u{0968}")), Ok(12));
        assert!(parse_int(&u("2147483648")).is_err());
        assert_eq!(digit(0xFF21, 16), Some(10));
        assert_eq!(digit(u16::from(b'g'), 16), None);
        assert_eq!(parse_double(&u("1.5e3f")), Ok(1500.0));
        assert_eq!(parse_double(&u("1.")), Ok(1.0));
        assert!(parse_double(&u("1\u{0967}")).is_err());
        assert_eq!(parse_double(&u("1.2.3")), Err(JStr::from("multiple points")));
        assert_eq!(parse_double(&u("..")), Err(JStr::from("multiple points")));
        assert_eq!(parse_double(&u(" -")), Err(JStr::from("For input string: \" -\"")));
        assert_eq!(parse_double(&u("  ")), Err(JStr::from("empty String")));
        assert_eq!(parse_double(&u(" 2.5d ")), Ok(2.5));
        assert_eq!(parse_double(&u("1e99999999999999999999")), Ok(f64::INFINITY));
        assert_eq!(
            parse_double(&u("-1e-99999999999999999999")).map(f64::to_bits),
            Ok((-0.0f64).to_bits())
        );
        assert!(parse_double(&u("-NaN")).is_ok_and(f64::is_nan));
        assert!(parse_double(&u("Infinityx")).is_err());
        assert_eq!(parse_double(&u("0x1p3")), Ok(8.0));
        assert_eq!(parse_double(&u("0X1.8P1")), Ok(3.0));
        assert_eq!(parse_double(&u("0x.8p0")), Ok(0.5));
        assert_eq!(parse_double(&u("0x1p-1074")), Ok(f64::from_bits(1)));
        assert_eq!(parse_double(&u("0x1p-1075")), Ok(0.0));
        assert_eq!(parse_double(&u("0x1.8p-1074")), Ok(f64::from_bits(2)));
        assert_eq!(parse_double(&u("0x1.fffffffffffffp1023")), Ok(f64::MAX));
        assert_eq!(parse_double(&u("0x1.fffffffffffff8p1023")), Ok(f64::INFINITY));
        assert_eq!(parse_double(&u("0x1p1024")), Ok(f64::INFINITY));
        assert_eq!(parse_double(&u("0x1.00000000000008p0")), Ok(1.0));
        assert_eq!(parse_double(&u("0x1.00000000000018p0")), Ok(1.0 + 2.0 * f64::EPSILON));
        assert_eq!(
            parse_double(&u("0x123456789abcdef0123p0")),
            Ok(f64::from_bits(0x4472_3456_789a_bcdf))
        );
        assert_eq!(parse_double(&u("0x0.fffffffffffff8p-1022")), Ok(f64::MIN_POSITIVE));
        assert_eq!(parse_double(&u("0x1.ffffffffffffffp-1023")), Ok(f64::MIN_POSITIVE));
        assert!(parse_double(&u("0x1")).is_err());
        let long: Vec<u16> = std::iter::repeat_n(u16::from(b'x'), 1_200).collect();
        let message = parse_double(&long).unwrap_err();
        assert_eq!(message.len(), "For input string: \"\"".len() + 1_000);
    }
}
