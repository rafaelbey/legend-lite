// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 14 (EF_CENSUS §1.1): each PM to a constructor field (`^C(p = ...)`), and the embedded,
//! otherwise-embedded and inline-embedded materializations. `pipeline.inline_stack`'s order is printed in a splice
//! cycle's error.

use std::sync::Arc;

use lite_front::records as p;
use lite_model::{JStr, PropertyMapping, PropertyMappingInlineEmbedded, PropertyMappingOtherwiseEmbedded};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::pipeline::Pipeline;
use crate::rel_op_translator::TableScope;
use crate::resolved_mapping::ResolvedMapping;

/// `MappingNormalizer.CtorField` (:1999): one constructor field -- its name, its value, and whether it is a
/// mapping-local property.
#[derive(Clone, Debug)]
pub struct CtorField {
    pub name: JStr,
    pub value: p::ValueSpecification,
    pub is_local: bool,
}

/// `MappingNormalizer.translatePmToField` (:2001): one PM's constructor field over the row.
#[allow(clippy::too_many_arguments)]
pub fn translate_pm_to_field(
    pm: &PropertyMapping,
    row_bind: &Arc<p::Variable>,
    table_scope: &TableScope,
    default_table: &JStr,
    pipeline: &mut Pipeline<'_>,
    owner_class_fqn: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
    under_group_by: bool,
) -> Result<CtorField, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.translatePmToField"))
}

/// `MappingNormalizer.materializeEmbedded` (:2100): an embedded block's `^C(...)` over the same row (`inner_override`
/// the inner class when the caller knows it).
#[allow(clippy::too_many_arguments)]
pub fn materialize_embedded(
    prop_name: &JStr,
    sub_pms: &[PropertyMapping],
    row_bind: &Arc<p::Variable>,
    table_scope: &TableScope,
    default_table: &JStr,
    pipeline: &mut Pipeline<'_>,
    owner_class_fqn: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
    inner_override: Option<&JStr>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.materializeEmbedded"))
}

/// `MappingNormalizer.materializeOtherwiseEmbedded` (:2150): an otherwise-embedded block (the embedded value, the
/// fallback set's navigation otherwise).
#[allow(clippy::too_many_arguments)]
pub fn materialize_otherwise_embedded(
    oe: &Arc<PropertyMappingOtherwiseEmbedded>,
    row_bind: &Arc<p::Variable>,
    table_scope: &TableScope,
    default_table: &JStr,
    pipeline: &mut Pipeline<'_>,
    owner_class_fqn: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.materializeOtherwiseEmbedded"))
}

/// `MappingNormalizer.materializeInlineEmbedded` (:2164): an inline-embedded splice of another set of the closure;
/// loud for a set already on `pipeline.inline_stack` (the one cycle an embedded materialization can have).
#[allow(clippy::too_many_arguments)]
pub fn materialize_inline_embedded(
    ie: &Arc<PropertyMappingInlineEmbedded>,
    row_bind: &Arc<p::Variable>,
    table_scope: &TableScope,
    default_table: &JStr,
    pipeline: &mut Pipeline<'_>,
    owner_class_fqn: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.materializeInlineEmbedded"))
}
