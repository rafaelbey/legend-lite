// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 19 (EF_CENSUS §1.1): instance construction (`new(@C, ^C(...))`), the `trustOne` and
//! `to(get, @Prim)` wrappings of a to-one declared property, and `'true'`/`'false'` CASE literals as Boolean literals.
//! Lite-own emissions with no register row (ASK 12): port as Java has them.

use lite_front::records as p;
use lite_model::JStr;
use lite_names::ModelIndex;

use super::Fields;
use crate::error::Thrown;
use crate::unported;

/// `MappingNormalizer.buildNewInstance` (:2729): `new(@C, ^C(fields))`; loud (a `NullPointerException`, Java's
/// `requireNonNull`) with no target class.
pub fn build_new_instance(class_fqn: Option<&JStr>, fields: &Fields) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.buildNewInstance"))
}

/// `MappingNormalizer.simpleTypeName` (:2762): the name after the last `::`.
pub fn simple_type_name(name: &JStr) -> JStr {
    // UNPORTED: MappingNormalizer.simpleTypeName
    unported!("MappingNormalizer.simpleTypeName")
}

/// `MappingNormalizer.buildNewInstanceToOne` (:2771): [`build_new_instance`] with each to-one declared property's
/// value wrapped (`trustOne`, a primitive row getter's `to(get, @Prim)`, booleanized CASE literals).
pub fn build_new_instance_to_one(
    class_fqn: Option<&JStr>,
    fields: &Fields,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.buildNewInstanceToOne"))
}

/// `MappingNormalizer.booleanizeCaseLiterals` (:2826): `'true'`/`'false'` string literals (in a CASE's branches) as
/// Boolean literals.
pub fn booleanize_case_literals(v: &p::ValueSpecification) -> p::ValueSpecification {
    // UNPORTED: MappingNormalizer.booleanizeCaseLiterals
    unported!("MappingNormalizer.booleanizeCaseLiterals")
}
