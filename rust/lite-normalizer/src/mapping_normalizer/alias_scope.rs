// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 18 (EF_CENSUS §1.1): the alias scope (which row a bare column reads: the first alias per
//! table wins; a table two sub-row slots reach is marked ambiguous), view references inside join conditions, and a
//! join's target table.

use std::sync::Arc;

use lite_model::{JStr, RelationalOperation};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::pipeline::Pipeline;
use crate::rel_op_translator::TableScope;
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

/// `MappingNormalizer.seedAliasScope` (:2504): each non-class slot's target table bound to `row.<alias>` in `scope`
/// (first alias wins), a non-main table reached by two slots added to `p.ambiguous_tables` instead.
pub fn seed_alias_scope(
    scope: &mut TableScope,
    p: &mut Pipeline<'_>,
    row_bind: &Arc<lite_front::records::Variable>,
    main_table: &JStr,
) {
    // UNPORTED: MappingNormalizer.seedAliasScope
    unported!("MappingNormalizer.seedAliasScope")
}

/// `MappingNormalizer.resolveViewRefsInJoin` (:2535, the seven-argument form): [`resolve_view_refs_in_join_keeping`]
/// with no kept target view, one side.
pub fn resolve_view_refs_in_join(
    op: &RelationalOperation,
    db: &JStr,
    source_table: Option<&JStr>,
    model: &ModelIndex,
    md: Option<&ResolvedMapping<'_>>,
    backing_view: Option<&JStr>,
    only_view: Option<&JStr>,
) -> Result<RelationalOperation, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.resolveViewRefsInJoin"))
}

/// `MappingNormalizer.resolveViewRefsInJoin` (:2550, the nine-argument form): a join condition's references to plain
/// views substituted by the views' columns (`any_side`: on either side of the join).
#[allow(clippy::too_many_arguments)]
pub fn resolve_view_refs_in_join_keeping(
    op: &RelationalOperation,
    db: &JStr,
    source_table: Option<&JStr>,
    model: &ModelIndex,
    md: Option<&ResolvedMapping<'_>>,
    backing_view: Option<&JStr>,
    only_view: Option<&JStr>,
    keep_target_view: Option<&JStr>,
    any_side: bool,
) -> Result<RelationalOperation, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.resolveViewRefsInJoin"))
}

/// `MappingNormalizer.viewChainReaches` (:2637): whether the view chain from `start` (each view's main table) reaches
/// `source_table`.
pub fn view_chain_reaches(
    start: &JStr,
    source_table: Option<&JStr>,
    db: &JStr,
    md: Option<&ResolvedMapping<'_>>,
    model: &ModelIndex,
) -> Result<bool, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.viewChainReaches"))
}

/// `MappingNormalizer.requireNonViewTarget` (:2663): loud when a join's target is a view the frame cannot reach.
pub fn require_non_view_target(
    target_table: &JStr,
    db: &JStr,
    join_name: &JStr,
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("MappingNormalizer.requireNonViewTarget"))
}

/// `MappingNormalizer.determineTargetTable` (:2674): the one non-source table a join condition reads (the source
/// table for a `{target}` marker); loud for none or several.
pub fn determine_target_table(
    cond: &RelationalOperation,
    source_table: Option<&JStr>,
    join_name: &JStr,
    owner_label: Option<&JStr>,
    hop_index: i32,
    mapping_fqn: &JStr,
) -> Result<JStr, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.determineTargetTable"))
}

/// `MappingNormalizer.containsTargetColumnRef` (:2697): whether the condition reads a `{target}` column.
pub fn contains_target_column_ref(op: &RelationalOperation) -> Result<bool, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.containsTargetColumnRef"))
}
