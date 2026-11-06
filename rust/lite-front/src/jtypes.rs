// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Java values the protocol records hold: a `String` as its UTF-16 units (`JStr`, a lone surrogate kept), a
//! `java.math.BigDecimal` as Java parses and prints it, and the leaves an open slot (`Number`, `Object`) holds.

use std::fmt;

/// A Java string: its UTF-16 units.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JStr(pub Vec<u16>);

impl JStr {
    pub fn units(&self) -> &[u16] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The string as Rust text, a lone surrogate as U+FFFD (for messages and comparisons, never for output).
    pub fn lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }

    pub fn eq_str(&self, s: &str) -> bool {
        self.0.iter().copied().eq(s.encode_utf16())
    }
}

impl From<&str> for JStr {
    fn from(s: &str) -> JStr {
        JStr(s.encode_utf16().collect())
    }
}

impl From<String> for JStr {
    fn from(s: String) -> JStr {
        JStr::from(s.as_str())
    }
}

impl From<&[u16]> for JStr {
    fn from(u: &[u16]) -> JStr {
        JStr(u.to_vec())
    }
}

impl PartialEq<str> for JStr {
    fn eq(&self, other: &str) -> bool {
        self.eq_str(other)
    }
}

impl PartialEq<&str> for JStr {
    fn eq(&self, other: &&str) -> bool {
        self.eq_str(other)
    }
}

impl fmt::Debug for JStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.lossy())
    }
}

impl fmt::Display for JStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.lossy())
    }
}

/// `java.math.BigDecimal`: an unscaled integer (its sign and decimal digits, no leading zero but a lone `0`) and a
/// scale, parsed as Java's `BigDecimal(String)` parses and printed as its `toString` prints.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BigDecimal {
    pub negative: bool,
    pub digits: String,
    pub scale: i32,
}

impl BigDecimal {
    /// Java's `new BigDecimal(String)`: an optional sign, digits with at most one `.`, an optional exponent.
    pub fn parse(s: &str) -> Result<BigDecimal, String> {
        let bad = || format!("Character array is missing \"exponent\" mark 'e' or 'E'. ({s})");
        let b = s.as_bytes();
        let mut i = 0;
        let mut negative = false;
        if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
            negative = b[i] == b'-';
            i += 1;
        }
        let mut digits = String::new();
        let mut fraction = 0i64;
        let mut seen_dot = false;
        let mut seen_digit = false;
        while i < b.len() {
            match b[i] {
                b'0'..=b'9' => {
                    digits.push(b[i] as char);
                    if seen_dot {
                        fraction += 1;
                    }
                    seen_digit = true;
                }
                b'.' if !seen_dot => seen_dot = true,
                b'e' | b'E' => break,
                _ => return Err(bad()),
            }
            i += 1;
        }
        if !seen_digit {
            return Err(format!("No digits found. ({s})"));
        }
        let mut exponent = 0i64;
        if i < b.len() {
            let exp = &s[i + 1..];
            exponent = exp.parse::<i64>().map_err(|_| bad())?;
        }
        let scale = fraction - exponent;
        let scale = i32::try_from(scale).map_err(|_| format!("Scale out of range. ({s})"))?;
        let trimmed = digits.trim_start_matches('0');
        let digits = if trimmed.is_empty() { "0".to_owned() } else { trimmed.to_owned() };
        let negative = negative && digits != "0";
        Ok(BigDecimal { negative, digits, scale })
    }

    /// Java's `new BigDecimal(String)` over a string's UTF-16 units, as JDK 25's
    /// `BigDecimal(char[], int, int, MathContext)` reads them: a sign, digits of any script (`Character.digit`), one
    /// point, an exponent (`parseExp`). An error is its `NumberFormatException` message -- `None` where the JDK
    /// raises one without, out of an array bound (`1.5e`, `1.5e+`, an empty string).
    pub fn parse_units(s: &[u16]) -> Result<BigDecimal, Option<JStr>> {
        let at = |i: usize| s.get(i).copied().ok_or(None);
        let message = |m: &str| Some(JStr::from(m));
        let mut offset = 0usize;
        let mut len = s.len() as i64;
        let mut negative = false;
        let first = at(offset)?;
        if first == u16::from(b'-') {
            negative = true;
            offset += 1;
            len -= 1;
        } else if first == u16::from(b'+') {
            offset += 1;
            len -= 1;
        }
        let compact = len <= 18; // MAX_COMPACT_DIGITS
        let mut dot = false;
        let mut seen_digit = false;
        let mut digits = String::new();
        let mut scale: i64 = 0;
        while len > 0 {
            let c = at(offset)?;
            let value = if (u16::from(b'0')..=u16::from(b'9')).contains(&c) {
                Some(u32::from(c - u16::from(b'0')))
            } else if lite_javacompat::character::is_digit(c) {
                crate::jnum::digit(c, 10)
            } else {
                None
            };
            if let Some(v) = value {
                seen_digit = true;
                if v != 0 || !digits.is_empty() {
                    digits.push(char::from_digit(v, 10).expect("a decimal digit"));
                }
                if dot {
                    scale += 1;
                }
            } else if c == u16::from(b'.') {
                if dot {
                    return Err(message("Character array contains more than one decimal point."));
                }
                dot = true;
            } else if c == u16::from(b'e') || c == u16::from(b'E') {
                scale -= Self::parse_exp(s, offset, len)?;
                break;
            } else if compact {
                let mut m: Vec<u16> = "Character ".encode_utf16().collect();
                m.push(c);
                m.extend(
                    " is neither a decimal digit number, decimal point, nor \"e\" notation exponential mark."
                        .encode_utf16(),
                );
                return Err(Some(JStr(m)));
            } else {
                return Err(message("Character array is missing \"e\" notation exponential mark."));
            }
            offset += 1;
            len -= 1;
        }
        if !seen_digit {
            return Err(message("No digits found."));
        }
        let scale = i32::try_from(scale).map_err(|_| message("Exponent overflow."))?;
        let digits = if digits.is_empty() { "0".to_owned() } else { digits };
        let negative = negative && digits != "0";
        Ok(BigDecimal { negative, digits, scale })
    }

    /// JDK 25's `BigDecimal.parseExp`: the exponent after the `e` at `offset`, `len` units left from it.
    fn parse_exp(s: &[u16], mut offset: usize, mut len: i64) -> Result<i64, Option<JStr>> {
        let at = |i: usize| s.get(i).copied().ok_or(None);
        let message = |m: &str| Some(JStr::from(m));
        let mut exp: i64 = 0;
        offset += 1;
        let mut c = at(offset)?;
        len -= 1;
        let negexp = c == u16::from(b'-');
        if negexp || c == u16::from(b'+') {
            offset += 1;
            c = at(offset)?;
            len -= 1;
        }
        if len <= 0 {
            return Err(message("No exponent digits."));
        }
        let digit = |c: u16| crate::jnum::digit(c, 10);
        while len > 10 && (c == u16::from(b'0') || digit(c) == Some(0)) {
            offset += 1;
            c = at(offset)?;
            len -= 1;
        }
        if len > 10 {
            return Err(message("Too many nonzero exponent digits."));
        }
        loop {
            let v = if (u16::from(b'0')..=u16::from(b'9')).contains(&c) {
                i64::from(c - u16::from(b'0'))
            } else {
                match digit(c) {
                    Some(v) => i64::from(v),
                    None => return Err(message("Not a digit.")),
                }
            };
            exp = exp * 10 + v;
            if len == 1 {
                break;
            }
            offset += 1;
            c = at(offset)?;
            len -= 1;
        }
        Ok(if negexp { -exp } else { exp })
    }

    pub fn negate(&self) -> BigDecimal {
        BigDecimal { negative: !self.negative && self.digits != "0", digits: self.digits.clone(), scale: self.scale }
    }

    /// Java's `compareTo`: the numeric values compared, scale ignored (`2.0` equals `2.00`).
    pub fn compare_to(&self, other: &BigDecimal) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let sign = |d: &BigDecimal| {
            if d.digits == "0" {
                0
            } else if d.negative {
                -1
            } else {
                1
            }
        };
        let (a, b) = (sign(self), sign(other));
        if a != b || a == 0 {
            return a.cmp(&b);
        }
        // the same nonzero sign: compare magnitudes, the larger adjusted exponent the larger, then the digits
        let adjusted = |d: &BigDecimal| d.digits.len() as i64 - 1 - i64::from(d.scale);
        let magnitude = match adjusted(self).cmp(&adjusted(other)) {
            Ordering::Equal => {
                let width = self.digits.len().max(other.digits.len());
                let pad = |s: &str| format!("{s:0<width$}");
                pad(&self.digits).cmp(&pad(&other.digits))
            }
            o => o,
        };
        if a < 0 { magnitude.reverse() } else { magnitude }
    }

    /// Java's `BigDecimal.toString` (scientific notation where the exponent asks for it).
    pub fn to_java_string(&self) -> String {
        let coeff = &self.digits;
        let len = coeff.len() as i64;
        let scale = i64::from(self.scale);
        let adjusted = -scale + (len - 1);
        let mut out = String::new();
        if self.negative {
            out.push('-');
        }
        if scale == 0 {
            out.push_str(coeff);
        } else if scale > 0 && adjusted >= -6 {
            let pad = scale - len;
            if pad >= 0 {
                out.push_str("0.");
                for _ in 0..pad {
                    out.push('0');
                }
                out.push_str(coeff);
            } else {
                let split = (-pad) as usize;
                out.push_str(&coeff[..split]);
                out.push('.');
                out.push_str(&coeff[split..]);
            }
        } else {
            out.push_str(&coeff[..1]);
            if len > 1 {
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
        }
        out
    }
}

/// A leaf in an open slot (a component declared `Number` or `Object`), by its Java class.
#[derive(Clone, Debug, PartialEq)]
pub enum JValue {
    String(JStr),
    Long(i64),
    Integer(i32),
    Boolean(bool),
    Double(f64),
    /// `java.math.BigInteger`: its decimal digits, signed.
    BigInteger(String),
    BigDecimal(BigDecimal),
}

impl JValue {
    pub fn java_class(&self) -> &'static str {
        match self {
            JValue::String(_) => "java.lang.String",
            JValue::Long(_) => "java.lang.Long",
            JValue::Integer(_) => "java.lang.Integer",
            JValue::Boolean(_) => "java.lang.Boolean",
            JValue::Double(_) => "java.lang.Double",
            JValue::BigInteger(_) => "java.math.BigInteger",
            JValue::BigDecimal(_) => "java.math.BigDecimal",
        }
    }
}

/// Java's `String.hashCode()` over UTF-16 units: `s[0]*31^(n-1) + ... + s[n-1]`, wrapping.
pub fn string_hash(s: &[u16]) -> i32 {
    s.iter().fold(0i32, |h, &u| h.wrapping_mul(31).wrapping_add(i32::from(u)))
}

/// The order a `java.util.HashMap<String, V>`'s key set iterates `keys` (distinct, put in turn): by bucket under the
/// final table size (16, doubled whenever the size passes three quarters of it), a bucket's keys in insertion order
/// (`resize` splits a bucket keeping its order) -- as indexes into `keys`. A bucket of eight or more keys is a tree in
/// Java, whose order this does not model; no grammar's map holds that many.
pub fn hash_map_order(keys: &[Vec<u16>]) -> Vec<usize> {
    let mut capacity: usize = 16;
    while keys.len() > capacity * 3 / 4 {
        capacity *= 2;
    }
    let bucket = |k: &[u16]| {
        let h = string_hash(k) as u32;
        ((h ^ (h >> 16)) as usize) & (capacity - 1)
    };
    let mut ordered: Vec<(usize, usize)> = keys.iter().enumerate().map(|(i, k)| (bucket(k), i)).collect();
    ordered.sort();
    ordered.into_iter().map(|(_, i)| i).collect()
}

/// `java.util.Base64.getEncoder().encodeToString(s.getBytes(StandardCharsets.UTF_8))`: the string's UTF-8 bytes (a
/// lone surrogate replaced by `?`, as the encoder does), in the padded standard alphabet.
pub fn base64_of_utf8(s: &[u16]) -> String {
    let mut bytes: Vec<u8> = Vec::with_capacity(s.len());
    for r in char::decode_utf16(s.iter().copied()) {
        match r {
            Ok(ch) => {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
            Err(_) => bytes.push(b'?'),
        }
    }
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_map_and_base64_as_the_jdk_has_them() {
        let k = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        // JDK 25's key sets (2026-11-05): a HashMap put with each list in turn
        for (put, iterated) in [
            (
                &["to", "from", "schemaFrom", "schemaTo", "x", "zz"][..],
                &["zz", "schemaFrom", "schemaTo", "x", "from", "to"][..],
            ),
            (&["from", "to", "bogus", "other"][..], &["bogus", "other", "from", "to"][..]),
        ] {
            let keys: Vec<Vec<u16>> = put.iter().map(|s| k(s)).collect();
            let order: Vec<Vec<u16>> = hash_map_order(&keys).into_iter().map(|i| keys[i].clone()).collect();
            assert_eq!(order, iterated.iter().map(|s| k(s)).collect::<Vec<_>>());
        }
        assert_eq!(base64_of_utf8(&k("hello")), "aGVsbG8=");
        assert_eq!(base64_of_utf8(&k("")), "");
        // "h\uD800é": the lone surrogate as '?'
        assert_eq!(base64_of_utf8(&[u16::from(b'h'), 0xD800, 0xE9]), "aD/DqQ==");
    }

    fn s(x: &str) -> String {
        BigDecimal::parse(x).unwrap().to_java_string()
    }

    #[test]
    fn big_decimal_prints_as_java_prints_it() {
        // each pair: Java's new BigDecimal(text).toString()
        for (text, java) in [
            ("1.0", "1.0"),
            ("0.00", "0.00"),
            ("-3.14159", "-3.14159"),
            ("1E+3", "1E+3"),
            ("1e3", "1E+3"),
            ("12.5e-10", "1.25E-9"),
            ("0.0000001", "1E-7"),
            ("0.000001", "0.000001"),
            ("123456789012345678901234567890.123", "123456789012345678901234567890.123"),
            ("-0.0", "0.0"),
            ("100", "100"),
            ("1.50E2", "150"),
            (".5", "0.5"),
        ] {
            assert_eq!(s(text), java, "{text}");
        }
        assert_eq!(BigDecimal::parse("2.5").unwrap().negate().to_java_string(), "-2.5");
        // JDK 25's BigDecimal(char[]...) over units: its values and its NumberFormatException messages
        let units = |s: &str| BigDecimal::parse_units(&s.encode_utf16().collect::<Vec<u16>>());
        assert_eq!(units("1.50E2").unwrap().to_java_string(), "150");
        assert_eq!(units("-0.000").unwrap().to_java_string(), "0.000");
        assert_eq!(units("1e\u{0665}").unwrap().to_java_string(), "1E+5");
        assert_eq!(units("1.5e"), Err(None));
        assert_eq!(units("1.5e+"), Err(None));
        assert_eq!(units("1e99999999999"), Err(Some(JStr::from("Too many nonzero exponent digits."))));
        assert_eq!(units("1e-2147483649"), Err(Some(JStr::from("Exponent overflow."))));
        assert_eq!(units("1..2"), Err(Some(JStr::from("Character array contains more than one decimal point."))));
        assert_eq!(units("."), Err(Some(JStr::from("No digits found."))));
        assert_eq!(
            BigDecimal::parse("2.0").unwrap().compare_to(&BigDecimal::parse("2.00").unwrap()),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            BigDecimal::parse("-3").unwrap().compare_to(&BigDecimal::parse("-2.5").unwrap()),
            std::cmp::Ordering::Less
        );
    }
}
