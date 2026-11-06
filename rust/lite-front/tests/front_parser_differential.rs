// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! lite-front's doors held to Java's (docs/rust-port/PHASE2.md §3, the differential): every question of the front
//! oracle (`LITE_FRONT_QUESTIONS`) whose door Rust answers, answered by Rust and written in the canonical form
//! (`lite_front::doors::answer_form`), that form's SHA-256 compared with the one Java's door gave the same question
//! (`LITE_FRONT_ANSWERS`, //core:front_answers: a cached action that runs the Java parser, live while it exists).
//!
//! One test per door asserts at least its floor (`tests/<door>-floor.txt` among `LITE_FRONT_FLOORS`) of its questions
//! identical; a floor only rises. Each prints its counts by dialect and kind and its first mismatches; with
//! `LITE_FRONT_MISMATCHES` naming a directory, each mismatch's Rust form is written there as
//! `<door>.<dialect>.<sha>.rust.txt`, and `bazel run //core:front_show -- QUESTIONS DIR door:dialect:sha...` writes
//! Java's beside it.
//!
//! Java's model answers were given with one overlay grammar registered, the test jar's `ToySectionGrammar`
//! (core/src/test/java/com/legend/parser/ToySectionGrammar.java); the model door's questions are asked with it
//! registered too, its part played here as the shim plays an overlay's ([`toy_splice`]).

use lite_front::doors::{Answer, Dialect, answer_form, pmcd_answer_form};
use lite_front::element_parser::lines_of;
use lite_front::jtypes::JStr;
use lite_front::model_parser::{OverlayGrammar, PmcdAnswer};
use lite_front::records::{Element, POpaqueElement, PmcdParserParsed};
use lite_front::sha256::sha256_utf16le;
use lite_front::ustr::{ends_with, starts_with, strip, u};
use lite_front::wire;
use std::collections::{BTreeMap, HashMap};
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
    let called: Vec<Vec<(Vec<u16>, Vec<u16>)>> =
        answer.overlays.iter().map(|o| toy(&text[o.start_offset..o.end_offset])).collect();
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
    let overlays = [OverlayGrammar { name: &toy_name, lexable: false }];
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
    let mut java: HashMap<(String, String, String), (String, String)> = HashMap::new();
    for line in BufReader::new(File::open(env("LITE_FRONT_ANSWERS")).unwrap()).lines() {
        let line = line.unwrap();
        let f: Vec<&str> = line.split('\t').collect();
        if f[0] == door {
            java.insert((f[0].to_owned(), f[1].to_owned(), f[2].to_owned()), (f[3].to_owned(), f[4].to_owned()));
        }
    }
    let dump = std::env::var("LITE_FRONT_MISMATCHES").ok();
    let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let (mut asked, mut matched) = (0usize, 0usize);
    let mut differ: Vec<String> = Vec::new();
    for line in BufReader::new(File::open(env("LITE_FRONT_QUESTIONS")).unwrap()).lines() {
        let line = line.unwrap();
        let q = wire::parse(&line, 64).unwrap();
        let sha = q.get("sha").and_then(wire::Json::as_string).unwrap();
        let text = q.get("text").and_then(wire::Json::as_units).unwrap();
        for a in q.get("asks").and_then(wire::Json::as_array).unwrap() {
            let pair = a.as_array().unwrap();
            if pair[0].as_string().unwrap() != door {
                continue;
            }
            let dialect_name = pair[1].as_string().unwrap();
            let dialect = Dialect::from_java(&dialect_name).unwrap();
            let (java_hash, java_kind) = java
                .get(&(door.to_owned(), dialect_name.clone(), sha.clone()))
                .unwrap_or_else(|| panic!("Java answered no {door} {dialect_name} {sha}"));
            asked += 1;
            let form = form(door, dialect, text);
            let rust_kind = if form.first() == Some(&u16::from(b'A')) { "A" } else { "E" };
            let c = counts.entry(format!("{dialect_name} {java_kind}")).or_default();
            c.0 += 1;
            if sha256_utf16le(&form) == *java_hash {
                matched += 1;
                c.1 += 1;
            } else {
                let head: String = String::from_utf16_lossy(&form[..form.len().min(160)]);
                differ.push(format!("{dialect_name} {sha}: Java {java_kind}, Rust {rust_kind}: {head}"));
                if let Some(dir) = &dump {
                    let path = format!("{dir}/{door}.{dialect_name}.{sha}.rust.txt");
                    std::fs::write(path, String::from_utf16_lossy(&form)).unwrap();
                }
            }
        }
    }
    println!("[front-{door}] {asked} questions: {matched} answered as Java answers them, {} differ", differ.len());
    for (k, (n, m)) in &counts {
        println!("[front-{door}]   {k}: {m} of {n}");
    }
    for d in differ.iter().take(SHOWN) {
        println!("[front-{door}]   {d}");
    }
    assert_eq!(asked, java.len(), "Java answered every {door} question of the set, and only those");
    let floor = floor(door);
    assert!(matched >= floor, "{door}: {matched} identical, below its floor of {floor}");
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
