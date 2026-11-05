// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! lite-front's lexer held to Java's (docs/rust-port/PHASE2.md §4 step 2): every text of the front oracle's question
//! set (`LITE_FRONT_QUESTIONS`, rust/oracles/front/questions.jsonl) lexed by Rust, its token stream written in the
//! canonical form (`lite_front::canonical_tokens`) and that form's SHA-256 compared with the one Java's lexer gave the
//! same text (`LITE_FRONT_TOKENS`, //core:front_tokens: a cached action that runs the Java lexer, live while it exists).
//! Every text's id is recomputed from its units too, which holds the UTF-16 reading of the oracle to Java's.

use lite_front::lexer::tokenize;
use lite_front::sha256::sha256_utf16le;
use lite_front::{canonical_tokens, wire};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

const SHOWN: usize = 20;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} names the file to read"))
}

#[test]
#[ignore = "reads the front oracle named by LITE_FRONT_QUESTIONS and Java's hashes named by LITE_FRONT_TOKENS"]
fn every_text_lexes_as_the_java_lexer_lexes_it() {
    let mut java: HashMap<String, (String, usize)> = HashMap::new();
    for line in BufReader::new(File::open(env("LITE_FRONT_TOKENS")).unwrap()).lines() {
        let line = line.unwrap();
        let mut f = line.split('\t');
        let (sha, hash, count) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        java.insert(sha.to_owned(), (hash.to_owned(), count.parse().unwrap()));
    }
    let (mut texts, mut matched, mut tokens) = (0usize, 0usize, 0usize);
    let mut differ: Vec<String> = Vec::new();
    for line in BufReader::new(File::open(env("LITE_FRONT_QUESTIONS")).unwrap()).lines() {
        let line = line.unwrap();
        let q = wire::parse(&line, 64).unwrap();
        let sha = q.get("sha").and_then(wire::Json::as_string).expect("a question's sha");
        let text = q.get("text").and_then(wire::Json::as_units).expect("a question's text");
        assert_eq!(sha256_utf16le(text), sha, "a text's id is the SHA-256 of its UTF-16 units");
        texts += 1;
        let ts = tokenize(text);
        tokens += ts.count();
        let form = sha256_utf16le(&canonical_tokens(&ts));
        let (java_form, java_count) = java.get(&sha).unwrap_or_else(|| panic!("Java lexed no text {sha}"));
        if form == *java_form {
            matched += 1;
        } else {
            differ.push(format!("{sha}: Rust {} tokens, Java {java_count}", ts.count()));
        }
    }
    println!("[front-lexer] {texts} texts, {tokens} tokens: {matched} lex as Java lexes them, {} differ", differ.len());
    for d in differ.iter().take(SHOWN) {
        println!("[front-lexer]   {d}");
    }
    assert_eq!(texts, java.len(), "Java lexed every text of the question set, and only those");
    assert!(differ.is_empty(), "{} texts lex differently from Java", differ.len());
}
