// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer families 4 and 7 (EF_CENSUS §1.1): the XStore direction check and the XStore / ModelJoin
//! association predicates in column space (`legacyAssocPredicate(a, b, pipeA, pipeB, (srcRow, tgtRow) | cond)`).
//!
//! RE-EXPRESSED (census §4; ruling 30, ASK 8: PARK-1's duplicated rule is ONE Rust function): `synthesizeXStoreMapping`
//! keeps its lines up to the ends' dispatch (:946-982: the association, the ends' set ids, `xstoreEndOf`); its
//! column-space tail (:983-1059) and `XStorePureEnds.synthesize` are the one function
//! [`xstore_pure_ends::synthesize`](crate::xstore_pure_ends::synthesize), parameterized by the reads it rewrites
//! ([`XStoreReads`](crate::xstore_pure_ends::XStoreReads)). The two differ only in the read rewrite, the predicate's
//! two end arguments, and where the empty-lines check sits (with no lines both throw the identical text).
//!
//! `canonicalizeEqualOperands` orders AND/OR operands by Java's record `toString()`, compare-only (never emitted):
//! RE-EXPRESSED with any total order consistent with equality (a derived structural order, or the codec's canonical
//! text) -- the same agree/disagree verdicts (census §4, with its hedge).

use std::sync::Arc;

use lite_front::records as p;
use lite_model::{AssociationMappingCross, AssociationMappingModelJoin, FunctionDefinition, JStr};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

/// `MappingNormalizer.nameRefOrNull` (:706): a `NameRef`'s name, else none. (Ported now: MappingClosures reads it
/// through `AssociationSynthesis.associationOwnerClass`.)
pub fn name_ref_or_null(t: &p::TypeExpression) -> Option<&JStr> {
    match t {
        p::TypeExpression::TypeExpressionNameRef(nr) => Some(&nr.name),
        _ => None,
    }
}

/// `MappingNormalizer.canonicalizeEqualOperands` (:723): the orientation-normal form of an XStore condition -- an
/// `==` whose right side is rooted at `src_var` swapped, AND/OR operands ordered (compare-only; module note).
pub fn canonicalize_equal_operands(v: &p::ValueSpecification, src_var: &JStr) -> p::ValueSpecification {
    // UNPORTED: MappingNormalizer.canonicalizeEqualOperands
    unported!("MappingNormalizer.canonicalizeEqualOperands")
}

/// `MappingNormalizer.rootedAt` (:750): whether `v` reads through the variable `var`.
pub fn rooted_at(v: &p::ValueSpecification, var: &JStr) -> bool {
    // UNPORTED: MappingNormalizer.rootedAt
    unported!("MappingNormalizer.rootedAt")
}

/// `MappingNormalizer.synthesizeXStoreMapping` (:946): an XStore association's predicate function; its ends resolved
/// (`XStorePureEnds.xstoreEndOf`), then the one predicate function, property space for a Pure or lossy end, column
/// space otherwise (module note).
pub fn synthesize_xstore_mapping(
    md: &ResolvedMapping<'_>,
    xs: &Arc<AssociationMappingCross>,
    model: &ModelIndex,
    class_a: &JStr,
    class_b: &JStr,
) -> Result<FunctionDefinition, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.synthesizeXStoreMapping"))
}

/// `MappingNormalizer.synthesizeModelJoinMapping` (:1069): a ModelJoin association's predicate function (the lambda's
/// reads rewritten by `RelationReads.rewrite`, nested hops composed by `ModelJoinNesting.compose`).
pub fn synthesize_model_join_mapping(
    md: &ResolvedMapping<'_>,
    mj: &Arc<AssociationMappingModelJoin>,
    model: &ModelIndex,
    class_a: &JStr,
    class_b: &JStr,
) -> Result<FunctionDefinition, Thrown> {
    Err(Thrown::Unported("MappingNormalizer.synthesizeModelJoinMapping"))
}
