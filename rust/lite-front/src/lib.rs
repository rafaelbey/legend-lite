// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's front end (core/'s `com.legend.lexer` and `com.legend.parser`, ported: docs/rust-port/PHASE2.md),
//! over a text's UTF-16 units as Java indexes a `String`. Step 2: the lexer, held to Java's every token stream; step 3:
//! the expression parser (`spec_parser`, over the shared cursor `cursor`), held to Java's every answer.

pub mod codec;
pub mod cursor;
pub mod doors;
pub mod escapes;
pub mod gql;
pub mod jnum;
pub mod jtypes;
pub mod lexer;
pub mod nodes;
pub mod operator_parts;
pub mod records;
pub mod sha256;
pub mod spec_parser;
pub mod temporal;
pub mod ustr;
pub mod wire;

use lexer::TokenStream;

/// A token stream in the one canonical form Java's `FrontTokens.canonical` writes (the differential compares the
/// forms' SHA-256): an `N` line with the count; a `T` line per token (type, start, end, start line, start column, end
/// line, end column); an `H` line per section header (name, name offset, content offset, the residue quoted or `-`,
/// its line and column); an `S` line per skipped section. UTF-16 units, as Java's string.
pub fn canonical_tokens(ts: &TokenStream<'_>) -> Vec<u16> {
    let mut b: Vec<u16> = Vec::new();
    let put = |s: &str, b: &mut Vec<u16>| b.extend(s.encode_utf16());
    put(&format!("N {}\n", ts.count()), &mut b);
    for i in 0..ts.count() {
        put(
            &format!(
                "T {} {} {} {} {} {} {}\n",
                ts.ty(i).name(),
                ts.start(i),
                ts.end(i),
                ts.start_line(i),
                ts.start_column(i),
                ts.end_line(i),
                ts.end_column(i)
            ),
            &mut b,
        );
    }
    for h in &ts.section_headers {
        put("H ", &mut b);
        b.extend_from_slice(&h.name);
        put(&format!(" {} {} ", h.name_offset, h.content_start_offset), &mut b);
        match &h.residue_token {
            None => put("-", &mut b),
            Some(r) => {
                put("'", &mut b);
                b.extend_from_slice(r);
                put("'", &mut b);
            }
        }
        put(&format!(" {} {}\n", h.residue_line, h.residue_col), &mut b);
    }
    for s in &ts.skipped_sections {
        put("S ", &mut b);
        b.extend_from_slice(&s.name);
        put(&format!(" {} {} {}\n", s.name_offset, s.start_offset, s.end_offset), &mut b);
    }
    b
}
