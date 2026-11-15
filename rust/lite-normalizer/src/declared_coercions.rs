// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `DeclaredCoercions` (DeclaredCoercions.java at ad86fa895): `castAsDeclared` / `typeAsDeclared` / `parse*` around
//! each mapped value, as its declared property type asks (SQL and result typing). RE-EXPRESSED (census §4): the
//! unused `ledger` parameters are dropped, and the javadoc's `RequiredNullableCensus` hook (a deleted class) is not
//! ported. `declaredAssertion` does not see through views where `coerceColumnToDeclared` does, and a declared-Boolean
//! string errors in SQL where the engine's `Boolean.parseBoolean` yields false ("DELIBERATE divergence", :47-51): ASK
//! 12 and 13's, ported as Java has them.

use lite_front::records as p;
use lite_model::{JStr, PropertyMappingColumn, PropertyMappingJoinTerminalColumn};
use lite_names::ModelIndex;

use crate::error::Thrown;

/// `DeclaredCoercions.coerceToDeclaredNumeric` (:53): a numeric value conformed to its declared number type.
pub fn coerce_to_declared_numeric(
    value: &p::ValueSpecification,
    prop_name: &JStr,
    owner_class_fqn: Option<&JStr>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("DeclaredCoercions.coerceToDeclaredNumeric"))
}

/// `DeclaredCoercions.declaredPlatformKind` (:75): the property's declared platform type's kind, else none.
pub fn declared_platform_kind(
    prop_name: &JStr,
    owner_class_fqn: Option<&JStr>,
    model: &ModelIndex,
) -> Result<Option<JStr>, Thrown> {
    Err(Thrown::Unported("DeclaredCoercions.declaredPlatformKind"))
}

/// `DeclaredCoercions.declaredAssertion` (:99, its `ledger` dropped): a join-terminal column's read asserted as its
/// declared type.
pub fn declared_assertion(
    read: &p::ValueSpecification,
    jtc: &PropertyMappingJoinTerminalColumn,
    owner_class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("DeclaredCoercions.declaredAssertion"))
}

/// `DeclaredCoercions.coerceColumnToDeclared` (:133, its `ledger` dropped): a column's read conformed to its declared
/// type (through a view's column to its physical one).
pub fn coerce_column_to_declared(
    read: &p::ValueSpecification,
    col: &PropertyMappingColumn,
    owner_class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("DeclaredCoercions.coerceColumnToDeclared"))
}
