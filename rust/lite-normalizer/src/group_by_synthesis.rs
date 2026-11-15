// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `GroupBySynthesis` (GroupBySynthesis.java at ad86fa895): a `~groupBy` set's `groupByComputedKeys` body; the key
//! names (`k0__PRODUCT_ID`) reach SQL. Withholding Join PMs and per-row PMs outside the keys, where the engine
//! accepts the mapping (:158-185), is ASK 12's: port as Java has it.
//!
//! RE-EXPRESSED (census §4): the two copies of the key-naming loop (`groupedKeyColumnNames` :43-76 and
//! `applyGroupBy`'s) are one -- [`grouped_key_column_names`], which `apply_group_by` calls (they differ only where
//! `applyGroupBy` throws, and it runs first on the same set: MN:1744 before :1751).

use std::sync::Arc;

use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{ClassMappingRelational, JStr, PropertyMapping, RelationalOperation, RelationalOperationFunctionCall};

use crate::error::Thrown;
use crate::pipeline::Pipeline;
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

/// `GroupBySynthesis.groupedKeyColumnNames` (:43): each grouped key's column name -> its key name (the one naming
/// loop; module note).
pub fn grouped_key_column_names(rcm: &ClassMappingRelational, main_table: &JStr) -> LinkedMap<JStr, JStr> {
    // UNPORTED: GroupBySynthesis.groupedKeyColumnNames
    unported!("GroupBySynthesis.groupedKeyColumnNames")
}

/// `GroupBySynthesis.renameGroupedNavCond` (:83): a navigation condition over a grouped set's source reads renamed to
/// the key names.
pub fn rename_grouped_nav_cond(
    expr: &p::ValueSpecification,
    grouped_names: &LinkedMap<JStr, JStr>,
    prop_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("GroupBySynthesis.renameGroupedNavCond"))
}

/// `GroupBySynthesis.renameSourceReads` (:105): `src_var`'s column reads renamed to the key names; loud for a read of
/// a column that is no key.
pub fn rename_source_reads(
    v: &p::ValueSpecification,
    src_var: &JStr,
    grouped_names: &LinkedMap<JStr, JStr>,
    prop_name: &JStr,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("GroupBySynthesis.renameSourceReads"))
}

/// `GroupBySynthesis.applyGroupBy` (:132): `source` grouped by the set's keys (`groupByComputedKeys`), the aggregate
/// PMs as its aggregates.
pub fn apply_group_by(
    source: &p::ValueSpecification,
    rcm: &ClassMappingRelational,
    row_bind: &Arc<p::Variable>,
    main_table: &JStr,
    p: &mut Pipeline<'_>,
    md: &ResolvedMapping<'_>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("GroupBySynthesis.applyGroupBy"))
}

/// `GroupBySynthesis.isGroupReducer` (:238): whether a call is an aggregate (a PURE reducer name).
pub fn is_group_reducer(fc: &RelationalOperationFunctionCall) -> bool {
    // UNPORTED: GroupBySynthesis.isGroupReducer
    unported!("GroupBySynthesis.isGroupReducer")
}

/// `GroupBySynthesis.isCollection` (:257): whether a multiplicity's upper bound exceeds one.
pub fn is_collection(m: &p::Multiplicity) -> bool {
    // UNPORTED: GroupBySynthesis.isCollection
    unported!("GroupBySynthesis.isCollection")
}

/// `GroupBySynthesis.isAggregatePm` (:263): whether a PM's expression is an aggregate.
pub fn is_aggregate_pm(pm: &PropertyMapping) -> bool {
    // UNPORTED: GroupBySynthesis.isAggregatePm
    unported!("GroupBySynthesis.isAggregatePm")
}

/// `GroupBySynthesis.pmAsRelationalOp` (:271): a PM's relational expression, if it has one.
pub fn pm_as_relational_op(pm: &PropertyMapping) -> Option<RelationalOperation> {
    // UNPORTED: GroupBySynthesis.pmAsRelationalOp
    unported!("GroupBySynthesis.pmAsRelationalOp")
}

/// `GroupBySynthesis.groupByOpsMatch` (:290): whether two key expressions are one (structurally, table-insensitive).
pub fn group_by_ops_match(a: &RelationalOperation, b: &RelationalOperation) -> bool {
    // UNPORTED: GroupBySynthesis.groupByOpsMatch
    unported!("GroupBySynthesis.groupByOpsMatch")
}

/// `GroupBySynthesis.keyBaseName` (:305): a key expression's base name (its column's), else none.
pub fn key_base_name(op: &RelationalOperation) -> Option<JStr> {
    // UNPORTED: GroupBySynthesis.keyBaseName
    unported!("GroupBySynthesis.keyBaseName")
}
