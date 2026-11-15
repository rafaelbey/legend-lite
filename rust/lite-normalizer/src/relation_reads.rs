// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `RelationReads` (RelationReads.java at ad86fa895): XStore / ModelJoin predicate lambdas' reads rewritten to the
//! ends' rows -- a Relation column inlined (`ClassMappingRelationFunctionCol::bind_src`, lite-model's), a derived
//! property beta-reduced to depth 16. `bindSrc` and `substVars` ignore lambda shadowing (as Java's do). Not ported
//! (census §3): the 6-argument `rewrite` (:49-56, no caller) and the `model == null` branches (both live callers pass
//! a model): `model` is not optional here.

use std::sync::Arc;

use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{ClassMappingRelationFunction, JStr};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::model_join_nesting::NestedCols;
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

/// `RelationReads.xstore` (:36): an XStore line's condition over `this` and `that` as the rows' column reads.
#[allow(clippy::too_many_arguments)]
pub fn xstore(
    v: &p::ValueSpecification,
    this_row: &Arc<p::Variable>,
    this_rf: Option<&Arc<ClassMappingRelationFunction>>,
    that_row: &Arc<p::Variable>,
    that_rf: Option<&Arc<ClassMappingRelationFunction>>,
    assoc_name: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("RelationReads.xstore"))
}

/// `RelationReads.rewrite` (:58, the seven-argument form): [`rewrite_at`] from depth 0.
pub fn rewrite(
    v: &p::ValueSpecification,
    row_by_var: &LinkedMap<JStr, Arc<p::Variable>>,
    rf_by_var: &LinkedMap<JStr, Arc<ClassMappingRelationFunction>>,
    assoc_name: &JStr,
    md: &ResolvedMapping<'_>,
    nested_cols: Option<&NestedCols>,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("RelationReads.rewrite"))
}

/// `RelationReads.rewrite` (:71, the eight-argument form): the reads rewritten, a derived property inlined up to
/// `derived_depth` 16.
#[allow(clippy::too_many_arguments)]
pub fn rewrite_at(
    v: &p::ValueSpecification,
    row_by_var: &LinkedMap<JStr, Arc<p::Variable>>,
    rf_by_var: &LinkedMap<JStr, Arc<ClassMappingRelationFunction>>,
    assoc_name: &JStr,
    md: &ResolvedMapping<'_>,
    nested_cols: Option<&NestedCols>,
    model: &ModelIndex,
    derived_depth: i32,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("RelationReads.rewrite"))
}

/// `RelationReads.substVars` (:184): the variables `binds` names substituted (no shadowing rule, as Java's).
pub fn subst_vars(v: &p::ValueSpecification, binds: &LinkedMap<JStr, p::ValueSpecification>) -> p::ValueSpecification {
    // UNPORTED: RelationReads.substVars
    unported!("RelationReads.substVars")
}
