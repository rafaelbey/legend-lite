// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 15 (EF_CENSUS §1.1): `~filter` -> `filter(row | cond)`, a direct filter or one mediated
//! by a join chain (its terminal slot read through the pipeline).

use std::sync::Arc;

use lite_front::records as p;
use lite_model::{ClassMappingRelational, FilterMappingDirect, FilterMappingJoinMediated, JStr};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::pipeline::Pipeline;
use crate::resolved_mapping::ResolvedMapping;

/// `MappingNormalizer.applyFilter` (:2240): `source` filtered by the set's `~filter`.
#[allow(clippy::too_many_arguments)]
pub fn apply_filter(
    source: &p::ValueSpecification,
    rcm: &ClassMappingRelational,
    row_bind: &Arc<p::Variable>,
    main_db: &JStr,
    main_table: &JStr,
    p: &mut Pipeline<'_>,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.applyFilter"))
}

/// `MappingNormalizer.applyDirectFilter` (:2256).
#[allow(clippy::too_many_arguments)]
pub fn apply_direct_filter(
    source: &p::ValueSpecification,
    rcm: &ClassMappingRelational,
    row_bind: &Arc<p::Variable>,
    main_db: &JStr,
    main_table: &JStr,
    p: &mut Pipeline<'_>,
    model: &ModelIndex,
    direct: &Arc<FilterMappingDirect>,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.applyDirectFilter"))
}

/// `MappingNormalizer.applyJoinMediatedFilter` (:2286).
#[allow(clippy::too_many_arguments)]
pub fn apply_join_mediated_filter(
    source: &p::ValueSpecification,
    rcm: &ClassMappingRelational,
    row_bind: &Arc<p::Variable>,
    main_db: &JStr,
    main_table: &JStr,
    p: &mut Pipeline<'_>,
    model: &ModelIndex,
    jm: &Arc<FilterMappingJoinMediated>,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.applyJoinMediatedFilter"))
}
