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
//! `canonicalizeEqualOperands` orders AND/OR operands, compare-only (never emitted), by the text of the components
//! Java's `equals` reads ([`p::ValueSpecification::equality_key`], lite-front's), whose ties are Java's equality: any
//! total order consistent with equality gives one agree/disagree verdict, two lines' conditions equal modulo the
//! commutation of AND/OR -- the comparison Java makes directly since 2026-11-15 (`MappingNormalizer
//! .sameModuloCommutation`; the normalizer batch, item 11, S394), where it ordered by `toString()`, source positions
//! included (census §4's hedge).

use std::sync::Arc;

use lite_builtin::natives::{AND__BOOLEAN_1__BOOLEAN_1, EQUAL__ANY_MANY__ANY_MANY, OR__BOOLEAN_1__BOOLEAN_1};
use lite_builtin::pure::{NativeId, lite};
use lite_error::Phase;
use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{AssociationMappingCross, AssociationMappingModelJoin, FunctionDefinition, JStr, cat};
use lite_names::{ModelIndex, resolved_names};

use crate::error::{Thrown, require};
use crate::model_join_nesting;
use crate::relation_reads;
use crate::resolved_mapping::ResolvedMapping;
use crate::xstore_pure_ends::{self, XStoreReads};

/// `MappingNormalizer.nameRefOrNull` (:706): a `NameRef`'s name, else none. (Ported now: MappingClosures reads it
/// through `AssociationSynthesis.associationOwnerClass`.)
pub fn name_ref_or_null(t: &p::TypeExpression) -> Option<&JStr> {
    match t {
        p::TypeExpression::TypeExpressionNameRef(nr) => Some(&nr.name),
        _ => None,
    }
}

/// `ResolvedNames.names(af, n.qualifiedName())` for a catalog native.
fn names_native(af: &p::AppliedFunction, n: NativeId) -> bool {
    resolved_names::names(af, &n.def().qualified_name.lossy())
}

/// `MappingNormalizer.canonicalizeEqualOperands` (:723): the orientation-normal form of an XStore condition -- an
/// `==` whose right side is rooted at `src_var` swapped, AND/OR operands ordered (compare-only; module note).
pub fn canonicalize_equal_operands(v: &p::ValueSpecification, src_var: &JStr) -> p::ValueSpecification {
    let p::ValueSpecification::AppliedFunction(af) = v else {
        return v.clone();
    };
    let mut ps: Vec<p::ValueSpecification> = af
        .parameters
        .iter()
        .map(|x| canonicalize_equal_operands(x, src_var))
        .collect();
    if names_native(af, EQUAL__ANY_MANY__ANY_MANY)
        && ps.len() == 2
        && !rooted_at(&ps[0], src_var)
        && rooted_at(&ps[1], src_var)
    {
        ps.swap(0, 1);
    }
    // the order of the module note: Java compares modulo commutation instead (sameModuloCommutation), one verdict
    if (names_native(af, AND__BOOLEAN_1__BOOLEAN_1) || names_native(af, OR__BOOLEAN_1__BOOLEAN_1))
        && ps.len() == 2
        && ps[0].equality_key() > ps[1].equality_key()
    {
        ps.swap(0, 1);
    }
    p::ValueSpecification::AppliedFunction(Arc::new(af.with_parameters(ps.into())))
}

/// `MappingNormalizer.rootedAt` (:750): whether `v` reads through the variable `var`.
pub fn rooted_at(v: &p::ValueSpecification, var: &JStr) -> bool {
    match v {
        p::ValueSpecification::Variable(x) => &x.name == var,
        p::ValueSpecification::AppliedProperty(ap) => rooted_at(&ap.receiver, var),
        p::ValueSpecification::AppliedFunction(af) if af.parameters.len() == 1 => rooted_at(&af.parameters[0], var),
        // the property-space local marker legacyLocalProperty($row, 'p') is rooted at its row argument
        // (XStorePureEnds emission)
        p::ValueSpecification::AppliedFunction(af2)
            if af2.parameters.len() == 2 && af2.function.eq_str(lite::LEGACY_LOCAL_PROPERTY) =>
        {
            rooted_at(&af2.parameters[0], var)
        }
        _ => false,
    }
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
    // a name step D left unresolved is no element's name here: the model's error, in the words of the inline
    // association predicate's same lookup (ruling 29, the normalizer slice's step 1)
    let ad = model.find_association(&xs.association_name).ok_or_else(|| {
        Thrown::model(
            Phase::Normalize,
            cat!(
                "XStore mapping references unknown association '",
                &xs.association_name,
                "'; mapping=",
                md.qualified_name()
            ),
            None,
        )
    })?;
    // the line's [srcSet, tgtSet] ids select the sets: srcSet = the OWNING end's, tgtSet = the line's target end's
    let mut set_a: Option<&JStr> = None;
    let mut set_b: Option<&JStr> = None;
    if let Some(l0) = xs.property_mappings2.first() {
        let p1 = l0.property_name == ad.property1.property_name;
        set_a = if p1 {
            l0.target_set_id.as_ref()
        } else {
            l0.source_set_id.as_ref()
        };
        set_b = if p1 {
            l0.source_set_id.as_ref()
        } else {
            l0.target_set_id.as_ref()
        };
    }
    let end_a = xstore_pure_ends::xstore_end_of(md, class_a, set_a, model)?;
    let end_b = xstore_pure_ends::xstore_end_of(md, class_b, set_b, model)?;
    // route A (docs/XSTORE_LEG.md): a Pure-set end, or a table-backed end whose column view is LOSSY, takes the
    // property-space emission, sets pinned by id; an exact view keeps the column-space emission
    let reads = if end_a.pure || end_b.pure || end_a.lossy_view || end_b.lossy_view {
        XStoreReads::PropertySpace
    } else {
        XStoreReads::ColumnSpace
    };
    xstore_pure_ends::synthesize(md, xs, ad, class_a, class_b, &end_a, &end_b, model, reads)
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
    let name = &mj.association_name;
    // as synthesizeXStoreMapping's lookup (ruling 29)
    let ad2 = model.find_association(name).ok_or_else(|| {
        Thrown::model(
            Phase::Normalize,
            cat!(
                "ModelJoin mapping references unknown association '",
                name,
                "'; mapping=",
                md.qualified_name()
            ),
            None,
        )
    })?;
    // ends resolve like the XStore path: a Relation(~func) set directly, or a TABLE-BACKED Relational set converted to
    // its column view
    let end_a = xstore_pure_ends::xstore_end_of(md, class_a, None, model)?;
    let end_b = xstore_pure_ends::xstore_end_of(md, class_b, None, model)?;
    if end_a.pure || end_b.pure {
        return Err(Thrown::not_implemented(cat!(
            "ModelJoin association '",
            name,
            "' has a Pure-set end — the property-space route covers XStore only so far (mapping=",
            md.qualified_name(),
            ")"
        )));
    }
    let rf_a = end_a.cols_view.clone();
    let rf_b = end_b.cols_view.clone();
    let params = &mj.lambda.parameters;
    if params.len() != 2 {
        return Err(Thrown::not_implemented(cat!(
            "ModelJoin for '",
            name,
            "' needs a 2-param lambda; got ",
            params.len().to_string()
        )));
    }
    let p0 = &params[0];
    let p1 = &params[1];
    let t0 = p0.r#type.as_ref().and_then(name_ref_or_null);
    let t1 = p1.r#type.as_ref().and_then(name_ref_or_null);
    let (a_var, b_var) = model_join_nesting::pair_end_vars(name, ad2, class_a, class_b, p0, p1, t0, t1)?;
    // the lambda's two parameters one name: one variable cannot be both ends; refused by name, as the parameter
    // count above is (ruling 29). ModelJoinNesting.compose relies on it.
    if a_var == b_var {
        return Err(Thrown::not_implemented(cat!(
            "ModelJoin for '",
            name,
            "' needs two distinct lambda parameter names; got [",
            &p0.name,
            ", ",
            &p1.name,
            "]"
        )));
    }
    let src_row = xstore_pure_ends::variable("srcRow");
    let tgt_row = xstore_pure_ends::variable("tgtRow");
    let row_by_var = LinkedMap::of([(a_var.clone(), src_row.clone()), (b_var.clone(), tgt_row.clone())]);
    // `Map.of(aVar, rfA, bVar, rfB)`: a null value is its NullPointerException (a Pure end, refused above)
    let rf_by_var = LinkedMap::of([
        (a_var.clone(), require(rf_a, None)?),
        (b_var.clone(), require(rf_b, None)?),
    ]);
    let nh = model_join_nesting::compose(
        md,
        model,
        mj,
        ad2,
        class_a,
        class_b,
        &a_var,
        &b_var,
        &rf_by_var,
        end_a.pipeline.as_ref(),
        end_b.pipeline.as_ref(),
    )?;
    let cond = relation_reads::rewrite(
        model_join_nesting::last_statement(&mj.lambda)?,
        &row_by_var,
        &rf_by_var,
        name,
        md,
        Some(&nh.nested_cols),
        model,
    )?;
    // `List.of(a, b, pipeA, pipeB, ...)`: a null pipeline is its NullPointerException
    let pipe_a = require(nh.pipe_a, None)?;
    let pipe_b = require(nh.pipe_b, None)?;
    xstore_pure_ends::predicate_function(md, name, class_a, class_b, pipe_a, pipe_b, src_row, tgt_row, cond)
}
