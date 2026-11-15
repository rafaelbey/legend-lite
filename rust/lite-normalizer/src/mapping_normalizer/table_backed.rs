// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 13 (EF_CENSUS §1.1): the table-backed pipeline -- the main row pipeline (hops, union
//! routes, filter, groupBy, distinct, `^C(...)`). The [`Pipeline`](crate::pipeline::Pipeline) made here holds the
//! ledger from then on (`Pipeline::new(src, ledger, md.views())`): reach it through the pipeline. Where Java writes
//! `p.expr = f(p.expr, .., p, ..)`, read `p.expr` first (a cheap `Arc` clone) and assign after: the callee may move
//! the pipeline itself. An unknown filter is skipped at :1720 (loud later, MN:2240); `~primaryKey` is a deliberate
//! no-op.

use std::sync::Arc;

use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{ClassMapping, ClassMappingRelational, JStr, PropertyMapping, RelationalOperation};
use lite_names::ModelIndex;

use super::Fields;
use crate::error::Thrown;
use crate::jdk::LinkedSet;
use crate::mapping_ledger::MappingLedger;
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

/// `MappingNormalizer.RelationalParts` (:1655): a table-backed set's pipeline, row binding and constructor fields,
/// before the `map(row | ^C(...))` that closes them.
#[derive(Clone, Debug)]
pub struct RelationalParts {
    pub pipeline: p::ValueSpecification,
    pub row_bind: Arc<p::Variable>,
    pub fields: Fields,
}

/// `MappingNormalizer.synthTableBackedMapping` (:1659): a table-backed set's body (`source_override` replaces the
/// main table as the pipeline's source: a view's frame, an INNER mapping filter's source).
pub fn synth_table_backed_mapping(
    md: &ResolvedMapping<'_>,
    rcm: &Arc<ClassMappingRelational>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
    source_override: Option<&p::ValueSpecification>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.synthTableBackedMapping"))
}

/// `MappingNormalizer.synthTableBackedParts` (:1677): the pipeline (passes 1-3 of JoinChainEmission, union routes,
/// filter, groupBy, distinct) and the fields; also GroupBySynthesis' and AssociationSynthesis' entry.
pub fn synth_table_backed_parts(
    md: &ResolvedMapping<'_>,
    rcm: &Arc<ClassMappingRelational>,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
    source_override: Option<&p::ValueSpecification>,
) -> Result<RelationalParts, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.synthTableBackedParts"))
}

/// `MappingNormalizer.collectMappedColumns` (:1874): the columns one PM reads, into `sink`; whether it is PLAIN (no
/// join, no splice, no fallback).
pub fn collect_mapped_columns(pm: &PropertyMapping, sink: &mut LinkedSet<JStr>) -> bool {
    // UNPORTED: MappingNormalizer.collectMappedColumns
    unported!("MappingNormalizer.collectMappedColumns")
}

/// `MappingNormalizer.collectExprColumns` (:1910): the columns an expression reads, into `sink`.
pub fn collect_expr_columns(op: &RelationalOperation, sink: &mut LinkedSet<JStr>) {
    // UNPORTED: MappingNormalizer.collectExprColumns
    unported!("MappingNormalizer.collectExprColumns")
}

/// `MappingNormalizer.propertyPinsOf` (:1940): property -> the set ids its routes pin, in PM order then
/// `propertyTargetSets`' (each id once).
pub fn property_pins_of(rcm: &ClassMappingRelational) -> LinkedMap<JStr, Vec<JStr>> {
    // UNPORTED: MappingNormalizer.propertyPinsOf
    unported!("MappingNormalizer.propertyPinsOf")
}

/// `MappingNormalizer.declaredPrimaryKeyColumns` (:1972): a Relational set's `~primaryKey` column names; empty for
/// any other kind.
pub fn declared_primary_key_columns(cm: &ClassMapping) -> Vec<JStr> {
    // UNPORTED: MappingNormalizer.declaredPrimaryKeyColumns
    unported!("MappingNormalizer.declaredPrimaryKeyColumns")
}
