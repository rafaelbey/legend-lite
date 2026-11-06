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

    pub fn negate(&self) -> BigDecimal {
        BigDecimal { negative: !self.negative && self.digits != "0", digits: self.digits.clone(), scale: self.scale }
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
