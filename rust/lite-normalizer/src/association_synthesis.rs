// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `AssociationSynthesis` (AssociationSynthesis.java at ad86fa895): association mappings -- the multi-hop / routed
//! injection pre-pass (appended Join PMs, hoisted owner sets: body order), the association predicate functions
//! (`(a, b) -> Boolean`), withheld reasons as poisons. EF_CENSUS §1.4's families: AS-1 the injection pre-pass
//! (:53-329), AS-2 the end helpers and `resolveAssociation` (:337-398), AS-3 predicate synthesis (:400-554,
//! :586-791).
//!
//! RE-EXPRESSED (census §4): AS:202's `m == md` is "skip index 0" (`closure = [md] ++ closure.mappings()`, and
//! `mappings()` excludes `md`'s own name, so only index 0 can be `md`). The texts holding
//! `getClass().getSimpleName()` (:411, :434) spell each kind's Java simple name (`Cross`, `ModelJoin`, ...).
//! `withInjectedPMs` drops an embedded block's `~primaryKey` (:308-309, ASK 13; lite-model's two-argument
//! `PropertyMappingEmbedded::new`); `resolveAssociation`'s import wildcards and same-package fallback (:387-397,
//! ASK 15) stay as Java has them. Not ported (census §3, AS-4): the 8-argument `buildAssocPredicateBody` (:564-573)
//! and `anchorNameOf` (:795-801), each called only by the other.

use std::sync::Arc;

use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{
    AssociationDefinition, AssociationMapping, ClassMapping, ClassMappingRelational, FunctionDefinition, JStr,
    LegacyMappingDefinition, LegacyMappingDefinitionTableReference as TableRef, PropertyMapping, PropertyMappingJoin,
};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::mapping_ledger::MappingLedger;
use crate::mapping_normalizer::name_ref_or_null;
use crate::resolved_mapping::ResolvedMapping;
use crate::unported;

/// `AssociationSynthesis.injectMultiHopAssociationPMs` (:53): the mapping with each multi-hop / routed association
/// end injected as a class-typed Join PM on its owner's sets (an owner set hoisted from an include when needed).
pub fn inject_multi_hop_association_pms(
    pp: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<Arc<LegacyMappingDefinition>, Thrown> {
    Err(Thrown::Unported("AssociationSynthesis.injectMultiHopAssociationPMs"))
}

/// `AssociationSynthesis.lineageOf` (:232): a set's `extends` chain of set ids, itself first.
pub fn lineage_of(cm: &ClassMapping, view: &ResolvedMapping<'_>) -> Result<Vec<JStr>, Thrown> {
    Err(Thrown::Unported("AssociationSynthesis.lineageOf"))
}

/// `AssociationSynthesis.withRoot` (:244): the set with its root flag replaced.
pub fn with_root(rcm: &ClassMappingRelational, root: bool) -> Arc<ClassMappingRelational> {
    // UNPORTED: AssociationSynthesis.withRoot
    unported!("AssociationSynthesis.withRoot")
}

/// `AssociationSynthesis.withInjectedPMs` (:255): the set with the injected PMs for its class (`by_class`) and its
/// lineage's set ids (`by_set`) appended, a PM it maps itself not repeated; none when nothing is injected.
pub fn with_injected_pms(
    cm: &ClassMapping,
    by_class: &LinkedMap<JStr, Vec<PropertyMapping>>,
    by_set: &LinkedMap<JStr, LinkedMap<JStr, Vec<PropertyMapping>>>,
    lineage: &[JStr],
) -> Option<Arc<ClassMappingRelational>> {
    // UNPORTED: AssociationSynthesis.withInjectedPMs
    unported!("AssociationSynthesis.withInjectedPMs")
}

/// `AssociationSynthesis.recordWithheld` (:337): an association whose predicate binding was not emitted, poisoned by
/// its resolved name (its spelling when unresolved).
pub fn record_withheld(
    md: &ResolvedMapping<'_>,
    am: &AssociationMapping,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
    reason: &JStr,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("AssociationSynthesis.recordWithheld"))
}

/// `AssociationSynthesis.associationTargetClass` (:349): the class at the END `prop_name` names (the property's own
/// target), else none. (Ported now, with its twin: MappingClosures reads them.)
pub fn association_target_class<'d>(ad: &'d AssociationDefinition, prop_name: &JStr) -> Option<&'d JStr> {
    if &ad.property1.property_name == prop_name {
        return name_ref_or_null(&ad.property1.target_class);
    }
    if &ad.property2.property_name == prop_name {
        return name_ref_or_null(&ad.property2.target_class);
    }
    None
}

/// `AssociationSynthesis.associationOwnerClass` (:365): the class OWNING the end `prop_name` (the other end's
/// target), else none.
pub fn association_owner_class<'d>(ad: &'d AssociationDefinition, prop_name: &JStr) -> Option<&'d JStr> {
    if &ad.property1.property_name == prop_name {
        return name_ref_or_null(&ad.property2.target_class);
    }
    if &ad.property2.property_name == prop_name {
        return name_ref_or_null(&ad.property1.target_class);
    }
    None
}

/// `AssociationSynthesis.resolveAssociation` (:379): the association an association mapping names -- exact, else
/// through the mapping's import wildcards, else the mapping's own package (ASK 15's Phase-D fallback, as Java has it).
pub fn resolve_association(
    model: &ModelIndex,
    md: &ResolvedMapping<'_>,
    am: &AssociationMapping,
) -> Option<Arc<AssociationDefinition>> {
    // UNPORTED: AssociationSynthesis.resolveAssociation
    unported!("AssociationSynthesis.resolveAssociation")
}

/// `AssociationSynthesis.synthesizeAssociationMapping` (:400): the association's predicate function; none for a
/// multi-hop association (navigated per end); a withheld reason poisoned.
pub fn synthesize_association_mapping(
    md: &ResolvedMapping<'_>,
    am: &AssociationMapping,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<Option<FunctionDefinition>, Thrown> {
    Err(Thrown::Unported("AssociationSynthesis.synthesizeAssociationMapping"))
}

/// `AssociationSynthesis.EndAnchors` (:586): the two ends' anchor tables.
#[derive(Clone, Debug, PartialEq)]
pub struct EndAnchors {
    pub a: Arc<TableRef>,
    pub b: Arc<TableRef>,
}

/// `AssociationSynthesis.endAnchors` (:590): the anchor tables of the two ends (`id_a`, `id_b` their sets), oriented
/// by the join; none when an end has none.
#[allow(clippy::too_many_arguments)]
pub fn end_anchors(
    md: &ResolvedMapping<'_>,
    class_a: &JStr,
    class_b: &JStr,
    id_a: Option<&JStr>,
    id_b: Option<&JStr>,
    join: &PropertyMappingJoin,
    model: &ModelIndex,
) -> Result<Option<EndAnchors>, Thrown> {
    Err(Thrown::Unported("AssociationSynthesis.endAnchors"))
}

/// `AssociationSynthesis.buildAssocPredicateBody` (:630, the eleven-argument form): the predicate's body over the two
/// rows -- the join's chain translated, the target end oriented by `target_is_a`.
#[allow(clippy::too_many_arguments)]
pub fn build_assoc_predicate_body(
    join: &PropertyMappingJoin,
    class_a: &JStr,
    class_b: &JStr,
    src_row: &Arc<p::Variable>,
    tgt_row: &Arc<p::Variable>,
    association_name: &JStr,
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
    target_is_a: bool,
    source_table: &JStr,
    class_b_table: &JStr,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("AssociationSynthesis.buildAssocPredicateBody"))
}

/// `AssociationSynthesis.anchorTableOf` (:709): the table a class's association end anchors on, over the closure.
pub fn anchor_table_of(
    md: &ResolvedMapping<'_>,
    class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<Option<Arc<TableRef>>, Thrown> {
    Err(Thrown::Unported("AssociationSynthesis.anchorTableOf"))
}

/// `AssociationSynthesis.endAnchor` (:753): an end's anchor -- its set's (`set_id`), else its class's.
pub fn end_anchor(
    md: &ResolvedMapping<'_>,
    class_fqn: &JStr,
    set_id: Option<&JStr>,
    model: &ModelIndex,
) -> Result<Option<Arc<TableRef>>, Thrown> {
    Err(Thrown::Unported("AssociationSynthesis.endAnchor"))
}

/// `AssociationSynthesis.anchorOfSet` (:769): the anchor table of the set `set_id`.
pub fn anchor_of_set(md: &ResolvedMapping<'_>, set_id: &JStr) -> Result<Option<Arc<TableRef>>, Thrown> {
    Err(Thrown::Unported("AssociationSynthesis.anchorOfSet"))
}

/// `AssociationSynthesis.mainTableOfSet` (:787): a set's main table, declared or inferred quietly.
pub fn main_table_of_set(rcm: &ClassMappingRelational) -> Result<Option<Arc<TableRef>>, Thrown> {
    Err(Thrown::Unported("AssociationSynthesis.mainTableOfSet"))
}
