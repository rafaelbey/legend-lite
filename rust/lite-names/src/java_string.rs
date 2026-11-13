// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's string rules the names layer answers by (2026-11-12): `String.toLowerCase(Locale.ROOT)`.
//!
//! Until 2026-11-13 this module also spelled a protocol type expression by Java's record `toString()`, which
//! `KnowledgeLayer.rawName` put into an association qualified property's refusal (a generic with type-variable values
//! refused, its values' `toString` unported). The names fixes, item 7 (ruling 29), made that refusal name the type's
//! kind in words in Java and here (`knowledge::spelled`), and the spelling went.

/// `String.toLowerCase(Locale.ROOT)` over UTF-16 units (2026-11-12, the names follow-up: a relational function's name
/// lowered before it is matched, `RelationalTypeInference` and `ViewSignatures`): Unicode's full lower-case mapping with
/// its final-sigma rule (`str::to_lowercase`, as the JDK's `ConditionalSpecialCasing`), each run between lone surrogates
/// lowered alone, a lone surrogate kept. The parser's crate holds the same rule privately (lite-front's
/// `tail_emitter::to_lower_root`, held by the front differential); this crate does not reach it.
pub fn to_lower_root(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut Vec<u16>| {
        out.extend(run.to_lowercase().encode_utf16());
        run.clear();
    };
    for r in char::decode_utf16(s.iter().copied()) {
        match r {
            Ok(c) => run.push(c),
            Err(e) => {
                flush(&mut run, &mut out);
                out.push(e.unpaired_surrogate());
            }
        }
    }
    flush(&mut run, &mut out);
    out
}
