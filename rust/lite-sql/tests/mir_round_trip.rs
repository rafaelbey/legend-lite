// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The golden round trip: every statement the JVM's dialects rendered, as the MirJson Java wrote for it, read into the
//! Rust MIR and written back, byte for byte. The dump (`LITE_MIR_DUMP`) is JSON lines of
//! `{dialect, fresh, op, kind, mir, answer}`; `op`'s prefix (`query.`, `ddl.`, `dml.`) names the reader.

use lite_json::{Node, parse_with_depth, to_compact};
use lite_sql::{MAX_DEPTH, read_ddl, read_dml, read_query, write_ddl, write_dml, write_query};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};

const SHOWN: usize = 20;

#[test]
#[ignore = "reads the JVM's MirJson dump named by LITE_MIR_DUMP"]
fn every_dumped_statement_round_trips() {
    let path = std::env::var("LITE_MIR_DUMP")
        .expect("LITE_MIR_DUMP is unset: it names the JVM's MirJson dump (JSON lines) this test reads");
    // the reader and the writer recurse with the tree, which nests thousands deep
    let run = std::thread::Builder::new().stack_size(256 << 20).spawn(move || round_trip_all(&path)).unwrap();
    let Tally { lines, ops, tags, failures } = run.join().unwrap();
    assert!(lines > 0, "the dump has no lines");
    eprintln!("{lines} statements read; by op: {ops:?}");
    eprintln!("{} tags seen, with their counts: {tags:?}", tags.len());
    assert!(
        failures.is_empty(),
        "{} of {lines} statements did not round-trip; the first {}:\n{}",
        failures.len(),
        failures.len().min(SHOWN),
        failures[..failures.len().min(SHOWN)].join("\n")
    );
}

#[derive(Default)]
struct Tally {
    lines: usize,
    /// statements by op
    ops: BTreeMap<String, usize>,
    /// every `_type` in the trees, with its count
    tags: BTreeMap<String, usize>,
    failures: Vec<String>,
}

fn round_trip_all(path: &str) -> Tally {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("cannot open {path}: {e}"));
    let mut tally = Tally::default();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line.unwrap_or_else(|e| panic!("{path}:{}: {e}", i + 1));
        if line.is_empty() {
            continue;
        }
        tally.lines += 1;
        if let Err(why) = round_trip(&line, &mut tally) {
            tally.failures.push(format!("line {}: {why}", i + 1));
        }
    }
    tally
}

fn count_tags(n: &Node, into: &mut BTreeMap<String, usize>) {
    match n {
        Node::Obj(o) => {
            for (k, v) in o.iter() {
                if let ("_type", Node::Str(t)) = (k, v) {
                    *into.entry(t.clone()).or_default() += 1;
                }
                count_tags(v, into);
            }
        }
        Node::Arr(items) => items.iter().for_each(|v| count_tags(v, into)),
        _ => {}
    }
}

fn round_trip(line: &str, tally: &mut Tally) -> Result<(), String> {
    let doc = parse_with_depth(line, MAX_DEPTH).map_err(|e| format!("unparsable: {e}"))?;
    let doc = doc.as_obj().map_err(|e| e.to_string())?;
    let field = |k: &str| doc.str(k).map(str::to_owned).unwrap_or_else(|_| "?".to_owned());
    let (dialect, op, kind) = (field("dialect"), field("op"), field("kind"));
    let at = format!("{dialect} {op} {kind}");
    let mir = doc.get("mir").ok_or_else(|| format!("{at}: no mir"))?;
    *tally.ops.entry(op.clone()).or_default() += 1;
    count_tags(mir, &mut tally.tags);
    let obj = mir.as_obj().map_err(|e| format!("{at}: {e}"))?;
    let back = match op.split('.').next() {
        Some("query") => read_query(obj).map(|q| write_query(&q)),
        Some("ddl") => read_ddl(obj).map(|d| write_ddl(&d)),
        Some("dml") => read_dml(obj).map(|d| write_dml(&d)),
        _ => return Err(format!("{at}: unknown op")),
    }
    .map_err(|e| format!("{at}: refused: {e}"))?;
    // Java's own bytes: the MirJson text as the probe wrote it, between `"mir":` and `,"answer":` -- so the round
    // trip is held to what Java wrote, not to lite-json's reading of it
    let raw = line
        .find(",\"mir\":")
        .and_then(|start| line.rfind(",\"answer\":").map(|end| &line[start + 7..end]))
        .ok_or_else(|| format!("{at}: no MirJson text in the line"))?;
    let reread = to_compact(mir);
    if reread != raw {
        return Err(format!("{at}: lite-json does not write Java's text back: {}", difference(raw, &reread)));
    }
    let got = to_compact(&back);
    if raw == got { Ok(()) } else { Err(format!("{at}: {}", difference(raw, &got))) }
}

/// Where two texts first differ, with the text around it in each.
fn difference(want: &str, got: &str) -> String {
    let at = want.bytes().zip(got.bytes()).position(|(a, b)| a != b).unwrap_or(want.len().min(got.len()));
    format!(
        "differs at byte {at} (lengths {} and {})\n  java: ...{}...\n  rust: ...{}...",
        want.len(),
        got.len(),
        around(want, at),
        around(got, at)
    )
}

fn around(s: &str, at: usize) -> &str {
    let mut from = at.saturating_sub(80);
    while !s.is_char_boundary(from) {
        from -= 1;
    }
    let mut to = (at + 80).min(s.len());
    while !s.is_char_boundary(to) {
        to += 1;
    }
    &s[from..to]
}
