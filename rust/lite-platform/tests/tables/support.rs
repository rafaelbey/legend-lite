// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! What every section of the tables' dump shares (Java's `BuiltinAnswers.Out`): a line is its section and its fields,
//! tab separated; a record the lossless codec's JSON; a large text its SHA-256 (of its UTF-16 units, little-endian).

#![allow(dead_code)]

use lite_front::codec::{self, Codec};
use lite_front::jtypes::JStr;

#[derive(Default)]
pub struct Out {
    pub lines: Vec<String>,
}

impl Out {
    /// One line: the section, then the fields (none holds a tab or a line break).
    pub fn line(&mut self, section: &str, fields: &[String]) {
        let mut b = section.to_owned();
        for f in fields {
            assert!(
                !f.contains('\t') && !f.contains('\n'),
                "a field with a tab or a line break: {f}"
            );
            b.push('\t');
            b.push_str(f);
        }
        self.lines.push(b);
    }
}

/// A record in the codec: `exact` when the declared type is the record's own class (no tag).
pub fn json<T: Codec + ?Sized>(value: &T, exact: bool) -> String {
    codec::write(value, exact)
}

/// A text as the codec writes a string.
pub fn str(s: &JStr) -> String {
    let mut w = codec::Writer::new();
    w.string(s);
    w.finish()
}

/// The SHA-256 of a text's UTF-16 units, little-endian, in lower-case hex.
pub fn sha(units: &[u16]) -> String {
    lite_front::sha256::sha256_utf16le(units)
}

/// Values joined by one space, in order.
pub fn join<T: ToString>(values: impl IntoIterator<Item = T>) -> String {
    values.into_iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ")
}

/// Values joined by one space, sorted (Java's `String` order, which is UTF-16 order; these are ASCII).
pub fn sorted<T: ToString>(values: impl IntoIterator<Item = T>) -> String {
    let mut v: Vec<String> = values.into_iter().map(|v| v.to_string()).collect();
    v.sort();
    v.join(" ")
}

/// Java's `String.valueOf(boolean)`.
pub fn b(v: bool) -> String {
    v.to_string()
}

/// Compares a section's lines with Java's, by section: the counts, and the first differences.
pub fn compare(rust: &[String], java: &[String], what: &str) {
    let mut differ = Vec::new();
    let n = rust.len().max(java.len());
    for i in 0..n {
        let r = rust.get(i).map(String::as_str).unwrap_or("<none>");
        let j = java.get(i).map(String::as_str).unwrap_or("<none>");
        if r != j {
            differ.push(format!("line {}:\n  java: {}\n  rust: {}", i + 1, clip(j), clip(r)));
        }
    }
    let mut sections: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for l in java {
        *sections.entry(l.split('\t').next().unwrap_or("")).or_default() += 1;
    }
    println!(
        "[{what}] {} lines from Java, {} from Rust, {} differ",
        java.len(),
        rust.len(),
        differ.len()
    );
    for (s, c) in &sections {
        println!("[{what}]   {s}: {c}");
    }
    for d in differ.iter().take(20) {
        println!("[{what}] {d}");
    }
    assert!(differ.is_empty(), "{} of {} lines differ from Java's", differ.len(), n);
}

fn clip(s: &str) -> String {
    if s.len() > 400 {
        format!("{}...", &s[..s.char_indices().nth(400).map_or(s.len(), |(i, _)| i)])
    } else {
        s.to_owned()
    }
}

/// The frozen file's lines: `env` names it under Bazel, else the repository's copy beside this crate.
pub fn java_lines(env: &str, default: &str) -> Vec<String> {
    let path = std::env::var(env).unwrap_or_else(|_| format!("{}/{default}", env!("CARGO_MANIFEST_DIR")));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines().map(str::to_owned).collect()
}
