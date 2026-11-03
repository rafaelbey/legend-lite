// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Java spellings the port must keep (docs/rust-port/TRIAGE.md, "B. Spelling that is upstream spec"):
//! legend-engine is Java, so the SQL literals, plan JSON and protocol JSON the corpora compare against carry the JDK's
//! own text for numbers, and the Rust compiler writes the same. Each spelling is held to the JDK's answers frozen
//! under rust/oracles/ (`tests/`).

mod double;

pub use double::double_to_string;
