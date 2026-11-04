// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `Character` predicates -- `isLetter`, `isDigit`, `isLetterOrDigit`, `isWhitespace`, `isSpaceChar`,
//! `isUpperCase`, `isLowerCase`, `isJavaIdentifierStart`, `isJavaIdentifierPart` -- exactly as the build JDK answers
//! them, for a UTF-16 unit (Java's `char` overload) and for a code point (its `int` overload). Java defines a `char`'s
//! answer as its value's as a code point, so one function takes both: a `u16` unit, a `char`, or a `u32` code point (a
//! surrogate unit is a code point of no class; a value past U+10FFFF is no code point and answers `false`, as Java's).
//!
//! The answers are the JDK's own: `rust/oracles/java-compat/extras/char-classes.tsv`, every code point's nine answers
//! as runs, written by `JavaCompatExtras.java` on the build's JDK (`jdk.txt` names it), packed into `character/runs.rs`
//! (3,182 runs, 12.7 KB) and held to the TSV by `tests/char_classes.rs`, which walks every code point. A JDK bump that
//! changes Java's Unicode tables regenerates the TSV (`JavaCompatExtras.java`) and then the packed runs
//! (`JAVACOMPAT_WRITE_CHAR_RUNS=1 cargo test -p lite-javacompat --test char_classes`).

mod runs;

use runs::RUNS;

/// The predicates in the order of the TSV's columns and of the packed runs' bits.
pub const PREDICATES: [&str; 9] = [
    "isLetter",
    "isDigit",
    "isLetterOrDigit",
    "isWhitespace",
    "isSpaceChar",
    "isUpperCase",
    "isLowerCase",
    "isJavaIdentifierStart",
    "isJavaIdentifierPart",
];

const LETTER: u32 = 1;
const DIGIT: u32 = 1 << 1;
const LETTER_OR_DIGIT: u32 = 1 << 2;
const WHITESPACE: u32 = 1 << 3;
const SPACE_CHAR: u32 = 1 << 4;
const UPPER_CASE: u32 = 1 << 5;
const LOWER_CASE: u32 = 1 << 6;
const JAVA_IDENTIFIER_START: u32 = 1 << 7;
const JAVA_IDENTIFIER_PART: u32 = 1 << 8;

/// The highest code point (Java's `Character.MAX_CODE_POINT`).
pub const MAX_CODE_POINT: u32 = 0x10FFFF;

/// The packed runs, each `first << 9 | answers` (the TSV's runs, in order): what the test holds to the TSV.
pub fn packed_runs() -> &'static [u32] {
    &RUNS
}

/// The nine answers of `cp`, bit `i` the `i`-th of [`PREDICATES`]; none for a value past U+10FFFF.
pub fn answers(cp: u32) -> u32 {
    if cp > MAX_CODE_POINT {
        return 0;
    }
    let at = RUNS.partition_point(|r| r >> 9 <= cp);
    RUNS[at - 1] & 0x1FF
}

fn has(c: impl Into<u32>, bit: u32) -> bool {
    answers(c.into()) & bit != 0
}

/// Java's `Character.isLetter`.
pub fn is_letter(c: impl Into<u32>) -> bool {
    has(c, LETTER)
}

/// Java's `Character.isDigit`: a decimal digit (`DECIMAL_DIGIT_NUMBER`) of any script.
pub fn is_digit(c: impl Into<u32>) -> bool {
    has(c, DIGIT)
}

/// Java's `Character.isLetterOrDigit`.
pub fn is_letter_or_digit(c: impl Into<u32>) -> bool {
    has(c, LETTER_OR_DIGIT)
}

/// Java's `Character.isWhitespace`.
pub fn is_whitespace(c: impl Into<u32>) -> bool {
    has(c, WHITESPACE)
}

/// Java's `Character.isSpaceChar`.
pub fn is_space_char(c: impl Into<u32>) -> bool {
    has(c, SPACE_CHAR)
}

/// Java's `Character.isUpperCase`.
pub fn is_upper_case(c: impl Into<u32>) -> bool {
    has(c, UPPER_CASE)
}

/// Java's `Character.isLowerCase`.
pub fn is_lower_case(c: impl Into<u32>) -> bool {
    has(c, LOWER_CASE)
}

/// Java's `Character.isJavaIdentifierStart`.
pub fn is_java_identifier_start(c: impl Into<u32>) -> bool {
    has(c, JAVA_IDENTIFIER_START)
}

/// Java's `Character.isJavaIdentifierPart`.
pub fn is_java_identifier_part(c: impl Into<u32>) -> bool {
    has(c, JAVA_IDENTIFIER_PART)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_few_answers_as_java_gives_them() {
        // ASCII
        assert!(is_letter('a') && !is_letter('1') && is_digit('7') && is_letter_or_digit('Z'));
        assert!(is_whitespace(' ') && is_whitespace('\t') && is_whitespace(0x1Fu16) && !is_whitespace(0xA0u16));
        assert!(is_space_char(0xA0u16) && is_upper_case('Q') && is_lower_case('q') && !is_lower_case('Q'));
        assert!(is_java_identifier_start('$') && is_java_identifier_part('9') && !is_java_identifier_start('9'));
        // past ASCII: an Arabic-Indic digit is a digit, a Roman numeral (a letter-number) is neither
        assert!(is_digit(0x0663u16) && is_letter_or_digit(0x0663u16));
        assert!(!is_letter(0x2163u16) && !is_digit(0x2163u16));
        // a supplementary digit as a code point; its surrogate units are of no class
        assert!(is_digit(0x1D7CEu32) && !is_digit(0xD835u16) && !is_digit(0xDFCEu16));
        // no code point
        assert!(!is_java_identifier_part(0x11_0000u32));
    }
}
