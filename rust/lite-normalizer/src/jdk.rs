// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The two JDK shapes the normalizer needs that no crate below it has (the skeleton, 2026-11-14):
//!
//! - [`LinkedSet`]: a `LinkedHashSet` -- insertion order, `add` answering whether it added, `remove` freeing the slot
//!   (a later `add` appends). E prints such sets' order into answers: the `extends` chain and the `~src` chain of
//!   MappingPrePass' cycle texts, `Pipeline.inlineStack` in a splice-cycle text, the database names a mapping reads.
//!   (An insertion-ordered map is lite-model's `LinkedMap`.)
//! - [`equals_ignore_case`]: `String.equalsIgnoreCase`, the JDK's fold: per character, equal, or equal after
//!   `Character.toUpperCase`, or equal after `toLowerCase(toUpperCase(..))`, with each code point's SIMPLE (one to
//!   one) case mapping. Rust's `char` mappings are the full ones (`'ß'` upper-cases to `"SS"`), so a mapping that is
//!   not one character keeps the character, which is Java's simple mapping for every such character but one: U+0130
//!   (`'İ'`), whose simple lower case is `'i'` and which is spelled here. The bound: the Unicode version of Rust's
//!   tables and the JDK's may differ for characters added since; the texts E folds (database and view column names,
//!   main tables) are identifiers.

use std::collections::HashMap;
use std::hash::Hash;

/// A `LinkedHashSet`: insertion-ordered, each element once.
#[derive(Clone, Debug)]
pub struct LinkedSet<T: Eq + Hash + Clone> {
    order: Vec<T>,
    index: HashMap<T, usize>,
}

impl<T: Eq + Hash + Clone> Default for LinkedSet<T> {
    fn default() -> Self {
        LinkedSet {
            order: Vec::new(),
            index: HashMap::new(),
        }
    }
}

impl<T: Eq + Hash + Clone> LinkedSet<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `add`: whether `x` was absent (and is now last).
    pub fn add(&mut self, x: T) -> bool {
        if self.index.contains_key(&x) {
            return false;
        }
        self.index.insert(x.clone(), self.order.len());
        self.order.push(x);
        true
    }

    /// `remove`: whether `x` was present.
    pub fn remove(&mut self, x: &T) -> bool {
        let Some(i) = self.index.remove(x) else {
            return false;
        };
        self.order.remove(i);
        for slot in self.index.values_mut() {
            if *slot > i {
                *slot -= 1;
            }
        }
        true
    }

    pub fn contains(&self, x: &T) -> bool {
        self.index.contains_key(x)
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The elements in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.order.iter()
    }

    /// The elements in insertion order, as a list.
    pub fn to_vec(&self) -> Vec<T> {
        self.order.clone()
    }
}

impl<T: Eq + Hash + Clone> FromIterator<T> for LinkedSet<T> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        let mut s = LinkedSet::new();
        for x in items {
            s.add(x);
        }
        s
    }
}

/// `a.equalsIgnoreCase(b)` over UTF-16 units (see the module's note on the fold and its bound).
pub fn equals_ignore_case(a: &[u16], b: &[u16]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut xs = char::decode_utf16(a.iter().copied());
    let mut ys = char::decode_utf16(b.iter().copied());
    loop {
        match (xs.next(), ys.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) => {
                let same = match (x, y) {
                    (Ok(x), Ok(y)) => {
                        x == y || {
                            let (ux, uy) = (simple_upper(x), simple_upper(y));
                            ux == uy || simple_lower(ux) == simple_lower(uy)
                        }
                    }
                    // a lone surrogate folds to itself
                    (Err(x), Err(y)) => x.unpaired_surrogate() == y.unpaired_surrogate(),
                    _ => false,
                };
                if !same {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// `Character.toUpperCase(cp)`: the simple mapping (a full mapping of several characters keeps the character).
fn simple_upper(c: char) -> char {
    let mut up = c.to_uppercase();
    match (up.next(), up.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// `Character.toLowerCase(cp)`: the simple mapping (U+0130's is `'i'`; another full mapping of several characters
/// keeps the character).
fn simple_lower(c: char) -> char {
    if c == '\u{130}' {
        return 'i';
    }
    let mut low = c.to_lowercase();
    match (low.next(), low.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn a_linked_set_keeps_insertion_order_and_frees_a_removed_slot() {
        let mut s = LinkedSet::new();
        assert!(s.add("b"));
        assert!(s.add("a"));
        assert!(!s.add("b"));
        assert_eq!(s.to_vec(), vec!["b", "a"]);
        assert!(s.remove(&"b"));
        assert!(!s.remove(&"b"));
        assert!(s.add("b"));
        assert_eq!(s.to_vec(), vec!["a", "b"]);
        assert!(s.contains(&"a") && s.len() == 2);
    }

    #[test]
    fn equals_ignore_case_folds_as_the_jdk() {
        assert!(equals_ignore_case(&u("FIRM_ID"), &u("firm_id")));
        assert!(!equals_ignore_case(&u("FIRM_ID"), &u("firm_i")));
        assert!(!equals_ignore_case(&u("A_B"), &u("A-B")));
        // the simple mappings: 'ß' has no one-character upper case, so it equals only itself
        assert!(equals_ignore_case(&u("straße"), &u("STRAßE")));
        assert!(!equals_ignore_case(&u("strasse"), &u("STRAßE")));
        // U+0130 lower-cases to 'i' (Java: toLowerCase(toUpperCase('i')) == toLowerCase('İ'))
        assert!(equals_ignore_case(&u("İ"), &u("i")));
        // the Kelvin sign upper-cases to itself and lower-cases to 'k'
        assert!(equals_ignore_case(&u("\u{212A}"), &u("k")));
        assert!(equals_ignore_case(&u(""), &u("")));
    }
}
