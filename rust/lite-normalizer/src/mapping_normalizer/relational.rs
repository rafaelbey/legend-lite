// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer families 10, 11 and 12 (EF_CENSUS §1.1): the relational dispatch (JSON / view / table), the
//! main-table inference and the `RelationalSource` stamp; JSON-source sets; view-backed sets (the view frame as the
//! pipeline's source, `md.views().body(view)`).
//!
//! RE-EXPRESSED (census §4): `inferMainTableQuiet`'s catch-as-probe is [`infer_main_table`] answering
//! `Result<Option<_>>` and [`infer_main_table_quiet`] mapping a `ModelException` (only) to `None`; anything else
//! (an unported body included) still propagates, so the quiet form is a `Result` too.

use std::sync::Arc;

use lite_front::records as p;
use lite_model::{
    ClassMappingRelational, DatabaseDefinitionViewDefinition, JStr, LegacyMappingDefinitionTableReference as TableRef,
    MappingDefinitionRelationalSource, PropertyMapping, RelationalOperation,
};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::mapping_ledger::MappingLedger;
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

/// `MappingNormalizer.synthRelational` (:1327): a Relational set's body -- JSON source, view-backed, or table-backed
/// (a set with no `~mainTable` takes its inferred one; loud when there is none).
pub fn synth_relational(
    md: &ResolvedMapping<'_>,
    rcm: &Arc<ClassMappingRelational>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.synthRelational"))
}

/// `MappingNormalizer.resolvedMainTable` (:1372): the set's main table, declared or inferred; none for a JSON source.
pub fn resolved_main_table(rcm: &ClassMappingRelational) -> Result<Option<Arc<TableRef>>, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.resolvedMainTable"))
}

/// `MappingNormalizer.relationalSourceOf` (:1387): the binding's `RelationalSource` stamp (JSON, or the table with
/// `aggregationAwareMain` and the enumerated columns); an internal error for a set whose synthesis must have walled.
pub fn relational_source_of(rcm: &ClassMappingRelational) -> Result<MappingDefinitionRelationalSource, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.relationalSourceOf"))
}

/// `MappingNormalizer.inferMainTable` (:1419): the one table the PMs read; none when they read none; loud for two
/// databases or two tables.
pub fn infer_main_table(rcm: &ClassMappingRelational) -> Result<Option<Arc<TableRef>>, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.inferMainTable"))
}

/// `MappingNormalizer.canonicalTable` (:1463): `KnowledgeLayer.canonicalTable` (lite-names').
pub fn canonical_table(table: &JStr) -> JStr {
    // UNPORTED: MappingNormalizer.canonicalTable
    unported!("MappingNormalizer.canonicalTable")
}

/// `MappingNormalizer.inferMainTableQuiet` (:1468): [`infer_main_table`] with a `ModelException` answered as none
/// (the module's note).
pub fn infer_main_table_quiet(rcm: &ClassMappingRelational) -> Result<Option<Arc<TableRef>>, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.inferMainTableQuiet"))
}

/// `MappingNormalizer.collectMainTables` (:1477): the tables one PM reads, into `sink` in read order.
pub fn collect_main_tables(pm: &PropertyMapping, sink: &mut Vec<Arc<TableRef>>) {
    // UNPORTED: MappingNormalizer.collectMainTables
    unported!("MappingNormalizer.collectMainTables")
}

/// `MappingNormalizer.collectExprTables` (:1509): the tables an expression reads, into `sink`.
pub fn collect_expr_tables(op: &RelationalOperation, sink: &mut Vec<Arc<TableRef>>) {
    // UNPORTED: MappingNormalizer.collectExprTables
    unported!("MappingNormalizer.collectExprTables")
}

/// `MappingNormalizer.synthJsonSourceMapping` (:1565): `sourceUrl(url) -> map(row | ^C(p = to(get(...))))`.
pub fn synth_json_source_mapping(
    md: &ResolvedMapping<'_>,
    rcm: &Arc<ClassMappingRelational>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.synthJsonSourceMapping"))
}

/// `MappingNormalizer.synthViewBackedMapping` (:1625): a set on a view -- the view's lifted body as the source, the
/// PMs re-pointed through the frame (`ViewRelation.throughFrame`).
pub fn synth_view_backed_mapping(
    md: &ResolvedMapping<'_>,
    rcm: &Arc<ClassMappingRelational>,
    view: &Arc<DatabaseDefinitionViewDefinition>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.synthViewBackedMapping"))
}
