// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The escape table (core/'s `protocol.Escapes.unescapeJavaLike`, commons-text `unescapeJava` semantics: octal,
//! `\uXXXX` with extra `u`s, the control table, drop-backslash for anything else) and the quoted-path decoder that
//! uses it (`Protocol.unquotePath`).

use crate::jnum::digit;

/// The `IllegalArgumentException` a short unicode escape raises.
pub const SHORT_UNICODE_ESCAPE: &str = "Less than 4 hex digits in unicode escape";

/// Java's `Escapes.unescapeJavaLike`: decoded, or the `IllegalArgumentException` message of a unicode escape with
/// fewer than four hex digits. A lone trailing backslash drops.
pub fn unescape_java_like(s: &[u16]) -> Result<Vec<u16>, &'static str> {
    let bs = u16::from(b'\\');
    if !s.contains(&bs) {
        return Ok(s.to_vec());
    }
    let mut sb: Vec<u16> = Vec::with_capacity(s.len());
    let n = s.len();
    let mut i = 0;
    let is_octal = |c: u16| (u16::from(b'0')..=u16::from(b'7')).contains(&c);
    while i < n {
        let c = s[i];
        if c != bs || i + 1 >= n {
            if c != bs {
                sb.push(c);
            }
            i += 1;
            continue;
        }
        let esc = s[i + 1];
        if is_octal(esc) {
            // octal, 1-3 digits, at most \377
            let mut k = i + 1;
            let mut val: u32 = 0;
            let max = if esc <= u16::from(b'3') { 3 } else { 2 };
            while k < n && k - i <= max && is_octal(s[k]) {
                val = val * 8 + u32::from(s[k] - u16::from(b'0'));
                k += 1;
            }
            sb.push(val as u16);
            i = k;
            continue;
        }
        if esc == u16::from(b'u') {
            // backslash-u+XXXX, extra u's legal
            let mut k = i + 2;
            while k < n && s[k] == u16::from(b'u') {
                k += 1;
            }
            if k + 4 <= n {
                let mut val: u32 = 0;
                let mut ok = true;
                for h in 0..4 {
                    match digit(s[k + h], 16) {
                        Some(d) => val = val * 16 + d,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    sb.push(val as u16);
                    i = k + 4;
                    continue;
                }
            }
            return Err(SHORT_UNICODE_ESCAPE);
        }
        sb.push(match esc {
            0x6E => 0x0A, // n
            0x74 => 0x09, // t
            0x72 => 0x0D, // r
            0x62 => 0x08, // b
            0x66 => 0x0C, // f
            _ => esc,     // \' \" \\ and drop-backslash
        });
        i += 2;
    }
    Ok(sb)
}

/// Java's `Protocol.unquotePath` (`unquoteSegments`): each quoted `::` segment of a path unquoted and unescaped, the
/// segment boundaries quote-aware; a path with no quote as it is.
pub fn unquote_path(path: &[u16]) -> Result<Vec<u16>, &'static str> {
    let quote = u16::from(b'\'');
    let colon = u16::from(b':');
    if !path.contains(&quote) {
        return Ok(path.to_vec());
    }
    let mut out: Vec<u16> = Vec::new();
    let mut in_quote = false;
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= path.len() {
        let at_sep = !in_quote && i + 1 < path.len() && path[i] == colon && path[i + 1] == colon;
        if i == path.len() || at_sep {
            if start > 0 {
                out.extend_from_slice(&[colon, colon]);
            }
            let s = &path[start..i];
            if s.len() >= 2 && s[0] == quote && s[s.len() - 1] == quote {
                out.extend(unescape_java_like(&s[1..s.len() - 1])?);
            } else {
                out.extend_from_slice(s);
            }
            if i == path.len() {
                break;
            }
            i += 1; // past the second ':'
            start = i + 1;
            i += 1;
            continue;
        }
        let c = path[i];
        if in_quote && c == u16::from(b'\\') {
            i += 1;
        } else if c == quote {
            in_quote = !in_quote;
        }
        i += 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ustr::u;

    #[test]
    fn escapes_decode_as_commons_text_does() {
        assert_eq!(unescape_java_like(&u(r"aAb\101\n\q")), Ok(u("aAbA\nq")));
        assert_eq!(unescape_java_like(&u(r"x\u12")), Err(SHORT_UNICODE_ESCAPE));
        assert_eq!(unescape_java_like(&u("x\\")), Ok(u("x")));
        assert_eq!(unquote_path(&u("test::'p a c k'::A")), Ok(u("test::p a c k::A")));
        assert_eq!(unquote_path(&u(r"'b\'c'")), Ok(u("b'c")));
    }
}
