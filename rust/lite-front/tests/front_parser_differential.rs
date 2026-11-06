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

use lite_front::doors::{Dialect, answer_form};
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
            let form = answer_form(door, dialect, text);
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
