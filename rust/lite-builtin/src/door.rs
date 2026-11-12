// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The text's door (`com.legend.door.ModelFromText`, merged into this crate: docs/rust-port/PHASE3_CRATES.md §1): a
//! text as the compiler's model -- the one parser reads it into its records (rust/lite-front's model door, at the
//! dialect the caller names), and the model's one door (`lite_model::model_from_protocol`) makes them its model, the
//! door a model given as records goes through too. A shape the model cannot represent is the text's error, at the
//! element.

use std::sync::Arc;

use lite_front::doors::{self, Dialect, JavaError};
use lite_front::jtypes::JStr;
use lite_front::records as p;
use lite_model::error::{ILLEGAL_STATE, ModelError, Unrepresentable};
use lite_model::linked::list;
use lite_model::{PackageableElement, ParsedModel, ParsedModelUnclaimedSection};

/// `ModelFromText.parse(source, dialect)`: the one parse entry. The caller names the dialect; there is no default.
/// No overlay grammar is registered here (a host's overlays are the shim's: `PmcdParser.parse`).
pub fn parse(source: &[u16], dialect: Dialect) -> Result<ParsedModel, JavaError> {
    let parsed = doors::pmcd(source, dialect, &[]).parsed?;
    of_parsed(&parsed, source)
}

/// The model of a model door's records parsed from `source`: the unclaimed sections kept, a shape the model cannot
/// represent the text's parse error.
pub fn of_parsed(parsed: &p::PmcdParserParsed, source: &[u16]) -> Result<ParsedModel, JavaError> {
    let unclaimed = list(
        parsed
            .unclaimed
            .iter()
            .map(|u| {
                Arc::new(ParsedModelUnclaimedSection {
                    name: u.name.clone(),
                    start_offset: u.start_offset,
                    end_offset: u.end_offset,
                })
            })
            .collect(),
    );
    let text = JStr(source.to_vec());
    lite_model::model_from_protocol::of_text(&parsed.model, Some(&text), unclaimed).map_err(parse_error)
}

/// `ModelFromText.element(parsed)`: one element's record as the model.
pub fn element(parsed: &p::Element) -> Result<PackageableElement, JavaError> {
    lite_model::model_from_protocol::element(parsed).map_err(parse_error)
}

/// A shape the model cannot represent is the text's error at the element; one parsed without a span is the parser's
/// bug, never placed at 1:1.
fn parse_error(e: ModelError) -> JavaError {
    match e {
        ModelError::Java(j) => j,
        ModelError::Unrepresentable(Unrepresentable { path, reason, at }) => match at {
            Some(at) => JavaError::parse(reason, at.line, at.column),
            None => JavaError::java(
                ILLEGAL_STATE,
                lite_model::cat!(path, " was parsed without a span: ", reason),
            ),
        },
    }
}
