// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! legend-lite's front end (core/'s `com.legend.lexer` and `com.legend.parser`, ported: docs/rust-port/PHASE2.md),
//! over a text's UTF-16 units as Java indexes a `String`. Step 2: the lexer, held to Java's every token stream; step 3:
//! the expression parser (`spec_parser`, over the shared cursor `cursor`), held to Java's every answer; step 4a: the
//! model parser's walk (`model_parser`) and its `###Pure` (`element_parser`), `###Text` (`sections`) and `###Data`
//! (`mapping_parser`) grammars; step 4b: `###Relational` (`database_parser`) and `###Mapping` (`mapping_grammar`,
//! `mapping_tests`); step 4c: `###Connection` (`connection_parser`, with `java.net.URI`'s verdict in `jnet`),
//! `###Runtime` (`runtime_parser`), `###Service` (`service_parser`) and `###DataSpace` (`dataspace_parser`); step 4d: the
//! last section grammars -- the function activators (`activator_parser`), `###Diagram` (`diagram_parser`), and the tail
//! grammars `generation_parser`, `deephaven_parser`, `mongodb_parser`, `data_quality_parser`, `elasticsearch_parser`,
//! `external_format_parser`, `service_store_parser`, `persistence_parser` and `query_post_processor_parser` -- so that
//! every section the walk reaches has its grammar, each held to Java's every answer; step 5: the protocol emitter
//! (core/'s `com.legend.protocol.ProtocolEmitter` and its family: `emitter`, `tail_emitter`, `mapping_emitter`,
//! `connection_emitters`, `gql_emitter`) and the doors' span cut (`source_information`), legend-engine's JSON written
//! from these records, held to Java's every answer at the two engine-JSON doors (`doors::document`,
//! `doors::lambda_json`); step 6a: `probes`, what `//parser-equivalence`'s gate tests asked of the Java lexer's and
//! parser's internals (section headers, the strict element sites, the Database sites, the known sections).

// 2026-11-11 (docs/rust-port/PHASE3_CRATES.md §3; the exemption lifted, docs/rust-port/PHASE3.md step 1): no unsafe
// code but lite-base's arena and lite-ffi's boundary
#![forbid(unsafe_code)]

pub mod activator_parser;
pub mod codec;
pub mod connection_emitters;
pub mod connection_parser;
pub mod cursor;
pub mod data_quality_parser;
pub mod database_parser;
pub mod dataspace_parser;
pub mod deephaven_parser;
pub mod diagram_parser;
pub mod doors;
pub mod elasticsearch_parser;
pub mod element_parser;
pub mod emitter;
pub mod escapes;
pub mod external_format_parser;
pub mod generation_parser;
pub mod gql;
pub mod gql_emitter;
pub mod jnet;
pub mod jnum;
pub mod jtypes;
pub mod lexer;
pub mod mapping_emitter;
pub mod mapping_grammar;
pub mod mapping_parser;
pub mod mapping_tests;
pub mod model_parser;
pub mod mongodb_parser;
pub mod nodes;
pub mod operator_parts;
pub mod persistence_parser;
pub mod probes;
pub mod protocol;
pub mod query_post_processor_parser;
#[rustfmt::skip] // generated, and held byte for byte to its generator: //rust/lite-front:update_generated_test
pub mod records;
pub mod runtime_parser;
pub mod sections;
pub mod service_parser;
pub mod service_store_parser;
pub mod sha256;
pub mod source_information;
pub mod spec_parser;
pub mod tail_emitter;
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
        put(
            &format!(" {} {} {}\n", s.name_offset, s.start_offset, s.end_offset),
            &mut b,
        );
    }
    b
}
