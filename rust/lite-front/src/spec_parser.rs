// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The expression parser (core/'s `com.legend.parser.SpecParser`, to be ported: docs/rust-port/PHASE2.md §4 step 3).
//! Until it is, every door answers a refusal no Java answer has, so the differential's floors start at zero.

use crate::doors::{Answer, Dialect, JavaError};
use crate::records::{LambdaFunction, ValueSpecification};

const NOT_PORTED: &str = "the expression parser is not ported yet";

pub fn parse(_text: &[u16], _dialect: Dialect) -> Answer<ValueSpecification> {
    Err(JavaError::of("not ported", Some(NOT_PORTED)))
}

pub fn parse_lambda(_text: &[u16]) -> Answer<LambdaFunction> {
    Err(JavaError::of("not ported", Some(NOT_PORTED)))
}

pub fn parse_code_block(_text: &[u16], _dialect: Dialect) -> Answer<Vec<ValueSpecification>> {
    Err(JavaError::of("not ported", Some(NOT_PORTED)))
}
