// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `RawSqlBoundary.translated`: one statement of the engine's H2 text in the flavour DuckDB and Postgres share,
//! and the column-type rewrites Postgres's `fromEngineText` applies over it ([`double_precision`], [`replace_word`]).
//! Its regular expressions are hand-written matchers here, each Java's pattern exactly as `java.util.regex` reads it
//! on the JDK 25 the corpora ran on: `(?i)` folds ASCII only, `\w` is `[a-zA-Z_0-9]`, `\s` is `[ \t\n\x0B\f\r]`,
//! and `\b` is the boundary between `\w` and the rest (ASCII since JDK 19).

use crate::java::{is_java_whitespace, split, strip};

/// The declared database whose text is legend-engine's H2: the one language raw text is translated from.
pub const ENGINE_TEXT: &str = "H2";

/// The H2 dialect level this boundary translates from (`SELECT H2VERSION()` answers it).
pub const H2_DIALECT_VERSION: &str = "2.1.214";

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// A word boundary at `i` of `s`.
fn boundary(s: &[u8], i: usize) -> bool {
    let before = i > 0 && is_word(s[i - 1]);
    let after = i < s.len() && is_word(s[i]);
    before != after
}

/// `word` (ASCII, matched case-insensitively) at `i`; the index after it.
fn keyword(s: &[u8], i: usize, word: &str) -> Option<usize> {
    let w = word.as_bytes();
    (s.len() >= i + w.len() && s[i..i + w.len()].eq_ignore_ascii_case(w)).then_some(i + w.len())
}

fn spaces(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    i
}

/// `\s+`: at least one.
fn spaces1(s: &[u8], i: usize) -> Option<usize> {
    let j = spaces(s, i);
    (j > i).then_some(j)
}

/// `\w+`.
fn word1(s: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    while j < s.len() && is_word(s[j]) {
        j += 1;
    }
    (j > i).then_some(j)
}

/// `[\w."]+`.
fn name1(s: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    while j < s.len() && (is_word(s[j]) || s[j] == b'.' || s[j] == b'"') {
        j += 1;
    }
    (j > i).then_some(j)
}

/// Java's `replaceAll` of a pattern `at` matches at an index (its end, and the replacement for the match), each
/// match searched from the end of the previous one.
fn replace_all(s: &str, at: impl Fn(&[u8], usize) -> Option<(usize, String)>) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut copied = 0;
    let mut i = 0;
    while i < b.len() {
        if let Some((end, replacement)) = at(b, i) {
            out.push_str(&s[copied..i]);
            out.push_str(&replacement);
            copied = end;
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    out.push_str(&s[copied..]);
    out
}

/// `(?i)\bCURRENT_TIMESTAMP\(\)`.
fn current_ts(s: &[u8], i: usize) -> Option<(usize, String)> {
    if !boundary(s, i) {
        return None;
    }
    let j = keyword(s, i, "CURRENT_TIMESTAMP()")?;
    Some((j, "CURRENT_TIMESTAMP".to_owned()))
}

/// `(?i)\bH2VERSION\s*\(\s*\)`.
fn h2_version(s: &[u8], i: usize) -> Option<(usize, String)> {
    if !boundary(s, i) {
        return None;
    }
    let j = keyword(s, i, "H2VERSION")?;
    let j = keyword(s, spaces(s, j), "(")?;
    let j = keyword(s, spaces(s, j), ")")?;
    Some((j, format!("'{H2_DIALECT_VERSION}'")))
}

/// `(?i)\bcount\(\s*\*\s*\)(?=\s*(,|from\b))`.
fn count_star_item(s: &[u8], i: usize) -> Option<(usize, String)> {
    if !boundary(s, i) {
        return None;
    }
    let j = keyword(s, i, "count(")?;
    let j = keyword(s, spaces(s, j), "*")?;
    let j = keyword(s, spaces(s, j), ")")?;
    let k = spaces(s, j);
    let ahead = keyword(s, k, ",").is_some() || keyword(s, k, "from").is_some_and(|e| boundary(s, e));
    ahead.then(|| (j, "count(*) AS \"COUNT(*)\"".to_owned()))
}

/// `(?i)\bdrop\s+schema\s+(\w+)\s+if\s+exists\b` to `Drop schema if exists $1`.
fn drop_schema(s: &[u8], i: usize) -> Option<(usize, String)> {
    if !boundary(s, i) {
        return None;
    }
    let j = spaces1(s, keyword(s, i, "drop")?)?;
    let j = spaces1(s, keyword(s, j, "schema")?)?;
    let name_end = word1(s, j)?;
    let k = spaces1(s, name_end)?;
    let k = spaces1(s, keyword(s, k, "if")?)?;
    let k = keyword(s, k, "exists")?;
    if !boundary(s, k) {
        return None;
    }
    Some((k, format!("Drop schema if exists {}", String::from_utf8_lossy(&s[j..name_end]))))
}

/// `(?i)\bcreate\s+schema\s+(?!if\b)(\w+)` to `Create schema if not exists $1`.
fn create_schema(s: &[u8], i: usize) -> Option<(usize, String)> {
    if !boundary(s, i) {
        return None;
    }
    let j = spaces1(s, keyword(s, i, "create")?)?;
    let j = spaces1(s, keyword(s, j, "schema")?)?;
    // the lookahead refuses `if` followed by a boundary; backtracking into the spaces cannot reach a word
    if keyword(s, j, "if").is_some_and(|e| boundary(s, e)) {
        return None;
    }
    let end = word1(s, j)?;
    Some((end, format!("Create schema if not exists {}", String::from_utf8_lossy(&s[j..end]))))
}

/// `(?i)^\s*create\s+table\s+[\w."]+\s*\(`, found at the start: the index after its `(`.
fn create_head(s: &[u8]) -> Option<usize> {
    let j = spaces1(s, keyword(s, spaces(s, 0), "create")?)?;
    let j = spaces1(s, keyword(s, j, "table")?)?;
    keyword(s, spaces(s, name1(s, j)?), "(")
}

/// `(?i)^(\s*insert\s+into\s+[\w."]+\s*\()([^)]*)(\))`: the end of group 1 and the index of its `)`.
fn insert_cols(s: &[u8]) -> Option<(usize, usize)> {
    let j = spaces1(s, keyword(s, spaces(s, 0), "insert")?)?;
    let j = spaces1(s, keyword(s, j, "into")?)?;
    let open = keyword(s, spaces(s, name1(s, j)?), "(")?;
    let close = open + s[open..].iter().position(|b| *b == b')')?;
    Some((open, close))
}

/// `(?i)primary|constraint|foreign|unique|check`, matched whole.
fn non_column_head(head: &str) -> bool {
    ["primary", "constraint", "foreign", "unique", "check"].iter().any(|w| head.eq_ignore_ascii_case(w))
}

/// `(?i)\b<word>\b` to `replacement` (`word` ASCII).
pub fn replace_word(s: &str, word: &str, replacement: &str) -> String {
    replace_all(s, |b, i| {
        if !boundary(b, i) {
            return None;
        }
        let j = keyword(b, i, word)?;
        boundary(b, j).then(|| (j, replacement.to_owned()))
    })
}

/// `(?i)\bDOUBLE\b(?!\s+PRECISION)` to `DOUBLE PRECISION` (Postgres's `ENGINE_DOUBLE`): the engine's 8-byte double as
/// Postgres spells it, a `DOUBLE PRECISION` already spelled left as it is.
pub fn double_precision(s: &str) -> String {
    replace_all(s, |b, i| {
        if !boundary(b, i) {
            return None;
        }
        let j = keyword(b, i, "DOUBLE")?;
        if !boundary(b, j) {
            return None;
        }
        // the lookahead: one or more spaces, then PRECISION (backtracking into the spaces reaches no other match)
        if let Some(k) = spaces1(b, j)
            && keyword(b, k, "PRECISION").is_some()
        {
            return None;
        }
        Some((j, "DOUBLE PRECISION".to_owned()))
    })
}

/// One statement of the engine's H2 text translated: `column_types` spells a column definition's type part, after
/// FLOAT, BIT and CLOB are read as H2 means them; `bare_name` is a bare column name as the target folds it.
pub fn translated(sql: &str, column_types: &dyn Fn(String) -> String, bare_name: &dyn Fn(&str) -> String) -> String {
    let out = replace_all(sql, current_ts);
    let out = replace_all(&out, h2_version);
    let out = replace_all(&out, count_star_item);
    let out = replace_all(&out, drop_schema);
    let out = replace_all(&out, create_schema);
    if let Some(body_start) = create_head(out.as_bytes()) {
        return quote_create_columns(&out, body_start, column_types, bare_name);
    }
    let Some((open, close)) = insert_cols(out.as_bytes()) else {
        return out;
    };
    let mut cols = String::new();
    for c in split(&out[open..close], ',') {
        if !cols.is_empty() {
            cols.push_str(", ");
        }
        let name = strip(c);
        if name.starts_with('"') {
            cols.push_str(name);
        } else {
            cols.push('"');
            cols.push_str(&bare_name(name));
            cols.push('"');
        }
    }
    format!("{}{cols}{}", &out[..open], &out[close..])
}

/// Quote the column name of each top-level column definition in a CREATE TABLE literal.
fn quote_create_columns(
    sql: &str,
    body_start: usize,
    column_types: &dyn Fn(String) -> String,
    bare_name: &dyn Fn(&str) -> String,
) -> String {
    // the body's end: the `)` closing the head's `(`, or the last character when none does (Java's `end - 1`)
    let mut depth = 1;
    let mut close = None;
    let mut last = body_start;
    for (i, c) in sql[body_start..].char_indices() {
        last = body_start + i;
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            close = Some(body_start + i);
            break;
        }
    }
    let close = close.unwrap_or(if body_start < sql.len() { last } else { body_start });
    let body = &sql[body_start..close];
    let mut parts = Vec::new();
    let mut d = 0;
    let mut start = 0;
    for (i, c) in body.char_indices() {
        match c {
            '(' => d += 1,
            ')' => d -= 1,
            ',' if d == 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    let mut out = String::new();
    for part in parts {
        let col = strip(part);
        if !out.is_empty() {
            out.push_str(", ");
        }
        let sp = col.find(|c: char| is_java_whitespace(c) || c == '(').unwrap_or(col.len());
        let head = &col[..sp];
        if let Some(quoted) = col.strip_prefix('"') {
            // a pre-quoted name: the type part still needs the kind mapping (Java's indexOf('"', 1) + 1, 0 for none)
            let endq = quoted.find('"').map_or(0, |i| i + 2);
            out.push_str(&col[..endq]);
            out.push_str(&map_column_types(&col[endq..], column_types));
        } else if non_column_head(head) {
            out.push_str(col);
        } else {
            out.push('"');
            out.push_str(&bare_name(head));
            out.push('"');
            out.push_str(&map_column_types(&col[sp..], column_types));
        }
    }
    format!("{}{out}{}", &sql[..body_start], &sql[close..])
}

/// H2 semantics on the type part only: FLOAT is an 8-byte double, BIT a boolean, CLOB text.
fn map_column_types(type_part: &str, column_types: &dyn Fn(String) -> String) -> String {
    let t = replace_word(type_part, "FLOAT", "DOUBLE");
    let t = replace_word(&t, "BIT", "BOOLEAN");
    let t = replace_word(&t, "CLOB", "TEXT");
    column_types(t)
}
