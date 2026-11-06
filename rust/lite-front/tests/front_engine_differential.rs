// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! lite-front's engine-JSON doors held to Java's (docs/rust-port/PHASE2.md §4 step 5): every question of the front
//! oracle (`LITE_FRONT_QUESTIONS`) at the two doors whose answer is legend-engine's JSON -- `document`, a model text
//! at the engine's dialect (`PmcdParser.parseDocument`), and `lambdaJson`, a lambda (`SpecParser.lambdaJson`) --
//! answered by Rust (`lite_front::doors::engine_forms`) in both forms, with its source information and without it,
//! each form's SHA-256 compared with the one Java gave the same question (`LITE_FRONT_ENGINE`,
//! //core:front_engine_answers: a cached action that runs Java's parser and emitter, live while they exist).
//!
//! A question is identical when both its forms are. One test per door asserts at least its floor
//! (`tests/<door>-floor.txt` among `LITE_FRONT_FLOORS`) of its questions identical; a floor only rises. Each prints its
//! counts by kind and form and its first mismatches; with `LITE_FRONT_MISMATCHES` naming a directory, each mismatch's
//! Rust forms are written there as `<door>.<sha>.rust.spans.txt` and `<door>.<sha>.rust.bare.txt`, and
//! `bazel run //core:front_show -- QUESTIONS DIR pmcd:LEGEND_ENGINE:<sha>...` (`lambda:LEGEND_ENGINE:<sha>` for a
//! lambda) writes Java's beside them as `<door>.<sha>.java.spans.txt` and `.java.bare.txt`. Java's hashes for another
//! question set: `bazel run //core:front_show -- --engine QUESTIONS OUT`.
//!
//! A form is hashed as its UTF-16 units, little-endian, each as it is (`sha256_utf16le`), as Java's
//! `FrontEngineAnswers.hash` hashes them: a lone surrogate the emitter writes raw hashes as itself.

use lite_front::doors::engine_forms;
use lite_front::sha256::sha256_utf16le;
use lite_front::wire;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};

const SHOWN: usize = 25;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} names the file to read"))
}

fn floor(door: &str) -> usize {
    let wanted = format!("{door}-floor.txt");
    let path = env("LITE_FRONT_FLOORS")
        .split_whitespace()
        .find(|p| p.ends_with(&wanted))
        .unwrap_or_else(|| panic!("no {wanted} among LITE_FRONT_FLOORS"))
        .to_owned();
    std::fs::read_to_string(&path).unwrap().trim().parse().unwrap()
}

/// The oracle's door for an engine door: the model door at the engine's dialect, or the lambda door.
fn asks(door: &str, pair: &[wire::Json]) -> bool {
    let (oracle_door, dialect) = (pair[0].as_string().unwrap(), pair[1].as_string().unwrap());
    match door {
        "document" => oracle_door == "pmcd" && dialect == "LEGEND_ENGINE",
        "lambdaJson" => oracle_door == "lambda",
        _ => panic!("no engine door {door}"),
    }
}

fn differential(door: &str) {
    // (spans form's sha, no-spans form's sha, A or E), by question sha
    let mut java: HashMap<String, (String, String, String)> = HashMap::new();
    for line in BufReader::new(File::open(env("LITE_FRONT_ENGINE")).unwrap()).lines() {
        let line = line.unwrap();
        let f: Vec<&str> = line.split('\t').collect();
        if f[0] == door {
            java.insert(f[1].to_owned(), (f[2].to_owned(), f[3].to_owned(), f[4].to_owned()));
        }
    }
    let dump = std::env::var("LITE_FRONT_MISMATCHES").ok();
    let mut counts: BTreeMap<String, (usize, usize, usize, usize)> = BTreeMap::new();
    let (mut asked, mut matched) = (0usize, 0usize);
    let mut differ: Vec<String> = Vec::new();
    for line in BufReader::new(File::open(env("LITE_FRONT_QUESTIONS")).unwrap()).lines() {
        let line = line.unwrap();
        let q = wire::parse(&line, 64).unwrap();
        let sha = q.get("sha").and_then(wire::Json::as_string).unwrap();
        let text = q.get("text").and_then(wire::Json::as_units).unwrap();
        let asked_here = q.get("asks").and_then(wire::Json::as_array).unwrap().iter().any(|a| asks(door, a.as_array().unwrap()));
        if !asked_here {
            continue;
        }
        let (java_spans, java_bare, java_kind) =
            java.get(&sha).unwrap_or_else(|| panic!("Java answered no {door} {sha}"));
        asked += 1;
        let (spans, bare) = engine_forms(door, text);
        let rust_kind = if spans.first() == Some(&u16::from(b'A')) { "A" } else { "E" };
        let spans_same = sha256_utf16le(&spans) == *java_spans;
        let bare_same = sha256_utf16le(&bare) == *java_bare;
        let c = counts.entry(format!("Java {java_kind}")).or_default();
        c.0 += 1;
        c.1 += usize::from(spans_same);
        c.2 += usize::from(bare_same);
        if spans_same && bare_same {
            matched += 1;
            c.3 += 1;
        } else {
            let head: String = String::from_utf16_lossy(&spans[..spans.len().min(160)]);
            let which = match (spans_same, bare_same) {
                (false, false) => "both forms",
                (false, true) => "the spans form",
                _ => "the bare form",
            };
            differ.push(format!("{sha}: Java {java_kind}, Rust {rust_kind}, {which} differ: {head}"));
            if let Some(dir) = &dump {
                std::fs::write(format!("{dir}/{door}.{sha}.rust.spans.txt"), String::from_utf16_lossy(&spans)).unwrap();
                std::fs::write(format!("{dir}/{door}.{sha}.rust.bare.txt"), String::from_utf16_lossy(&bare)).unwrap();
            }
        }
    }
    println!("[front-{door}] {asked} questions: {matched} answered as Java answers them, {} differ", differ.len());
    for (k, (n, s, b, m)) in &counts {
        println!("[front-{door}]   {k}: {m} of {n} identical (the spans form {s}, the bare form {b})");
    }
    for d in differ.iter().take(SHOWN) {
        println!("[front-{door}]   {d}");
    }
    assert_eq!(asked, java.len(), "Java answered every {door} question of the set, and only those");
    let floor = floor(door);
    assert!(matched >= floor, "{door}: {matched} identical, below its floor of {floor}");
}

#[test]
#[ignore = "reads the front oracle and Java's engine answers named by LITE_FRONT_QUESTIONS and LITE_FRONT_ENGINE"]
fn document() {
    differential("document");
}

#[test]
#[ignore = "reads the front oracle and Java's engine answers named by LITE_FRONT_QUESTIONS and LITE_FRONT_ENGINE"]
fn lambda_json() {
    differential("lambdaJson");
}
