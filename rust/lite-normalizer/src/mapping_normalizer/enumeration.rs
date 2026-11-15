// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 17 (EF_CENSUS §1.1): the enumeration decode -- a nested `if(equal(...))` per source value
//! (an SQL CASE). An inline "NEVER fired" internal error (:2422) stays (as [`Thrown::never_fired`]'s sibling, its text
//! exactly); an anonymous reference prints `EnumerationMapping 'null'` (ASK 13); an id lookup takes the first match
//! across includes where the anonymous path is loud on two: port as Java has them.
//!
//! [`Thrown::never_fired`]: crate::error::Thrown::never_fired

use std::sync::Arc;

use lite_front::records as p;
use lite_model::{JStr, PropertyMappingEnumeratedColumn};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::pipeline::Pipeline;
use crate::rel_op_translator::TableScope;
use crate::resolved_mapping::ResolvedMapping;

/// `MappingNormalizer.translateEnumeratedColumn` (:2386): the column read (`p` none: `PipelineView.NONE`), decoded.
#[allow(clippy::too_many_arguments)]
pub fn translate_enumerated_column(
    ec: &Arc<PropertyMappingEnumeratedColumn>,
    table_scope: &TableScope,
    default_table: &JStr,
    md: &ResolvedMapping<'_>,
    p: Option<&Pipeline<'_>>,
    owner_class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.translateEnumeratedColumn"))
}

/// `MappingNormalizer.translateEnumeratedSource` (:2403): `source_read` decoded through the enumeration mapping
/// `enum_mapping_id` names (or the property's enum's one mapping).
pub fn translate_enumerated_source(
    property_name: &JStr,
    enum_mapping_id: Option<&JStr>,
    source_read: &p::ValueSpecification,
    md: &ResolvedMapping<'_>,
    owner_class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.translateEnumeratedSource"))
}
