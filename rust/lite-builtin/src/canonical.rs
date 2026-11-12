// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The model differential's canonical text (docs/rust-port/PHASE3.md, slice 1b): the front oracle's form over the
//! door's answer -- `A` and the lossless codec's JSON of the `ParsedModel` (every record, every component, in
//! canonical-constructor order, a map in its iteration order), or `E`, the error's class, its line and column (a
//! `ParseException`'s, empty otherwise) and its message, tab separated -- as UTF-16 units. Java writes the same text
//! over its door's answer (`core/src/test/java/com/legend/frontoracle/ModelAnswers.java`), and the two are held equal
//! by their SHA-256.

use lite_front::codec;
use lite_front::doors::{Dialect, JavaError, canonical_units};
use lite_model::ParsedModel;

/// The canonical text of a door's answer.
pub fn form(answer: &Result<ParsedModel, JavaError>) -> Vec<u16> {
    // the door's declared ParsedModel fixes the class: no tag
    canonical_units(
        answer
            .as_ref()
            .map(|m| codec::write(m, true).encode_utf16().collect())
            .map_err(Clone::clone),
    )
}

/// `text` at `dialect` through the door, its canonical text.
pub fn model_form(text: &[u16], dialect: Dialect) -> Vec<u16> {
    form(&crate::door::parse(text, dialect))
}
