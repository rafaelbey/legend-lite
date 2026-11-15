// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `MappingValidation` (MappingValidation.java at ad86fa895): the invalid sets of a mapping BY SET ID, in declaration
//! order, each with the engine's compile-time rejection -- recorded, never thrown here (the driver applies strict /
//! module); a mapping-level error (a duplicated set id across the include closure) throws in both builds. The per-set
//! `catch (ModelException)` keeps the exception (its full message becomes a poison). The switch's `default -> {}`
//! is spelled as the two no-op kinds (census §1).

use std::sync::Arc;

use lite_model::linked::LinkedMap;
use lite_model::{ClassMappingPure, ClassMappingRelational, ClassMappingUnion, JStr, LegacyMappingDefinition};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::resolved_mapping::ResolvedMapping;

/// `MappingValidation.run` (:43): set id -> its `ModelException`, declaration order.
pub fn run(r: &ResolvedMapping<'_>, model: &ModelIndex) -> Result<LinkedMap<JStr, Thrown>, Thrown> {
    Err(Thrown::Unported("MappingValidation.run"))
}

/// `MappingValidation.validateUnionMembers` (:76): every member set is a class mapping of the mapping or its
/// includes ("Can't find class mapping").
pub fn validate_union_members(u: &ClassMappingUnion, r: &ResolvedMapping<'_>) -> Result<(), Thrown> {
    Err(Thrown::Unported("MappingValidation.validateUnionMembers"))
}

/// `MappingValidation.validatePmNames` (:91): every PM names a property its class declares or inherits.
pub fn validate_pm_names(
    rcm: &ClassMappingRelational,
    model: &ModelIndex,
    md: &LegacyMappingDefinition,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("MappingValidation.validatePmNames"))
}

/// `MappingValidation.validateM2mRoutes` (:107): every routed M2M binding names a benign route.
pub fn validate_m2m_routes(
    pcm: &Arc<ClassMappingPure>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("MappingValidation.validateM2mRoutes"))
}
