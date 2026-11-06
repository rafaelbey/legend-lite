// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `String` operations over UTF-16 units, as the parser calls them (`indexOf`, `lastIndexOf`, `startsWith`,
//! `trim`, `strip`, `substring` with its bounds check), and [`jcat!`](crate::jcat), the message builder: Java's
//! `"a" + b + c` over parts that may hold lone surrogates.

use crate::doors::JavaError;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use lite_javacompat::character::is_whitespace;

/// A piece of a message: text, units, a number or a token type (its Java name, as `"" + type` writes it).
pub trait Part {
    fn put(&self, out: &mut Vec<u16>);
}

impl Part for str {
    fn put(&self, out: &mut Vec<u16>) {
        out.extend(self.encode_utf16());
    }
}

impl Part for String {
    fn put(&self, out: &mut Vec<u16>) {
        out.extend(self.encode_utf16());
    }
}

impl Part for [u16] {
    fn put(&self, out: &mut Vec<u16>) {
        out.extend_from_slice(self);
    }
}

impl Part for Vec<u16> {
    fn put(&self, out: &mut Vec<u16>) {
        out.extend_from_slice(self);
    }
}

impl Part for JStr {
    fn put(&self, out: &mut Vec<u16>) {
        out.extend_from_slice(&self.0);
    }
}

impl Part for TokenType {
    fn put(&self, out: &mut Vec<u16>) {
        out.extend(self.name().encode_utf16());
    }
}

macro_rules! number_part {
    ($($t:ty),*) => {
        $(impl Part for $t {
            fn put(&self, out: &mut Vec<u16>) {
                out.extend(self.to_string().encode_utf16());
            }
        })*
    };
}

number_part!(i32, i64, usize, isize, u32);

impl<T: Part + ?Sized> Part for &T {
    fn put(&self, out: &mut Vec<u16>) {
        (**self).put(out);
    }
}

/// Java's string concatenation: the parts, in order, as one string's units.
#[macro_export]
macro_rules! jcat {
    ($($p:expr),* $(,)?) => {{
        let mut out: Vec<u16> = Vec::new();
        $( $crate::ustr::Part::put(&$p, &mut out); )*
        $crate::jtypes::JStr(out)
    }};
}

/// The units of a Rust string.
pub fn u(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Whether `h` is exactly `s`.
pub fn eq(h: &[u16], s: &str) -> bool {
    h.iter().copied().eq(s.encode_utf16())
}

/// Java's `startsWith`.
pub fn starts_with(h: &[u16], p: &str) -> bool {
    let p: Vec<u16> = u(p);
    h.len() >= p.len() && h[..p.len()] == p[..]
}

/// Java's `endsWith`.
pub fn ends_with(h: &[u16], p: &str) -> bool {
    let p: Vec<u16> = u(p);
    h.len() >= p.len() && h[h.len() - p.len()..] == p[..]
}

/// Java's `indexOf(char, from)`.
pub fn index_of(h: &[u16], c: u16, from: usize) -> Option<usize> {
    (from..h.len()).find(|&i| h[i] == c)
}

/// Java's `indexOf(String, from)`: an empty needle is found at `from` (clamped to the length).
pub fn index_of_units(h: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    let from = from.min(h.len());
    if needle.is_empty() {
        return Some(from);
    }
    if needle.len() > h.len() {
        return None;
    }
    (from..=h.len() - needle.len()).find(|&i| h[i..i + needle.len()] == *needle)
}

/// Java's `lastIndexOf(char)`.
pub fn last_index_of(h: &[u16], c: u16) -> Option<usize> {
    h.iter().rposition(|&x| x == c)
}

/// Java's `lastIndexOf(String)`.
pub fn last_index_of_str(h: &[u16], needle: &str) -> Option<usize> {
    let n = u(needle);
    if n.len() > h.len() {
        return None;
    }
    (0..=h.len() - n.len()).rev().find(|&i| h[i..i + n.len()] == n[..])
}

/// Java's `String.trim`: every unit at most `' '` off both ends.
pub fn trim(h: &[u16]) -> &[u16] {
    let mut s = 0;
    let mut e = h.len();
    while s < e && h[s] <= 0x20 {
        s += 1;
    }
    while e > s && h[e - 1] <= 0x20 {
        e -= 1;
    }
    &h[s..e]
}

/// Java's `String.strip`: `Character.isWhitespace` off both ends (a surrogate is never whitespace, so units do).
pub fn strip(h: &[u16]) -> &[u16] {
    let mut s = 0;
    let mut e = h.len();
    while s < e && is_whitespace(h[s]) {
        s += 1;
    }
    while e > s && is_whitespace(h[e - 1]) {
        e -= 1;
    }
    &h[s..e]
}

/// Java's `substring(begin, end)`, its `StringIndexOutOfBoundsException` included.
pub fn substring(h: &[u16], begin: isize, end: isize) -> Result<&[u16], JavaError> {
    let len = h.len() as isize;
    if begin < 0 || begin > end || end > len {
        return Err(JavaError::java(
            "java.lang.StringIndexOutOfBoundsException",
            jcat!("begin ", begin, ", end ", end, ", length ", len),
        ));
    }
    Ok(&h[begin as usize..end as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_answer_as_java_strings_do() {
        assert_eq!(trim(&u(" \t a b \n")), &u("a b")[..]);
        assert_eq!(strip(&u("\u{2003}a\u{3000}")), &u("a")[..]);
        assert_eq!(index_of_units(&u("abc"), &[], 7), Some(3));
        assert_eq!(last_index_of_str(&u("a::b::c"), "::"), Some(4));
        assert!(substring(&u("abc"), 2, 1).is_err());
        assert_eq!(jcat!("x", 3, TokenType::EOF), JStr::from("x3EOF"));
    }
}
