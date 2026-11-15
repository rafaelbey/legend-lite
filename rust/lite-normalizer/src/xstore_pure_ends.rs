// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `XStorePureEnds` (XStorePureEnds.java at ad86fa895): an XStore / ModelJoin association's ENDS (a Relation set, a
//! Relational set's columns, or a Pure set), the property-space predicates and the `legacyLocalProperty` markers;
//! PARK-1's wall ("has direction-specific conditions").
//!
//! RE-EXPRESSED (census §4; ruling 30, ASK 8: PARK-1's rule is ONE Rust function): [`synthesize`] is
//! `XStorePureEnds.synthesize` and `MappingNormalizer.synthesizeXStoreMapping`'s column-space tail (MN:983-1059) as
//! one function, parameterized by [`XStoreReads`]: orientation, the canonical-form comparison and the four texts are
//! one; the reads each condition is rewritten by and the predicate's two end arguments are the parameter's. Not ported
//! (census §3): `XEnd.localTypes` and the `"<relational>"` sentinel of a Relational end's column view (written, never
//! read: the view's `func_ref` may be none).

use std::sync::Arc;

use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{
    AssociationDefinition, AssociationMappingCross, ClassMappingRelationFunction, FunctionDefinition, JStr,
};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::jdk::LinkedSet;
use crate::resolved_mapping::ResolvedMapping;

/// `XStorePureEnds.XEnd` (:61, its never-read `localTypes` dropped): an end's pipeline (none for a Pure end), its
/// column view (a Relation set, or a Relational set's columns as one), its set id, whether it is Pure, its local
/// properties, and whether the column view is lossy (a Relational PM it cannot carry).
#[derive(Clone, Debug)]
pub struct XEnd {
    pub pipeline: Option<p::ValueSpecification>,
    pub cols_view: Option<Arc<ClassMappingRelationFunction>>,
    pub set_id: JStr,
    pub pure: bool,
    pub local_props: LinkedSet<JStr>,
    pub lossy_view: bool,
}

/// The reads the one XStore predicate function rewrites each condition by (module note).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XStoreReads {
    /// `XStorePureEnds.synthesize`'s: property space (`renameReads` over the ends, `this`/`that`), the predicate's ends
    /// by set id (`CString(end.setId)`). For an end that is Pure or a lossy view.
    PropertySpace,
    /// `MappingNormalizer.synthesizeXStoreMapping`'s (:983-1059): column space (`RelationReads.xstore` over each end's
    /// `cols_view`), the predicate's ends by pipeline.
    ColumnSpace,
}

/// `XStorePureEnds.xstoreEndOf` (:72): the end of `class_fqn` (`set_id` when given) in the closure -- a Relation set
/// first, then a Relational set, then a Pure set; loud for none.
pub fn xstore_end_of(
    md: &ResolvedMapping<'_>,
    class_fqn: &JStr,
    set_id: Option<&JStr>,
    model: &ModelIndex,
) -> Result<XEnd, Thrown> {
    Err(Thrown::Unported("XStorePureEnds.xstoreEndOf"))
}

/// `XStorePureEnds.synthesize` (:176), merged with MN:983-1059 (module note): the XStore association's predicate
/// function `(a: A[1], b: B[1]): Boolean[1]`.
#[allow(clippy::too_many_arguments)]
pub fn synthesize(
    md: &ResolvedMapping<'_>,
    xs: &Arc<AssociationMappingCross>,
    ad: &AssociationDefinition,
    class_a: &JStr,
    class_b: &JStr,
    end_a: &XEnd,
    end_b: &XEnd,
    model: &ModelIndex,
    reads: XStoreReads,
) -> Result<FunctionDefinition, Thrown> {
    Err(Thrown::Unported("XStorePureEnds.synthesize"))
}

/// `XStorePureEnds.renameReads` (:268): `this.p` / `that.p` as the rows' property reads (a local property through
/// `legacyLocalProperty`).
pub fn rename_reads(
    v: &p::ValueSpecification,
    row_by_var: &LinkedMap<JStr, Arc<p::Variable>>,
    end_by_var: &LinkedMap<JStr, XEnd>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("XStorePureEnds.renameReads"))
}
