// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The element differential (docs/rust-port/PHASE3.md, the element slice, 2026-11-15): milestone 1, the boot layer.

mod support;

use lite_front::codec::{self, Codec, Writer};
use lite_front::sha256::sha256_utf16le;
use lite_model::NormalizedModel;

use support::boot::{Outcome, boot_normalized};

fn env_opt(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// The SHA-256 of a codec text's UTF-16 units, little-endian (ElementAnswers' `hash`).
fn hash(text: &str) -> String {
    sha256_utf16le(&units(text))
}

/// `ElementAnswers.bootNormalized`: one line per element, in order (`index TAB fqn TAB` the SHA-256 of its record-codec
/// text, tagged: written as a `PackageableElement`), then `imports` (untagged), `legacySurfaces` (the values as a list,
/// each untagged) and the whole `NormalizedModel` (untagged).
fn boot_normalized_lines(model: &NormalizedModel) -> Vec<Vec<u16>> {
    let mut out = Vec::with_capacity(model.elements.len() + 3);
    for (i, el) in model.elements.iter().enumerate() {
        let mut line = units(&format!("{i}\t"));
        line.extend_from_slice(&el.qualified_name().0);
        line.extend(units(&format!("\t{}", hash(&codec::write(el, false)))));
        out.push(line);
    }
    out.push(units(&format!(
        "imports\t-\t{}",
        hash(&codec::write(&*model.imports, true))
    )));
    let mut w = Writer::new();
    w.raw("[");
    for (i, (_, x)) in model.legacy_surfaces.iter().enumerate() {
        if i > 0 {
            w.raw(",");
        }
        x.write(&mut w, true);
    }
    w.raw("]");
    out.push(units(&format!(
        "legacySurfaces\t{}\t{}",
        model.legacy_surfaces.len(),
        hash(&w.finish())
    )));
    out.push(units(&format!(
        "NormalizedModel\t{}\t{}",
        model.elements.len(),
        hash(&codec::write(model, true))
    )));
    out
}

/// The repository's oracle file beside this crate, or the one `env` names.
fn oracle_path(env: &str, file: &str) -> String {
    env_opt(env).unwrap_or_else(|| format!("{}/../oracles/element/{file}", env!("CARGO_MANIFEST_DIR")))
}

/// MILESTONE 1: the boot layer as `Compiler.boot()` normalizes it, element by element, then its imports', legacy
/// surfaces' and whole model's lines, equal to rust/oracles/element/boot-normalized.tsv (ElementAnswers'
/// `bootNormalized`). No element compiler: D and E alone.
#[test]
fn the_boot_layer_normalizes_as_javas() {
    let path = oracle_path("LITE_ELEMENT_BOOT_NORMALIZED", "boot-normalized.tsv");
    let java: Vec<Vec<u16>> = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the boot's normalized lines {path}: {e}"))
        .lines()
        .map(units)
        .collect();
    let boot = match boot_normalized() {
        Outcome::Done(b) => b,
        Outcome::Threw(e) => panic!("the boot layer does not normalize: {e:?}"),
        Outcome::Panicked(m) => panic!("the boot layer panicked: {m}"),
    };
    let rust = boot_normalized_lines(&boot.model);
    if let Some(dir) = env_opt("LITE_ELEMENT_REPORT") {
        std::fs::create_dir_all(&dir).unwrap();
        let text: Vec<String> = rust.iter().map(|l| String::from_utf16_lossy(l)).collect();
        std::fs::write(format!("{dir}/boot-normalized.rust.tsv"), text.join("\n") + "\n").unwrap();
    }
    let key = |l: &Vec<u16>| -> Vec<u16> {
        // index TAB fqn: the line less its hash
        let tabs: Vec<usize> = l
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == u16::from(b'\t'))
            .map(|(i, _)| i)
            .collect();
        l[..tabs.get(1).copied().unwrap_or(l.len())].to_vec()
    };
    let order_differs = java.len() != rust.len() || java.iter().zip(&rust).any(|(j, r)| key(j) != key(r));
    let mut differences = Vec::new();
    for (i, (j, r)) in java.iter().zip(&rust).enumerate() {
        if j != r {
            differences.push(format!(
                "  line {i}: java {} | rust {}",
                String::from_utf16_lossy(j),
                String::from_utf16_lossy(r)
            ));
        }
    }
    let shown: Vec<&String> = differences.iter().take(25).collect();
    assert!(
        !order_differs && differences.is_empty(),
        "the boot layer's normalized model is not Java's: {} java lines, {} rust lines{}, {} lines differ:\n{}",
        java.len(),
        rust.len(),
        if order_differs {
            " (the elements' order or names differ)"
        } else {
            ""
        },
        differences.len(),
        shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n")
    );
}
