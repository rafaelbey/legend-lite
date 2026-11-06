// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `new java.net.URI(String)` as the parser calls it (the Elasticsearch connection's `clusterDetails`, which keeps the
//! text when it parses and refuses it when it throws): `URI.Parser.parse(false)` of the JDK the oracle ran (25),
//! ported for its verdict alone -- the RFC 2396 scan with Java's deviations (RFC 2732 IPv6, an empty authority before
//! a path, an empty relative path), the server-based authority tried first and a registry-based one as its fallback,
//! escape pairs and visible non-ASCII characters where escapes are allowed. `URI.toString` of a parsed text is the text.

use lite_javacompat::character::is_space_char;

// The character-class masks (`URI.L_*`/`H_*`): a low mask for 0..63, a high mask for 64..127.
const L_DIGIT: u64 = 0x3FF000000000000;
const H_DIGIT: u64 = 0;
const L_UPALPHA: u64 = 0;
const H_UPALPHA: u64 = 0x7FFFFFE;
const L_LOWALPHA: u64 = 0;
const H_LOWALPHA: u64 = 0x7FFFFFE00000000;
const L_ALPHA: u64 = L_LOWALPHA | L_UPALPHA;
const H_ALPHA: u64 = H_LOWALPHA | H_UPALPHA;
const L_ALPHANUM: u64 = L_DIGIT | L_ALPHA;
const H_ALPHANUM: u64 = H_DIGIT | H_ALPHA;
const L_HEX: u64 = L_DIGIT;
const H_HEX: u64 = 0x7E0000007E;
const L_MARK: u64 = 0x678200000000;
const H_MARK: u64 = 0x4000000080000000;
const L_UNRESERVED: u64 = L_ALPHANUM | L_MARK;
const H_UNRESERVED: u64 = H_ALPHANUM | H_MARK;
const L_RESERVED: u64 = 0xAC00985000000000;
const H_RESERVED: u64 = 0x28000001;
const L_ESCAPED: u64 = 1;
const H_ESCAPED: u64 = 0;
const L_URIC: u64 = L_RESERVED | L_UNRESERVED | L_ESCAPED;
const H_URIC: u64 = H_RESERVED | H_UNRESERVED | H_ESCAPED;
const L_PCHAR: u64 = L_UNRESERVED | L_ESCAPED | 0x2400185000000000;
const H_PCHAR: u64 = H_UNRESERVED | H_ESCAPED | 0x1;
const L_PATH: u64 = L_PCHAR | 0x800800000000000;
const H_PATH: u64 = H_PCHAR;
const L_DASH: u64 = 0x200000000000;
const H_DASH: u64 = 0;
const L_DOT: u64 = 0x400000000000;
const H_DOT: u64 = 0;
const L_USERINFO: u64 = L_UNRESERVED | L_ESCAPED | 0x2C00185000000000;
const H_USERINFO: u64 = H_UNRESERVED | H_ESCAPED;
const L_REG_NAME: u64 = L_UNRESERVED | L_ESCAPED | 0x2C00185000000000;
const H_REG_NAME: u64 = H_UNRESERVED | H_ESCAPED | 0x1;
const L_SERVER: u64 = L_USERINFO | L_ALPHANUM | L_DASH | 0x400400000000000;
const H_SERVER: u64 = H_USERINFO | H_ALPHANUM | H_DASH | 0x28000001;
const L_SERVER_PERCENT: u64 = L_SERVER | 0x2000000000;
const H_SERVER_PERCENT: u64 = H_SERVER;
const L_SCHEME: u64 = L_ALPHA | L_DIGIT | 0x680000000000;
const H_SCHEME: u64 = H_ALPHA | H_DIGIT;
const L_SCOPE_ID: u64 = L_ALPHANUM | 0x400000000000;
const H_SCOPE_ID: u64 = H_ALPHANUM | 0x80000000;

/// `URI.match(c, lowMask, highMask)`.
fn matches(c: u16, low: u64, high: u64) -> bool {
    if c == 0 {
        return false;
    }
    if c < 64 {
        return (1u64 << c) & low != 0;
    }
    if c < 128 {
        return (1u64 << (c - 64)) & high != 0;
    }
    false
}

/// `Character.isISOControl`.
fn is_iso_control(c: u16) -> bool {
    c <= 0x1F || (0x7F..=0x9F).contains(&c)
}

/// The parse failed (`URISyntaxException`): the verdict alone is kept.
struct Fail;

type P<T> = Result<T, Fail>;

struct Parser<'a> {
    input: &'a [u16],
    ipv6_byte_count: i32,
}

impl Parser<'_> {
    fn ch(&self, p: usize) -> u16 {
        self.input[p]
    }

    fn at(&self, start: usize, end: usize, c: char) -> bool {
        start < end && self.ch(start) == c as u16
    }

    fn at_str(&self, start: usize, end: usize, s: &str) -> bool {
        let s: Vec<u16> = s.encode_utf16().collect();
        if s.len() > end.saturating_sub(start) || start > end {
            return false;
        }
        self.input[start..start + s.len()] == s[..]
    }

    fn scan_char(&self, start: usize, end: usize, c: char) -> usize {
        if start < end && self.ch(start) == c as u16 { start + 1 } else { start }
    }

    /// `scan(start, end, err, stop)`: -1 at a char of `err`, else the first char of `stop` or the end.
    fn scan_err_stop(&self, start: usize, end: usize, err: &str, stop: &str) -> isize {
        let mut p = start;
        while p < end {
            let c = self.ch(p);
            if err.encode_utf16().any(|e| e == c) {
                return -1;
            }
            if stop.encode_utf16().any(|s| s == c) {
                break;
            }
            p += 1;
        }
        p as isize
    }

    fn scan_stop(&self, start: usize, end: usize, stop: &str) -> usize {
        let mut p = start;
        while p < end {
            let c = self.ch(p);
            if stop.encode_utf16().any(|s| s == c) {
                break;
            }
            p += 1;
        }
        p
    }

    fn scan_escape(&self, start: usize, n: usize, first: u16) -> P<usize> {
        let p = start;
        let c = first;
        if c == u16::from(b'%') {
            if p + 3 <= n && matches(self.ch(p + 1), L_HEX, H_HEX) && matches(self.ch(p + 2), L_HEX, H_HEX) {
                return Ok(p + 3);
            }
            return Err(Fail);
        } else if c > 128 && !is_space_char(c) && !is_iso_control(c) {
            return Ok(p + 1);
        }
        Ok(p)
    }

    fn scan_mask(&self, start: usize, n: usize, low: u64, high: u64) -> P<usize> {
        let mut p = start;
        while p < n {
            let c = self.ch(p);
            if matches(c, low, high) {
                p += 1;
                continue;
            }
            if low & L_ESCAPED != 0 {
                let q = self.scan_escape(p, n, c)?;
                if q > p {
                    p = q;
                    continue;
                }
            }
            break;
        }
        Ok(p)
    }

    fn check_chars(&self, start: usize, end: usize, low: u64, high: u64) -> P<()> {
        let p = self.scan_mask(start, end, low, high)?;
        if p < end { Err(Fail) } else { Ok(()) }
    }

    fn parse(&mut self) -> P<()> {
        let n = self.input.len();
        let mut p = self.scan_err_stop(0, n, "/?#", ":");
        if p >= 0 && self.at(p as usize, n, ':') {
            let pu = p as usize;
            if pu == 0 {
                return Err(Fail);
            }
            self.check_chars(0, 1, L_ALPHA, H_ALPHA)?;
            self.check_chars(1, pu, L_SCHEME, H_SCHEME)?;
            let mut q = pu + 1;
            if self.at(q, n, '/') {
                q = self.parse_hierarchical(q, n)?;
            } else {
                let r = self.scan_stop(q, n, "#");
                if r <= q {
                    return Err(Fail);
                }
                self.check_chars(q, r, L_URIC, H_URIC)?;
                q = r;
            }
            p = q as isize;
        } else {
            p = self.parse_hierarchical(0, n)? as isize;
        }
        let mut p = p as usize;
        if self.at(p, n, '#') {
            self.check_chars(p + 1, n, L_URIC, H_URIC)?;
            p = n;
        }
        if p < n { Err(Fail) } else { Ok(()) }
    }

    fn parse_hierarchical(&mut self, start: usize, n: usize) -> P<usize> {
        let mut p = start;
        if self.at(p, n, '/') && self.at(p + 1, n, '/') {
            p += 2;
            let q = self.scan_stop(p, n, "/?#");
            if q > p {
                p = self.parse_authority(p, q)?;
            } else if q < n {
                // an empty authority before a path, query or fragment
            } else {
                return Err(Fail);
            }
        }
        let q = self.scan_stop(p, n, "?#");
        self.check_chars(p, q, L_PATH, H_PATH)?;
        p = q;
        if self.at(p, n, '?') {
            p += 1;
            let q = self.scan_stop(p, n, "#");
            self.check_chars(p, q, L_URIC, H_URIC)?;
            p = q;
        }
        Ok(p)
    }

    fn parse_authority(&mut self, start: usize, n: usize) -> P<usize> {
        let p = start;
        let mut q = p;
        let server_chars = if self.scan_stop(p, n, "]") > p {
            self.scan_mask(p, n, L_SERVER_PERCENT, H_SERVER_PERCENT)? == n
        } else {
            self.scan_mask(p, n, L_SERVER, H_SERVER)? == n
        };
        let reg_chars = self.scan_mask(p, n, L_REG_NAME, H_REG_NAME)? == n;
        if reg_chars && !server_chars {
            return Ok(n);
        }
        // a server-based parse first (a failure kept, `q` reset to the start); the registry-based one is the fallback
        let skip_parse_exception = reg_chars;
        if server_chars {
            q = match self.parse_server(p, n, skip_parse_exception) {
                Ok(r) if r >= n => r,
                _ => p,
            };
        }
        if q < n && !reg_chars {
            return Err(Fail);
        }
        Ok(n)
    }

    fn parse_server(&mut self, start: usize, n: usize, skip_parse_exception: bool) -> P<usize> {
        let mut p = start;
        let q = self.scan_err_stop(p, n, "/?#", "@");
        if q >= p as isize && self.at(q as usize, n, '@') {
            self.check_chars(p, q as usize, L_USERINFO, H_USERINFO)?;
            p = q as usize + 1;
        }
        if self.at(p, n, '[') {
            p += 1;
            let q = self.scan_err_stop(p, n, "/?#", "]");
            if q > p as isize && self.at(q as usize, n, ']') {
                let q = q as usize;
                let r = self.scan_stop(p, q, "%");
                if r > p {
                    self.parse_ipv6_reference(p, r)?;
                    if r + 1 == q {
                        return Err(Fail);
                    }
                    self.check_chars(r + 1, q, L_SCOPE_ID, H_SCOPE_ID)?;
                } else {
                    self.parse_ipv6_reference(p, q)?;
                }
                p = q + 1;
            } else {
                return Err(Fail);
            }
        } else {
            let mut q = self.parse_ipv4_address(p, n);
            if q <= p as isize {
                q = self.parse_hostname(p, n, skip_parse_exception)? as isize;
            }
            p = q as usize;
        }
        if self.at(p, n, ':') {
            p += 1;
            let q = self.scan_stop(p, n, "/");
            if q > p {
                self.check_chars(p, q, L_DIGIT, H_DIGIT)?;
                if parse_int(&self.input[p..q]).is_none() {
                    return Err(Fail);
                }
                p = q;
            }
        } else if p < n && skip_parse_exception {
            return Ok(p);
        }
        if p < n {
            return Err(Fail);
        }
        Ok(p)
    }

    fn scan_byte(&self, start: usize, n: usize) -> P<usize> {
        let p = start;
        let q = self.scan_mask(p, n, L_DIGIT, H_DIGIT)?;
        if q <= p {
            return Ok(q);
        }
        let mut i = p;
        loop {
            let j = self.scan_char(i, q, '0');
            if j > i {
                i = j;
            } else {
                break;
            }
        }
        let significant = q - i;
        if significant < 3 {
            return Ok(q);
        }
        if significant > 3 {
            return Ok(p);
        }
        match parse_int(&self.input[p..q]) {
            Some(v) if v > 255 => Ok(p),
            Some(_) => Ok(q),
            None => Err(Fail),
        }
    }

    /// `scanIPv4Address(start, n, strict)`: -1 when the interval holds no IPv4 characters.
    fn scan_ipv4_address(&self, start: usize, n: usize, strict: bool) -> P<isize> {
        let mut p = start;
        let m = self.scan_mask(p, n, L_DIGIT | L_DOT, H_DIGIT | H_DOT)?;
        if m <= p || (strict && m != n) {
            return Ok(-1);
        }
        // byte '.' byte '.' byte '.' byte, the whole run
        'address: {
            for i in 0..7 {
                let q = if i % 2 == 0 { self.scan_byte(p, m)? } else { self.scan_char(p, m, '.') };
                if q <= p {
                    break 'address;
                }
                p = q;
            }
            if p < m {
                break 'address;
            }
            return Ok(p as isize);
        }
        if strict {
            return Err(Fail);
        }
        Ok(-1)
    }

    fn take_ipv4_address(&self, start: usize, n: usize) -> P<usize> {
        let p = self.scan_ipv4_address(start, n, true)?;
        if p <= start as isize {
            return Err(Fail);
        }
        Ok(p as usize)
    }

    /// `parseIPv4Address`: -1 on any failure.
    fn parse_ipv4_address(&self, start: usize, n: usize) -> isize {
        let p = match self.scan_ipv4_address(start, n, false) {
            Ok(p) => p,
            Err(Fail) => return -1,
        };
        if p == -1 {
            return p;
        }
        let pu = p as usize;
        if pu > start && pu < n && self.ch(pu) != u16::from(b':') {
            return -1;
        }
        p
    }

    fn parse_hostname(&self, start: usize, n: usize, skip_parse_exception: bool) -> P<usize> {
        let mut p = start;
        let mut l: Option<usize> = None;
        loop {
            let q = self.scan_mask(p, n, L_ALPHANUM, H_ALPHANUM)?;
            if q <= p {
                break;
            }
            l = Some(p);
            p = q;
            let q = self.scan_mask(p, n, L_ALPHANUM | L_DASH, H_ALPHANUM | H_DASH)?;
            if q > p {
                if self.ch(q - 1) == u16::from(b'-') {
                    return Err(Fail);
                }
                p = q;
            }
            let q = self.scan_char(p, n, '.');
            if q <= p {
                break;
            }
            p = q;
            if p >= n {
                break;
            }
        }
        if p < n && !self.at(p, n, ':') {
            if skip_parse_exception {
                return Ok(p);
            }
            return Err(Fail);
        }
        let Some(l) = l else { return Err(Fail) };
        if l > start && !matches(self.ch(l), L_ALPHA, H_ALPHA) {
            return Err(Fail);
        }
        Ok(p)
    }

    fn parse_ipv6_reference(&mut self, start: usize, n: usize) -> P<usize> {
        let mut p = start;
        let mut compressed_zeros = false;
        let q = self.scan_hex_seq(p, n)?;
        if q > p as isize {
            p = q as usize;
            if self.at_str(p, n, "::") {
                compressed_zeros = true;
                p = self.scan_hex_post(p + 2, n)?;
            } else if self.at(p, n, ':') {
                p = self.take_ipv4_address(p + 1, n)?;
                self.ipv6_byte_count += 4;
            }
        } else if self.at_str(p, n, "::") {
            compressed_zeros = true;
            p = self.scan_hex_post(p + 2, n)?;
        }
        if p < n
            || self.ipv6_byte_count > 16
            || (!compressed_zeros && self.ipv6_byte_count < 16)
            || (compressed_zeros && self.ipv6_byte_count == 16)
        {
            return Err(Fail);
        }
        Ok(p)
    }

    fn scan_hex_post(&mut self, start: usize, n: usize) -> P<usize> {
        let mut p = start;
        if p == n {
            return Ok(p);
        }
        let q = self.scan_hex_seq(p, n)?;
        if q > p as isize {
            p = q as usize;
            if self.at(p, n, ':') {
                p += 1;
                p = self.take_ipv4_address(p, n)?;
                self.ipv6_byte_count += 4;
            }
        } else {
            p = self.take_ipv4_address(p, n)?;
            self.ipv6_byte_count += 4;
        }
        Ok(p)
    }

    /// `scanHexSeq`: -1 when no hex sequence starts here.
    fn scan_hex_seq(&mut self, start: usize, n: usize) -> P<isize> {
        let mut p = start;
        let q = self.scan_mask(p, n, L_HEX, H_HEX)?;
        if q <= p {
            return Ok(-1);
        }
        if self.at(q, n, '.') {
            return Ok(-1);
        }
        if q > p + 4 {
            return Err(Fail);
        }
        self.ipv6_byte_count += 2;
        p = q;
        while p < n {
            if !self.at(p, n, ':') {
                break;
            }
            if self.at(p + 1, n, ':') {
                break;
            }
            p += 1;
            let q = self.scan_mask(p, n, L_HEX, H_HEX)?;
            if q <= p {
                return Err(Fail);
            }
            if self.at(q, n, '.') {
                p -= 1;
                break;
            }
            if q > p + 4 {
                return Err(Fail);
            }
            self.ipv6_byte_count += 2;
            p = q;
        }
        Ok(p as isize)
    }
}

/// `Integer.parseInt(s, 10)` over ASCII digits (the parser checks them first): `None` past an `int`.
fn parse_int(digits: &[u16]) -> Option<i32> {
    let mut v: i32 = 0;
    for &d in digits {
        v = v.checked_mul(10)?.checked_add(i32::from(d - u16::from(b'0')))?;
    }
    Some(v)
}

/// Whether `new java.net.URI(text)` parses (else it throws `URISyntaxException`).
pub fn uri_parses(text: &[u16]) -> bool {
    Parser { input: text, ipv6_byte_count: 0 }.parse().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ustr::u;

    #[test]
    fn the_jdk_s_uri_verdicts() {
        // each verdict JDK 25's `new URI(text)` gave (2026-11-05)
        for ok in [
            "http://dummyurl.com:1234/api",
            "http://localhost:9200",
            "",
            "#f",
            "http://[::1]:80/x",
            "http://a_b.c/",
            "http://ü.com/",
            "http://h:99999999999/",
            "http://1.2.3.4:5/",
            "http://1.2.3.400/",
            "http://-a.com/",
            "http://a@b@c/",
            "http://a.1b/",
            "http://h:/",
        ] {
            assert!(uri_parses(&u(ok)), "{ok} parses");
        }
        for bad in [
            "wrong url",
            "'http://x'",
            "http://_%_ELASTIC_HOST_%_",
            "http://",
            ":x",
            "a b",
            "x:",
            "http://[1::2::3]/",
            "%zz",
        ] {
            assert!(!uri_parses(&u(bad)), "{bad} is refused");
        }
    }
}
