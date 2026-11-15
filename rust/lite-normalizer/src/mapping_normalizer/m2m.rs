// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 8 (EF_CENSUS §1.1): M2M (Pure) sets -- `getAll(Src) -> filter? -> map(src | ^T(...))`.
//! MN:1168's text is in the plan oracle (a poison suffix). `m2mPropertyValue` passes the raw expression through on
//! three misses and strips an uncited `AllVersions` suffix (ASK 12): port as Java has them.

use std::sync::Arc;

use lite_front::records as p;
use lite_model::{ClassDefinition, ClassMappingPure, ClassMappingPurePropertyBinding};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::mapping_ledger::MappingLedger;
use crate::resolved_mapping::ResolvedMapping;

/// `MappingNormalizer.synthM2M` (:1155): a Pure set's body.
pub fn synth_m2m(
    md: &ResolvedMapping<'_>,
    pcm: &Arc<ClassMappingPure>,
    model: &ModelIndex,
    ledger: &MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.synthM2M"))
}

/// `MappingNormalizer.m2mPropertyValue` (:1229): one binding's value -- the expression, or a `NewInstanceCast` to a
/// mapped class-typed property (loud for an unmapped one).
pub fn m2m_property_value(
    pb: &Arc<ClassMappingPurePropertyBinding>,
    tgt: Option<&Arc<ClassDefinition>>,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
    ledger: &MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.m2mPropertyValue"))
}
