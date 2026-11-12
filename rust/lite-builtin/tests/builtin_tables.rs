// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The platform's tables held to Java's (docs/rust-port/PHASE3.md, slice 1b, 2026-11-12): every table of
//! `com.legend.builtin`, dumped one fact per line by `core/src/test/java/com/legend/frontoracle/BuiltinAnswers.java`
//! and frozen in rust/oracles/builtin/builtin-tables.tsv (its MANIFEST), written again here from this crate's tables,
//! section by section in Java's order, and compared line for line: every line identical.

mod tables {
    pub mod metamodel;
    pub mod native_fn;
    pub mod pure;
    pub mod support;
}

use tables::support::{Out, compare, java_lines};

#[test]
fn every_builtin_table_is_javas() {
    // the catalog parses its declarations on first read: a deep stack, as the parser's doors run on
    let rust = lite_front::doors::on_parse_stack(|| {
        let mut out = Out::default();
        tables::pure::dump(&mut out);
        tables::native_fn::dump(&mut out);
        tables::metamodel::dump(&mut out);
        out.lines
    });
    let java = java_lines("LITE_BUILTIN_TABLES", "../oracles/builtin/builtin-tables.tsv");
    compare(&rust, &java, "builtin-tables");
}
