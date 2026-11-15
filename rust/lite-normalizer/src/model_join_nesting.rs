// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ModelJoinNesting` (ModelJoinNesting.java at ad86fa895): a ModelJoin association's nested hops -- the ends'
//! composite pipelines and the nested slot reads. RE-EXPRESSED (census §4): MJN:57's `Set<String[]> hops` (arrays
//! hash by identity: it dedups nothing; the real dedup is :66-67) is a list of pairs. The raw NPE (:118) and the
//! `Map.of` duplicate-key IAE (:59, :117) are ASK 11's; since ad86fa895 the end-variable clash is an internal error
//! with its own text (:58).

use std::collections::HashSet;
use std::sync::Arc;

use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{AssociationDefinition, AssociationMappingModelJoin, ClassMappingRelationFunction, JStr};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

/// The nested column reads a ModelJoin's composition records: end variable -> property -> nested property -> slot
/// column (Java's `Map<String, Map<String, Map<String, String>>>`).
pub type NestedCols = LinkedMap<JStr, LinkedMap<JStr, LinkedMap<JStr, JStr>>>;

/// `ModelJoinNesting.Composed` (:38): the ends' composite pipelines and the nested reads.
#[derive(Clone, Debug)]
pub struct Composed {
    pub pipe_a: Option<p::ValueSpecification>,
    pub pipe_b: Option<p::ValueSpecification>,
    pub nested_cols: NestedCols,
}

/// `ModelJoinNesting.compose` (:42): the nested hops of the lambda composed onto the ends' pipelines.
#[allow(clippy::too_many_arguments)]
pub fn compose(
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
    mj: &AssociationMappingModelJoin,
    ad2: &AssociationDefinition,
    class_a: &JStr,
    class_b: &JStr,
    a_var: &JStr,
    b_var: &JStr,
    rf_by_var: &LinkedMap<JStr, Arc<ClassMappingRelationFunction>>,
    pipe_a0: Option<&p::ValueSpecification>,
    pipe_b0: Option<&p::ValueSpecification>,
) -> Result<Composed, Thrown> {
    Err(Thrown::Unported("ModelJoinNesting.compose"))
}

/// `ModelJoinNesting.pairEndVars` (:169): which lambda parameter is end A's and which end B's (Java's `String[2]`).
#[allow(clippy::too_many_arguments)]
pub fn pair_end_vars(
    assoc_name: &JStr,
    ad2: &AssociationDefinition,
    class_a: &JStr,
    class_b: &JStr,
    p0: &p::Variable,
    p1: &p::Variable,
    t0: Option<&JStr>,
    t1: Option<&JStr>,
) -> Result<(JStr, JStr), Thrown> {
    Err(Thrown::Unported("ModelJoinNesting.pairEndVars"))
}

/// `ModelJoinNesting.collectNestedHops` (:196): `(end variable, property)` for each nested hop the expression reads,
/// in reading order (module note).
pub fn collect_nested_hops(v: &p::ValueSpecification, end_vars: &HashSet<JStr>, out: &mut Vec<(JStr, JStr)>) {
    // UNPORTED: ModelJoinNesting.collectNestedHops
    unported!("ModelJoinNesting.collectNestedHops")
}
