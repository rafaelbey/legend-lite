// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Subtyping on the boot model held to the live Java (2026-11-12, the owner's ruling 22 of
//! docs/rust-port/PHASE3_SHIM_DESIGN.md §10: held to the live Java in slice 1b, not frozen): `LITE_SUBTYPE_JAVA` names
//! rust/oracles/builtin/SubtypeProbe.java's output on the tree's built jars (a line per child of the universe: the
//! parents, sorted, it is a subtype of), and this crate's lattice answers every ordered pair of the same universe
//! identically. Ignored unless run by hand while Java exists: its answers are not frozen.

use std::collections::BTreeSet;

use lite_builtin::lattice::boot;
use lite_front::jtypes::JStr;

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
        let lattice = boot();
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
    // a child whose walk meets a supertype spelled by its simple name (the prelude's, under its section's imports) needs
    // name resolution, which is lite-names' (step D): Java resolves it before F compiles the class; held apart, counted
    let simple = lite_front::doors::on_parse_stack(|| {
        universe
            .iter()
            .filter(|c| meets_a_simple_name(c))
            .cloned()
            .collect::<BTreeSet<String>>()
    });
    let (mut pairs, mut yes, mut differ, mut apart, mut apart_differ) = (0usize, 0usize, Vec::new(), 0usize, 0usize);
    for ((c, j), (_, r)) in java.iter().zip(&rust) {
        pairs += universe.len();
        yes += r.split(' ').filter(|s| !s.is_empty()).count();
        if simple.contains(c) {
            apart += 1;
            if j != r {
                apart_differ += 1;
            }
            continue;
        }
        if j != r {
            differ.push(format!("{c}\n  java: {j}\n  rust: {r}"));
        }
    }
    println!(
        "[subtype-live] {} classes, {pairs} pairs, {yes} subtype in Rust; {} children whose walk meets only FQNs, {} \
         of them differ; {apart} meet a simple name (name resolution, lite-names'), {apart_differ} of them differ",
        universe.len(),
        universe.len() - apart,
        differ.len()
    );
    for d in differ.iter().take(10) {
        println!("[subtype-live] {d}");
    }
    assert!(differ.is_empty());
}

/// Whether the child's declared walk meets a supertype head with no package: one Java resolves by name first.
fn meets_a_simple_name(child: &str) -> bool {
    let lattice = boot();
    let mut seen = BTreeSet::new();
    let mut work = vec![JStr::from(child)];
    while let Some(cur) = work.pop() {
        if !seen.insert(cur.clone()) {
            continue;
        }
        if let Some(def) = lattice.find(&cur) {
            for s in def.super_classes.iter() {
                let Ok(head) = lite_model::subtyping::head_fqn(s) else {
                    continue;
                };
                if !head.lossy().contains("::") {
                    return true;
                }
                work.push(head.clone());
            }
        }
    }
    false
}
