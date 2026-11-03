// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `Double.toString` as the JDK writes it since JDK 19 (the shortest-decimal specification), the JDK the corpora were
//! answered on (25): `1.0`, `0.001`, `1.0E7`, `4.9E-324`.

/// `v` as Java's `Double.toString(v)` writes it.
///
/// The decimal is the JDK's choice: among the decimals that round to `v`, those of the fewest digits -- or, when that
/// is one digit, those of one or two -- and of them the closest to `v`. Its layout is Java's: plain, with at least one
/// digit after the point, when 10<sup>-3</sup> &le; |v| &lt; 10<sup>7</sup>; otherwise one digit, the point, at least
/// one more digit, `E` and the exponent.
pub fn double_to_string(v: f64) -> String {
    if v.is_nan() {
        return "NaN".to_owned();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0" } else { "0.0" }.to_owned();
    }
    let (digits, exponent) = decimal(v.abs());
    let mut out = String::with_capacity(digits.len() + 8);
    if v < 0.0 {
        out.push('-');
    }
    if (-3..7).contains(&exponent) {
        plain(&mut out, &digits, exponent);
    } else {
        scientific(&mut out, &digits, exponent);
    }
    out
}

/// The decimal the JDK selects for a positive finite `v`: its significant digits, without trailing zeros, and the
/// power of ten of the first, so that `v` reads `d.ddd × 10^exponent`.
fn decimal(v: f64) -> (String, i32) {
    // Rust's shortest form: the fewest digits that round to v
    let (digits, exponent) = split(&format!("{v:e}"));
    // the JDK's choice among decimals of that length -- of one or two digits when the fewest is one -- is the
    // closest to v, the even last digit on a tie: the correctly rounded decimal of that length (Rust's fixed
    // precision rounds ties to even), when it rounds to v. It differs from the shortest form on a tie
    // (2.9802322387695312E-8, the shortest form ending in 3) and where one digit is the fewest (4.9E-324, not 5E-324).
    let length = digits.len().max(2);
    let rounded = format!("{v:.*e}", length - 1);
    if rounded.parse::<f64>() == Ok(v) {
        return split(&rounded);
    }
    (digits, exponent)
}

/// `d[.ddd]e<exponent>` as its digits, trailing zeros dropped, and its exponent.
fn split(scientific: &str) -> (String, i32) {
    let (mantissa, exponent) = scientific.split_once('e').expect("Rust's {:e} writes an exponent");
    let mut digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    (digits, exponent.parse().expect("Rust's {:e} writes an integer exponent"))
}

/// `ddd.ddd`: the digits around the point, at least one digit after it.
fn plain(out: &mut String, digits: &str, exponent: i32) {
    if exponent >= 0 {
        let whole = exponent as usize + 1;
        if digits.len() <= whole {
            out.push_str(digits);
            out.extend(std::iter::repeat_n('0', whole - digits.len()));
            out.push_str(".0");
        } else {
            out.push_str(&digits[..whole]);
            out.push('.');
            out.push_str(&digits[whole..]);
        }
    } else {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-exponent - 1) as usize));
        out.push_str(digits);
    }
}

/// `d.dddE<exponent>`: one digit, the point, at least one digit after it.
fn scientific(out: &mut String, digits: &str, exponent: i32) {
    out.push_str(&digits[..1]);
    out.push('.');
    out.push_str(if digits.len() > 1 { &digits[1..] } else { "0" });
    out.push('E');
    out.push_str(&exponent.to_string());
}

#[cfg(test)]
mod tests {
    use super::double_to_string;

    #[test]
    fn spells_as_the_jdk_does() {
        for (v, java) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (1.0, "1.0"),
            (-2.5, "-2.5"),
            (100.0, "100.0"),
            (0.1, "0.1"),
            (1.0 / 3.0, "0.3333333333333333"),
            (0.001, "0.001"),
            (0.0001, "1.0E-4"),
            (9_999_999.0, "9999999.0"),
            (10_000_000.0, "1.0E7"),
            (1e23, "1.0E23"),
            (123_456_789.0, "1.23456789E8"),
            (f64::MAX, "1.7976931348623157E308"),
            (f64::MIN_POSITIVE, "2.2250738585072014E-308"),
            (f64::from_bits(1), "4.9E-324"),
            (-f64::from_bits(1), "-4.9E-324"),
            // a tie between two shortest decimals: the even last digit
            (f64::from_bits(0x3e60_0000_0000_0000), "2.9802322387695312E-8"),
        ] {
            assert_eq!(double_to_string(v), java, "{v:e}");
        }
    }
}
