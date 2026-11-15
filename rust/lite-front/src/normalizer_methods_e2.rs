// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! A protocol record's method the normalizer calls that no crate had yet (2026-11-14, the normalizer slice's fill,
//! `ModelNormalizer`'s E.3 and E.3c): `ConstraintDefinition.expression()`, ported beside the record as Java has it.

use crate::doors::JavaError;
use crate::jtypes::JStr;
use crate::records::{ConstraintDefinition, Realization, ValueSpecification};

const ILLEGAL_STATE: &str = "java.lang.IllegalStateException";

impl ConstraintDefinition {
    /// `expression()`: the inline predicate (the sugar form) -- an inline body of exactly one statement; otherwise
    /// Java's `IllegalStateException` (a Door-4 function-ref binding has none, nor has an inline body of several
    /// statements).
    pub fn expression(&self) -> Result<&ValueSpecification, JavaError> {
        if let Realization::RealizationInline(inl) = &self.realization
            && inl.body.len() == 1
        {
            return Ok(&inl.body[0]);
        }
        let mut message: Vec<u16> = "constraint '".encode_utf16().collect();
        message.extend_from_slice(&self.name.0);
        message.extend("' is a function-ref binding, not an inline predicate".encode_utf16());
        Err(JavaError::java(ILLEGAL_STATE, JStr(message)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{CString, RealizationInline, RealizationRef};
    use std::sync::Arc;

    fn constraint(realization: Realization) -> ConstraintDefinition {
        ConstraintDefinition {
            name: JStr::from("c0"),
            realization,
            message: None,
            enforcement_level: None,
            external_id: None,
            owner: None,
            pos: None,
        }
    }

    fn statement() -> ValueSpecification {
        ValueSpecification::CString(Arc::new(CString {
            value: JStr::from("x"),
            pos: None,
            multi_line: false,
        }))
    }

    #[test]
    fn the_expression_is_the_one_inline_statement_else_javas_illegal_state() {
        let one = constraint(Realization::RealizationInline(Box::new(RealizationInline {
            body: vec![statement()],
        })));
        assert_eq!(one.expression(), Ok(&statement()));
        let refused = Err(JavaError::java(
            ILLEGAL_STATE,
            JStr::from("constraint 'c0' is a function-ref binding, not an inline predicate"),
        ));
        let two = constraint(Realization::RealizationInline(Box::new(RealizationInline {
            body: vec![statement(), statement()],
        })));
        assert_eq!(two.expression(), refused.clone());
        let by_ref = constraint(Realization::RealizationRef(Box::new(RealizationRef {
            function_fqn: JStr::from("a::f"),
            source: None,
        })));
        assert_eq!(by_ref.expression(), refused);
    }
}
