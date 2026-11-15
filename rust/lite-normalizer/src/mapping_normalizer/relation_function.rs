// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 6 (EF_CENSUS §1.1): Relation (`~func`) sets -- `<fn>() -> map(row | ^C(...))`. Reads
//! lite-model's `ClassMappingRelationFunctionCol::is_embedded` and `ClassMappingRelationFunctionCol::bind_src`.

use std::sync::Arc;

use lite_front::records as p;
use lite_model::{ClassMappingRelationFunction, ClassMappingRelationFunctionCol, JStr};
use lite_names::ModelIndex;

use super::Fields;
use crate::error::Thrown;
use crate::resolved_mapping::ResolvedMapping;

/// `MappingNormalizer.synthRelationFunction` (:817): a Relation set's body.
pub fn synth_relation_function(
    md: &ResolvedMapping<'_>,
    rf: &Arc<ClassMappingRelationFunction>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.synthRelationFunction"))
}

/// `MappingNormalizer.putRelationCols` (:833): bindings for a Relation mapping's column list into `fields` (EMBEDDED
/// blocks and inline-embedded sibling sets nested, locals skipped).
pub fn put_relation_cols(
    fields: &mut Fields,
    cols: &[Arc<ClassMappingRelationFunctionCol>],
    row: &Arc<p::Variable>,
    owner_class_fqn: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("MappingNormalizer.putRelationCols"))
}

/// `MappingNormalizer.relationFunctionPipeline` (:908): the Relation set's source -- its inline source, or the one
/// zero-arg single-expression function `~func` names.
pub fn relation_function_pipeline(
    rf: &ClassMappingRelationFunction,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.relationFunctionPipeline"))
}
