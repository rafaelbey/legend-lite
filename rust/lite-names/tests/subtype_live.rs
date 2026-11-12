// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Subtyping on the boot model held to the live Java (the owner's ruling 22 of docs/rust-port/PHASE3_SHIM_DESIGN.md
//! §10: held to the live Java, not frozen; made in slice 1b in rust/lite-builtin, moved here 2026-11-12 with the names
//! slice, which resolves the boot layer's names): `LITE_SUBTYPE_JAVA` names rust/oracles/builtin/SubtypeProbe.java's
//! output on the tree's built jars (a line per child of the universe: the parents, sorted, it is a subtype of), and this
//! crate's lattice answers every ordered pair of the same universe identically -- every class, none held apart.
//! Ignored unless run by hand while Java exists: its answers are not frozen.

use std::collections::BTreeSet;

use lite_front::jtypes::JStr;
use lite_names::lattice::boot;

#[test]
#[ignore = "asks the live Java's answers (LITE_SUBTYPE_JAVA): run by hand while Java exists (ruling 22)"]
fn subtyping_on_the_boot_model_answers_as_javas() {
    let path = std::env::var("LITE_SUBTYPE_JAVA").expect("LITE_SUBTYPE_JAVA names SubtypeProbe's output");
    let java: Vec<(String, String)> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|l| {
            let (c, p) = l.split_once('\t').unwrap();
            (c.to_owned(), p.to_owned())
        })
        .collect();
    let universe: BTreeSet<String> = java.iter().map(|(c, _)| c.clone()).collect();
    let rust = lite_front::doors::on_parse_stack(|| {
        let lattice = boot().expect("the boot layer resolves and indexes");
        let mut lines = Vec::new();
        for child in &universe {
            let mut parents = Vec::new();
            let mut failure = None;
            for parent in &universe {
                match lattice.is_subtype(&JStr::from(child.as_str()), &JStr::from(parent.as_str())) {
                    Ok(true) => parents.push(parent.clone()),
                    Ok(false) => {}
                    Err(e) => {
                        failure = Some(format!("!ModelException: {e}"));
                        break;
                    }
                }
            }
            lines.push((child.clone(), failure.unwrap_or_else(|| parents.join(" "))));
        }
        lines
    });
    let (mut pairs, mut yes, mut differ) = (0usize, 0usize, Vec::new());
    for ((c, j), (_, r)) in java.iter().zip(&rust) {
        pairs += universe.len();
        yes += r.split(' ').filter(|s| !s.is_empty()).count();
        if j != r {
            differ.push(format!("{c}\n  java: {j}\n  rust: {r}"));
        }
    }
    println!(
        "[subtype-live] {} classes, {pairs} pairs, {yes} subtype in Rust; {} of the classes differ",
        universe.len(),
        differ.len()
    );
    for d in differ.iter().take(10) {
        println!("[subtype-live] {d}");
    }
    assert!(differ.is_empty());
}
