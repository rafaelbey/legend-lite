// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The model differential (docs/rust-port/PHASE3.md, slice 1b, 2026-11-12): every model text of the front oracle's
//! question set (`LITE_MODEL_QUESTIONS`, rust/oracles/front/questions.jsonl.zst decompressed), at every dialect it is
//! asked at (`["pmcd", dialect]`), through this crate's door (`door::parse`: rust/lite-front's parser, then
//! lite-model's door), its answer's canonical text (`canonical::form`) hashed and compared with the hash Java's door
//! gave the same ask (`LITE_MODEL_ANSWERS`, rust/oracles/model/answers.tsv: `ModelAnswers`' output, frozen before the
//! Java goes -- one line per ask, `dialect TAB hash TAB A|E`, in the questions file's order; rust/oracles/model/
//! MANIFEST.tsv). Every ask must agree: there is no list of deliberate differences, and a difference is a defect.
//!
//! With `LITE_MODEL_MISMATCHES` naming a directory, each mismatch's Rust text is written there as
//! `<dialect>.<sha>.rust.txt`; Java's is in the archive the MANIFEST names.

use lite_builtin::canonical::model_form;
use lite_front::doors::{Dialect, on_parse_stack};
use lite_front::sha256::sha256_utf16le;
use lite_front::wire;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

const SHOWN: usize = 25;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} names the file to read"))
}

#[test]
#[ignore = "reads the front oracle's questions and the model oracle (LITE_MODEL_QUESTIONS, LITE_MODEL_ANSWERS): \
            //rust/lite-builtin:model_differential_test runs it"]
fn every_model_text_answers_as_javas_door() {
    on_parse_stack(differential);
}

fn differential() {
    let mut java = BufReader::new(File::open(env("LITE_MODEL_ANSWERS")).unwrap()).lines();
    let dump = std::env::var("LITE_MODEL_MISMATCHES").ok();
    let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let (mut asked, mut matched) = (0usize, 0usize);
    let mut differ: Vec<String> = Vec::new();
    for line in BufReader::new(File::open(env("LITE_MODEL_QUESTIONS")).unwrap()).lines() {
        let line = line.unwrap();
        let q = wire::parse(&line, 64).unwrap();
        let sha = q.get("sha").and_then(wire::Json::as_string).unwrap();
        let text = q.get("text").and_then(wire::Json::as_units).unwrap();
        for a in q.get("asks").and_then(wire::Json::as_array).unwrap() {
            let pair = a.as_array().unwrap();
            let (door, dialect_name) = (pair[0].as_string().unwrap(), pair[1].as_string().unwrap());
            if door != "pmcd" {
                continue;
            }
            let java_line = java
                .next()
                .unwrap_or_else(|| panic!("Java answered no pmcd {dialect_name} {sha}"))
                .unwrap();
            let f: Vec<&str> = java_line.split('\t').collect();
            assert_eq!(
                f.len(),
                3,
                "a line of the model oracle is dialect, hash, A or E: {java_line}"
            );
            assert_eq!(
                f[0], dialect_name,
                "the model oracle is out of step with the questions at {sha}"
            );
            let dialect = Dialect::from_java(&dialect_name).expect("a dialect");
            let form = model_form(text, dialect);
            let kind = if form.starts_with(&[u16::from(b'E'), u16::from(b'\t')]) {
                "E"
            } else {
                "A"
            };
            let hash = sha256_utf16le(&form);
            asked += 1;
            let c = counts.entry(format!("{dialect_name} {}", f[2])).or_default();
            c.0 += 1;
            if hash == f[1] && kind == f[2] {
                matched += 1;
                c.1 += 1;
            } else {
                differ.push(format!("{dialect_name} {sha} (Java {}, Rust {kind})", f[2]));
                if let Some(dir) = &dump {
                    std::fs::write(
                        format!("{dir}/{dialect_name}.{sha}.rust.txt"),
                        String::from_utf16_lossy(&form),
                    )
                    .unwrap();
                }
            }
        }
    }
    assert!(
        java.next().is_none(),
        "the model oracle holds more lines than the questions' pmcd asks"
    );
    println!(
        "[model-differential] {asked} asks, {matched} identical to Java's door, {} differ",
        differ.len()
    );
    for (k, (n, m)) in &counts {
        println!("[model-differential]   {k}: {m} of {n}");
    }
    for d in differ.iter().take(SHOWN) {
        println!("[model-differential]   differs: {d}");
    }
    assert!(
        differ.is_empty(),
        "{} of {asked} model answers differ from Java's door",
        differ.len()
    );
}
