// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The model's text, as Java's `String` is: UTF-16 units ([`JStr`], a lone surrogate kept). A message the model
//! builds from a name is made of the name's units, never of a lossy copy ([`cat!`](crate::cat)), and the few `String`
//! operations the model's code calls are here, over units.

use lite_front::jtypes::JStr;

/// Something whose UTF-16 units a message is built from: Rust text (encoded), a Java string, its units.
pub trait Units {
    fn push_to(&self, out: &mut Vec<u16>);
}

impl Units for str {
    fn push_to(&self, out: &mut Vec<u16>) {
        out.extend(self.encode_utf16());
    }
}

impl Units for String {
    fn push_to(&self, out: &mut Vec<u16>) {
        out.extend(self.encode_utf16());
    }
}

impl Units for JStr {
    fn push_to(&self, out: &mut Vec<u16>) {
        out.extend_from_slice(&self.0);
    }
}

impl Units for [u16] {
    fn push_to(&self, out: &mut Vec<u16>) {
        out.extend_from_slice(self);
    }
}

impl Units for Vec<u16> {
    fn push_to(&self, out: &mut Vec<u16>) {
        out.extend_from_slice(self);
    }
}

impl<T: Units + ?Sized> Units for &T {
    fn push_to(&self, out: &mut Vec<u16>) {
        (**self).push_to(out);
    }
}

/// Java's `a + b + ...` over strings: the parts' units in order, as a [`JStr`].
#[macro_export]
macro_rules! cat {
    ($($part:expr),* $(,)?) => {{
        let mut out: Vec<u16> = Vec::new();
        $( $crate::text::Units::push_to(&$part, &mut out); )*
        ::lite_front::jtypes::JStr(out)
    }};
}

/// `s.equals(lit)` for a literal.
pub fn is(s: &[u16], lit: &str) -> bool {
    s.iter().copied().eq(lit.encode_utf16())
}

/// `s.startsWith(prefix)`.
pub fn starts_with(s: &[u16], prefix: &str) -> bool {
    let p: Vec<u16> = prefix.encode_utf16().collect();
    s.starts_with(&p)
}

/// `s.endsWith(suffix)`.
pub fn ends_with(s: &[u16], suffix: &str) -> bool {
    let p: Vec<u16> = suffix.encode_utf16().collect();
    s.ends_with(&p)
}

/// `s.indexOf(needle)` (units), `None` for -1.
pub fn index_of(s: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    s.windows(needle.len()).position(|w| w == needle)
}

/// `s.lastIndexOf(needle)` (units), `None` for -1.
pub fn last_index_of(s: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.is_empty() {
        return Some(s.len());
    }
    if needle.len() > s.len() {
        return None;
    }
    (0..=s.len() - needle.len())
        .rev()
        .find(|&i| &s[i..i + needle.len()] == needle)
}

/// `s.contains(needle)`.
pub fn contains(s: &[u16], needle: &str) -> bool {
    let n: Vec<u16> = needle.encode_utf16().collect();
    index_of(s, &n).is_some()
}

/// `s.replace(from, to)` for literal targets: every non-overlapping occurrence, left to right.
pub fn replace(s: &[u16], from: &str, to: &str) -> Vec<u16> {
    let f: Vec<u16> = from.encode_utf16().collect();
    let t: Vec<u16> = to.encode_utf16().collect();
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if !f.is_empty() && s[i..].starts_with(&f) {
            out.extend_from_slice(&t);
            i += f.len();
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

/// `s.toUpperCase(Locale.ROOT)` where the answer is compared with ASCII words: the upper case of each unit that has an
/// ASCII one (`a`-`z`, and the few others Java's case mapping takes to ASCII: dotless `ı` to `I`, long `ſ` to `S`, `ß`
/// to `SS`, the Latin ligatures `ﬀ` to `ﬆ`); `None` when some unit's upper case is not ASCII, so the answer equals no
/// ASCII word.
pub fn ascii_upper(s: &[u16]) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    for &u in s {
        match u {
            0x61..=0x7a => out.push((u as u8 - 0x20) as char),
            0x20..=0x7e => out.push(u as u8 as char),
            0x0131 => out.push('I'),
            0x017f => out.push('S'),
            0x00df => out.push_str("SS"),
            0xfb00 => out.push_str("FF"),
            0xfb01 => out.push_str("FI"),
            0xfb02 => out.push_str("FL"),
            0xfb03 => out.push_str("FFI"),
            0xfb04 => out.push_str("FFL"),
            0xfb05 | 0xfb06 => out.push_str("ST"),
            _ => return None,
        }
    }
    Some(out)
}

/// A Java string of Rust text.
pub fn j(s: &str) -> JStr {
    JStr::from(s)
}
