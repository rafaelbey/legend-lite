// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `double_to_string` against the JDK's own answers: every case of the frozen conformance snapshot's
//! `double.toString.edges`, `double.toString.random` and `double.shortDecimals` families
//! (rust/oracles/java-compat/double-tostring.tsv).

use std::path::PathBuf;

/// The golden file: Bazel names it in `JAVA_COMPAT_DOUBLES`; cargo finds it beside the crate.
fn golden() -> PathBuf {
    match std::env::var_os("JAVA_COMPAT_DOUBLES") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oracles/java-compat/double-tostring.tsv"),
    }
}

#[test]
fn every_case_spells_as_the_jdk_does() {
    let path = golden();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut cases = 0;
    let mut wrong = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let (bits, java) = line.split_once('\t').expect("<bits>\\t<text>");
        let v = f64::from_bits(u64::from_str_radix(bits, 16).expect("hex bits"));
        let ours = lite_javacompat::double_to_string(v);
        if ours != java {
            wrong.push(format!("{bits}: the JDK writes {java}, we write {ours}"));
        }
        cases += 1;
    }
    assert!(cases > 80_000, "only {cases} cases read from {}", path.display());
    assert!(wrong.is_empty(), "{} of {cases} differ, the first:\n{}", wrong.len(), wrong[..wrong.len().min(25)].join("\n"));
}
