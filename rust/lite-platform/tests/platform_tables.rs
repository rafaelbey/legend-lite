// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The platform's tables held to Java's (docs/rust-port/PHASE3.md, slice 1b, 2026-11-12): every table of
//! `com.legend.platform`, dumped one fact per line by rust/oracles/builtin/PlatformTables.java (the second file of
//! BuiltinAnswers.java) and frozen in rust/oracles/builtin/platform-tables.tsv, written again here from this crate's
//! tables, section by section in Java's order, and compared line for line: every line identical.

mod tables {
    pub mod platform;
    pub mod support;
}

use tables::support::{Out, compare, java_lines};

#[test]
fn every_platform_table_is_javas() {
    // the catalog, the prelude and the system metamodel parse their declarations on first read: a deep stack, as the
    // parser's doors run on
    let rust = lite_front::doors::on_parse_stack(|| {
        let mut out = Out::default();
        tables::platform::dump(&mut out);
        out.lines
    });
    let java = java_lines("LITE_PLATFORM_TABLES", "../oracles/builtin/platform-tables.tsv");
    compare(&rust, &java, "platform-tables");
}
