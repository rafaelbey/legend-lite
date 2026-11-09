// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The questions of the front oracle where Rust answers differently from the frozen Java on purpose, and the raw JDK
//! answers kept on purpose (the user's ruling 4 of 2026-11-09, docs/rust-port/PHASE2.md section 6): the differentials
//! read both files. The frozen oracle stays Java's.
//!
//! `LITE_FRONT_DELIBERATE` names `rust/oracles/front/deliberate-differences.tsv`: per question, the PARITY row and the
//! new answer, its class, line, column and the SHA-256 of its message's UTF-16 units. Each differential holds that
//! `identical + listed == total`, that every listed question really differs from Java's frozen answer and that each
//! answers exactly its listed new answer. A question neither identical nor listed fails.
//!
//! `LITE_FRONT_RAW_LEFT` names `rust/oracles/front/raw-left-for-user.tsv`: the questions still answering a raw JDK class
//! because legend-engine accepts the input (the ruling's carve-out: such an input comes back to the user). Any other
//! `java.*` answer over the frozen questions fails, and so does a line of the file whose question no longer answers it.

use lite_front::sha256::sha256_utf16le;
use std::collections::{BTreeMap, BTreeSet};

/// A listed question's new answer.
pub struct Expected {
    pub class: String,
    pub line: String,
    pub column: String,
    pub message_sha: String,
}

/// A question's key: its door, its dialect (`-` at the engine-JSON doors) and its text's id.
type Key = (String, String, String);

pub struct Deliberate {
    expected: BTreeMap<Key, Expected>,
    left: BTreeMap<Key, String>,
    left_seen: BTreeSet<Key>,
}

fn key(door: &str, dialect: &str, sha: &str) -> Key {
    (door.to_owned(), dialect.to_owned(), sha.to_owned())
}

fn rows(variable: &str) -> Vec<Vec<String>> {
    let path = std::env::var(variable).unwrap_or_else(|_| panic!("{variable} names the file to read"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split('\t').map(str::to_owned).collect())
        .collect()
}

impl Deliberate {
    pub fn load() -> Deliberate {
        let mut expected = BTreeMap::new();
        for r in rows("LITE_FRONT_DELIBERATE") {
            assert_eq!(r.len(), 8, "door, dialect, sha, PARITY row, class, line, column, message sha: {r:?}");
            let e = Expected {
                class: r[4].clone(),
                line: r[5].clone(),
                column: r[6].clone(),
                message_sha: r[7].clone(),
            };
            assert!(expected.insert(key(&r[0], &r[1], &r[2]), e).is_none(), "listed twice: {r:?}");
        }
        let mut left = BTreeMap::new();
        for r in rows("LITE_FRONT_RAW_LEFT") {
            assert_eq!(r.len(), 6, "door, dialect, sha, PARITY row, class, what: {r:?}");
            assert!(left.insert(key(&r[0], &r[1], &r[2]), r[4].clone()).is_none(), "listed twice: {r:?}");
        }
        Deliberate { expected, left, left_seen: BTreeSet::new() }
    }

    /// How many questions the list names at `door`.
    pub fn listed(&self, door: &str) -> usize {
        self.expected.keys().filter(|k| k.0 == door).count()
    }

    pub fn expected(&self, door: &str, dialect: &str, sha: &str) -> Option<&Expected> {
        self.expected.get(&key(door, dialect, sha))
    }

    /// A raw JDK answer's verdict: `Err` unless the left-for-the-user list names exactly this question and class.
    pub fn raw_answer(&mut self, door: &str, dialect: &str, sha: &str, form: &[u16]) -> Result<(), String> {
        let Some(class) = raw_class(form) else {
            return Ok(());
        };
        let k = key(door, dialect, sha);
        if self.left.get(&k) == Some(&class) {
            self.left_seen.insert(k);
            return Ok(());
        }
        Err(format!("{door} {dialect} {sha}: a raw {class}"))
    }

    /// The left-for-the-user lines of `door` whose question no longer answers the raw class they name.
    pub fn stale_left(&self, door: &str) -> Vec<String> {
        self.left.keys().filter(|k| k.0 == door && !self.left_seen.contains(*k)).map(|k| format!("{k:?}")).collect()
    }
}

/// The `java.*` class of an error form, if it is one.
fn raw_class(form: &[u16]) -> Option<String> {
    let text = String::from_utf16_lossy(&form[..form.len().min(200)]);
    let rest = text.strip_prefix("E\t")?;
    let class = rest.split('\t').next()?;
    class.starts_with("java.").then(|| class.to_owned())
}

/// Whether `form` (`E\t<class>\t<line>\t<column>\t<message>`) is exactly this new answer.
pub fn is_the_new_answer(form: &[u16], e: &Expected) -> Result<(), String> {
    let tab = u16::from(b'\t');
    let f: Vec<&[u16]> = form.splitn(5, |&c| c == tab).collect();
    let field = |i: usize| f.get(i).map_or(String::new(), |u| String::from_utf16_lossy(u));
    let message_sha = f.get(4).map_or(String::new(), |m| sha256_utf16le(m));
    if field(0) == "E" && field(1) == e.class && field(2) == e.line && field(3) == e.column && message_sha == e.message_sha {
        return Ok(());
    }
    Err(format!(
        "answers {} {}:{} message sha {message_sha} ({}), the list says {} {}:{} {}",
        field(1),
        field(2),
        field(3),
        field(4),
        e.class,
        e.line,
        e.column,
        e.message_sha
    ))
}
