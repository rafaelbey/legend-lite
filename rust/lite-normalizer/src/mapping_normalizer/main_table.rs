// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 16 (EF_CENSUS §1.1): the closure's main-table lookups (the table an association anchors
//! on). `mainTableOrNull` skips an ambiguous root set and answers a non-root set's table (ASK 12): port as Java has it.

use std::sync::Arc;

use lite_model::{JStr, LegacyMappingDefinitionTableReference as TableRef};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::resolved_mapping::ResolvedMapping;

/// `MappingNormalizer.hasMainTable` (:2333).
pub fn has_main_table(md: &ResolvedMapping<'_>, class_fqn: &JStr, model: &ModelIndex) -> Result<bool, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.hasMainTable"))
}

/// `MappingNormalizer.mainTableOrNull` (:2344): the root set's main table across the closure, else the first set's
/// with one; none when no set has one (`inferMainTableQuiet` for a set with no `~mainTable`).
pub fn main_table_or_null(md: &ResolvedMapping<'_>, class_fqn: Option<&JStr>) -> Result<Option<Arc<TableRef>>, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.mainTableOrNull"))
}

/// `MappingNormalizer.mainTableDefOf` (:2364): [`main_table_or_null`], loud when none (JoinChainEmission:884's one
/// cause, RE-EXPRESSED there as `main_table_or_null(..)?.is_none()`).
pub fn main_table_def_of(
    md: &ResolvedMapping<'_>,
    class_fqn: Option<&JStr>,
    model: &ModelIndex,
) -> Result<Arc<TableRef>, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.mainTableDefOf"))
}

/// `MappingNormalizer.mainTableOf` (:2376): [`main_table_def_of`]'s table.
pub fn main_table_of(md: &ResolvedMapping<'_>, class_fqn: Option<&JStr>, model: &ModelIndex) -> Result<JStr, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.mainTableOf"))
}
