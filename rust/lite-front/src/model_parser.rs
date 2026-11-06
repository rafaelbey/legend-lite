// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The model parser (core/'s `com.legend.parser.PmcdParser` and what it reaches: `ElementParser`,
//! `MappingProtocolParser`, `DatabaseProtocolParser`, the section grammars -- to be ported: docs/rust-port/PHASE2.md §4
//! step 4). Until it is, the door answers a refusal no Java answer has, so its floor starts at zero.

use crate::doors::{Answer, Dialect, JavaError};
use crate::records::PmcdParserParsed;

pub fn parse(_text: &[u16], _dialect: Dialect) -> Answer<PmcdParserParsed> {
    Err(JavaError::of("not ported", Some("the model parser is not ported yet")))
}
