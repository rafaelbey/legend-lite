// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! lite-front's doors held to Java's (docs/rust-port/PHASE2.md §3, the differential): every question of the front
//! oracle (`LITE_FRONT_QUESTIONS`) whose door Rust answers, answered by Rust and written in the canonical form
//! (`lite_front::doors::answer_form`), that form's SHA-256 compared with the one Java's door gave the same question
//! (`LITE_FRONT_ANSWERS`, rust/oracles/front/answers.tsv: `//core:front_answers`' output frozen before the Java parser
//! was deleted, step 6a -- one line per ask, in the questions file's order; rust/oracles/front/MANIFEST.tsv).
//!
//! One test per door asserts at least its floor (`tests/<door>-floor.txt` among `LITE_FRONT_FLOORS`) of its questions
//! identical; a floor only rises. Each prints its counts by dialect and kind and its first mismatches; with
//! `LITE_FRONT_MISMATCHES` naming a directory, each mismatch's Rust form is written there as
//! `<door>.<dialect>.<sha>.rust.txt`. Java's answer to it is in the archive MANIFEST.tsv names (the line whose door,
//! dialect and sha are the question's); while the Java parser exists,
//! `bazel run //core:front_show -- QUESTIONS DIR door:dialect:sha...` writes it beside Rust's.
//!
//! Java's model answers were given with one overlay grammar registered, the test jar's `ToySectionGrammar`
//! (core/src/test/java/com/legend/parser/ToySectionGrammar.java); the model door's questions are asked with it
//! registered too, its part played here as the shim plays an overlay's ([`toy_splice`]).

#[path = "support/deliberate.rs"]
mod deliberate;

use deliberate::{Deliberate, is_the_new_answer};
use lite_front::doors::{Answer, Dialect, answer_form, pmcd_answer_form};
use lite_front::element_parser::lines_of;
use lite_front::jtypes::JStr;
use lite_front::model_parser::{OverlayGrammar, PmcdAnswer};
use lite_front::records::{Element, POpaqueElement, PmcdParserParsed};
use lite_front::sha256::sha256_utf16le;
use lite_front::ustr::{ends_with, starts_with, strip, u};
use lite_front::wire;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

const SHOWN: usize = 25;

/// `ToySectionGrammar.parse`: each line `Toy x::Name;` (stripped) an element `x::Name`, its JSON
/// `{"_type":"toy","name":"x::Name"}`.
fn toy(text: &[u16]) -> Vec<(Vec<u16>, Vec<u16>)> {
    let mut out = Vec::new();
    for line in lines_of(text) {
        let t = strip(line);
        if starts_with(t, "Toy ") && ends_with(t, ";") {
            let fqn = strip(&t[4..t.len() - 1]).to_vec();
            let mut json = u("{\"_type\":\"toy\",\"name\":\"");
            json.extend_from_slice(&fqn);
            json.extend(u("\"}"));
            out.push((fqn, json));
        }
    }
    out
}

/// The shim's part for the toy overlay (Java's `PmcdParser.registered` and `OverlayElementSink`): the grammar called
/// on each overlay section's text, in section order, before the door's own error is raised; its elements spliced where
/// the door says, as `POpaqueElement`s with no span, their paths, each once, the section's index entry.
fn toy_splice(text: &[u16], answer: PmcdAnswer) -> Answer<PmcdParserParsed> {
    let called: Vec<Vec<(Vec<u16>, Vec<u16>)>> = answer
        .overlays
        .iter()
        .map(|o| toy(&text[o.start_offset..o.end_offset]))
        .collect();
    let mut parsed = answer.parsed?;
    for (o, elements) in answer.overlays.iter().zip(called).rev() {
        let Some(Element::PSectionIndex(index)) = parsed.model.elements.last_mut() else {
            panic!("the section index is the answer's last element");
        };
        let paths = &mut index.sections[o.section_index].elements;
        for (fqn, _) in &elements {
            let path = Some(JStr(fqn.clone()));
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        for (k, (fqn, json)) in elements.into_iter().enumerate() {
            parsed.model.elements.insert(
                o.element_index + k,
                Element::POpaqueElement(Box::new(POpaqueElement {
                    qualified_name: JStr(fqn),
                    section_name: JStr(o.name.clone()),
                    protocol_json: JStr(json),
                    source_information: None,
                })),
            );
        }
    }
    Ok(parsed)
}

/// A question's canonical answer, the model door's with the toy overlay registered.
fn form(door: &str, dialect: Dialect, text: &[u16]) -> Vec<u16> {
    if door != "pmcd" {
        return answer_form(door, dialect, text);
    }
    let toy_name = u("Toy");
    let overlays = [OverlayGrammar {
        name: &toy_name,
        lexable: false,
    }];
    pmcd_answer_form(dialect, text, &overlays, &toy_splice)
}

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

fn differential(door: &str) {
    // [door, dialect, the form's sha, A or E] per ask, in the questions file's order: walked in lockstep with it
    let mut java = BufReader::new(File::open(env("LITE_FRONT_ANSWERS")).unwrap()).lines();
    let dump = std::env::var("LITE_FRONT_MISMATCHES").ok();
    let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let (mut asked, mut matched, mut listed) = (0usize, 0usize, 0usize);
    let mut differ: Vec<String> = Vec::new();
    let mut deliberate = Deliberate::load();
    let mut wrong: Vec<String> = Vec::new();
    for line in BufReader::new(File::open(env("LITE_FRONT_QUESTIONS")).unwrap()).lines() {
        let line = line.unwrap();
        let q = wire::parse(&line, 64).unwrap();
        let sha = q.get("sha").and_then(wire::Json::as_string).unwrap();
        let text = q.get("text").and_then(wire::Json::as_units).unwrap();
        for a in q.get("asks").and_then(wire::Json::as_array).unwrap() {
            let pair = a.as_array().unwrap();
            let (ask_door, dialect_name) = (pair[0].as_string().unwrap(), pair[1].as_string().unwrap());
            let java_line = java
                .next()
                .unwrap_or_else(|| panic!("Java answered no {ask_door} {dialect_name} {sha}"))
                .unwrap();
            let f: Vec<&str> = java_line.split('\t').collect();
            assert!(
                f.len() == 4 && f[0] == ask_door && f[1] == dialect_name,
                "the frozen answers are out of step with the questions at {ask_door} {dialect_name} {sha}: {java_line}"
            );
            if ask_door != door {
                continue;
            }
            let (java_hash, java_kind) = (f[2], f[3]);
            let dialect = Dialect::from_java(&dialect_name).unwrap();
            asked += 1;
            let form = form(door, dialect, text);
            let rust_kind = if form.first() == Some(&u16::from(b'A')) {
                "A"
            } else {
                "E"
            };
            let c = counts.entry(format!("{dialect_name} {java_kind}")).or_default();
            c.0 += 1;
            if let Err(raw) = deliberate.raw_answer(door, &dialect_name, &sha, &form) {
                wrong.push(raw);
            }
            if let Some(expected) = deliberate.expected(door, &dialect_name, &sha) {
                listed += 1;
                if sha256_utf16le(&form) == *java_hash {
                    wrong.push(format!(
                        "{door} {dialect_name} {sha}: listed, yet identical to Java's frozen answer"
                    ));
                }
                if let Err(m) = is_the_new_answer(&form, expected) {
                    wrong.push(format!("{door} {dialect_name} {sha}: {m}"));
                }
            } else if sha256_utf16le(&form) == *java_hash {
                matched += 1;
                c.1 += 1;
            } else {
                let head: String = String::from_utf16_lossy(&form[..form.len().min(160)]);
                differ.push(format!(
                    "{dialect_name} {sha}: Java {java_kind}, Rust {rust_kind}: {head}"
                ));
                if let Some(dir) = &dump {
                    let path = format!("{dir}/{door}.{dialect_name}.{sha}.rust.txt");
                    std::fs::write(path, String::from_utf16_lossy(&form)).unwrap();
                }
            }
        }
    }
    println!(
        "[front-{door}] {asked} questions: {matched} answered as Java answers them, {listed} listed differences, {} differ",
        differ.len()
    );
    for (k, (n, m)) in &counts {
        println!("[front-{door}]   {k}: {m} of {n}");
    }
    for d in differ.iter().take(SHOWN) {
        println!("[front-{door}]   {d}");
    }
    assert!(
        java.next().is_none(),
        "Java answered every question of the set, and only those"
    );
    assert!(
        wrong.is_empty(),
        "{door}: the deliberate differences and the raw-answer pin: {wrong:#?}"
    );
    assert_eq!(
        listed,
        deliberate.listed(door),
        "{door}: every listed question was asked"
    );
    let stale = deliberate.stale_left(door);
    assert!(
        stale.is_empty(),
        "{door}: left-for-the-user lines that no longer hold: {stale:?}"
    );
    assert!(
        differ.is_empty(),
        "{door}: {} questions neither identical nor listed",
        differ.len()
    );
    assert_eq!(matched + listed, asked, "{door}: identical + listed == total");
    let floor = floor(door);
    assert!(
        matched + listed >= floor,
        "{door}: {matched} identical and {listed} listed, below its floor of {floor}"
    );
}

#[test]
#[ignore = "reads the front oracle and Java's answers named by LITE_FRONT_QUESTIONS and LITE_FRONT_ANSWERS"]
fn spec() {
    differential("spec");
}

#[test]
#[ignore = "reads the front oracle and Java's answers named by LITE_FRONT_QUESTIONS and LITE_FRONT_ANSWERS"]
fn lambda() {
    differential("lambda");
}

#[test]
#[ignore = "reads the front oracle and Java's answers named by LITE_FRONT_QUESTIONS and LITE_FRONT_ANSWERS"]
fn block() {
    differential("block");
}

#[test]
#[ignore = "reads the front oracle and Java's answers named by LITE_FRONT_QUESTIONS and LITE_FRONT_ANSWERS"]
fn pmcd() {
    differential("pmcd");
}
