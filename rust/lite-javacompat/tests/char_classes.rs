// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `lite_javacompat::character` against the JDK's own answers: every code point 0..=0x10FFFF, each of Java's nine
//! `Character` predicates, as rust/oracles/java-compat/extras/char-classes.tsv records them (the build JDK's, written by
//! `JavaCompatExtras.java`); and the packed runs the crate embeds (`src/character/runs.rs`) held to the TSV's runs.
//! With `JAVACOMPAT_WRITE_CHAR_RUNS=1` (cargo, in the crate's checkout) the packed runs are rewritten from the TSV
//! instead: how they are regenerated after a JDK bump regenerates the TSV.

use lite_javacompat::character::{self, MAX_CODE_POINT, PREDICATES};
use std::path::PathBuf;

/// The golden file: Bazel names it in `JAVA_COMPAT_CHAR_CLASSES`; cargo finds it beside the crate.
fn golden() -> PathBuf {
    match std::env::var_os("JAVA_COMPAT_CHAR_CLASSES") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oracles/java-compat/extras/char-classes.tsv"),
    }
}

/// The TSV's runs: first code point, last, and the nine answers (bit `i` the `i`-th column).
fn runs() -> Vec<(u32, u32, u32)> {
    let path = golden();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut lines = text.lines();
    let header = lines.next().expect("a header");
    assert_eq!(header, format!("# first\tlast\t{}", PREDICATES.join(",")), "the TSV's columns are the predicates'");
    lines
        .map(|line| {
            let mut f = line.split('\t');
            let first = u32::from_str_radix(f.next().expect("first"), 16).expect("hex");
            let last = u32::from_str_radix(f.next().expect("last"), 16).expect("hex");
            let flags = f.next().expect("flags");
            assert_eq!(flags.len(), PREDICATES.len(), "{line}");
            let bits = flags.bytes().enumerate().fold(0, |b, (i, c)| if c == b'1' { b | 1 << i } else { b });
            (first, last, bits)
        })
        .collect()
}

#[test]
fn every_code_point_answers_as_the_jdk_does() {
    let runs = runs();
    let mut next = 0;
    let mut wrong = Vec::new();
    for (first, last, bits) in &runs {
        assert_eq!(*first, next, "the runs cover every code point in order");
        for cp in *first..=*last {
            let ours = [
                character::is_letter(cp),
                character::is_digit(cp),
                character::is_letter_or_digit(cp),
                character::is_whitespace(cp),
                character::is_space_char(cp),
                character::is_upper_case(cp),
                character::is_lower_case(cp),
                character::is_java_identifier_start(cp),
                character::is_java_identifier_part(cp),
            ];
            for (i, answer) in ours.iter().enumerate() {
                if *answer != (bits >> i & 1 == 1) && wrong.len() < 25 {
                    wrong.push(format!("U+{cp:04X} {}: the JDK answers {}", PREDICATES[i], !answer));
                }
            }
            // a UTF-16 unit is its value as a code point (Java's char overloads)
            if let Ok(unit) = u16::try_from(cp) {
                assert_eq!(character::answers(u32::from(unit)), character::answers(cp));
                assert_eq!(character::is_letter(unit), character::is_letter(cp));
            }
        }
        next = last + 1;
    }
    assert_eq!(next, MAX_CODE_POINT + 1, "the runs end at the last code point");
    assert_eq!(character::answers(MAX_CODE_POINT + 1), 0);
    assert!(wrong.is_empty(), "differ from the JDK:\n{}", wrong.join("\n"));
}

#[test]
fn the_packed_runs_are_the_tsvs() {
    let packed: Vec<u32> = runs().iter().map(|(first, _, bits)| first << 9 | bits).collect();
    if std::env::var_os("JAVACOMPAT_WRITE_CHAR_RUNS").is_some() {
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/character/runs.rs");
        std::fs::write(&out, source(&packed)).unwrap_or_else(|e| panic!("{}: {e}", out.display()));
        return;
    }
    assert!(
        character::packed_runs() == packed.as_slice(),
        "src/character/runs.rs is not the TSV's runs: regenerate it with JAVACOMPAT_WRITE_CHAR_RUNS=1 cargo test -p \
         lite-javacompat --test char_classes"
    );
}

/// `src/character/runs.rs` for `packed`.
fn source(packed: &[u32]) -> String {
    let rows: Vec<String> = packed
        .chunks(8)
        .map(|c| format!("    {},", c.iter().map(|r| format!("0x{r:08X}")).collect::<Vec<_>>().join(", ")))
        .collect();
    format!(
        "// Copyright 2026 Legend Contributors\n// SPDX-License-Identifier: Apache-2.0\n\n// GENERATED from \
         rust/oracles/java-compat/extras/char-classes.tsv (the build JDK's Character classes over every code\n// \
         point); regenerate with `JAVACOMPAT_WRITE_CHAR_RUNS=1 cargo test -p lite-javacompat --test char_classes`, \
         which\n// `char_classes_test` holds this file to. Each entry is a run of code points with one set of \
         answers: its first code\n// point << 9 | its answers, bit i the i-th of `character::PREDICATES`.\n\npub(super) \
         static RUNS: [u32; {}] = [\n{}\n];\n",
        packed.len(),
        rows.join("\n")
    )
}
