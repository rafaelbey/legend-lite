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
//! `getClass().getSimpleName()` (:418, :441) spell each kind's Java simple name (`Cross`, `ModelJoin`, ...).
//! `withInjectedPMs` drops an embedded block's `~primaryKey` (:308-309, ASK 13; lite-model's two-argument
//! `PropertyMappingEmbedded::new`); `resolveAssociation`'s import wildcards and same-package fallback (:387-397,
//! ASK 15) stay as Java has them. Not ported (census §3, AS-4): the 8-argument `buildAssocPredicateBody` (:564-573)
//! and `anchorNameOf` (:795-801), each called only by the other.
//!
//! Every hash set here is membership-only (census H3: `routedUnionGroups`, `ownClasses`, `ownMapped`, `ownSetIds`,
//! `hoisted`, `seen`, `have`); `byClass` / `bySet` are iterated (the appended PMs' order), so they are `LinkedMap`s.

use std::collections::HashSet;
use std::sync::Arc;

use lite_builtin::pure::lite;
use lite_error::Phase;
use lite_front::nodes;
use lite_front::records as p;
use lite_model::linked::{LinkedMap, empty, list};
use lite_model::names::default_set_id;
use lite_model::{
    AssociationDefinition, AssociationDefinitionAssociationEndDefinition, AssociationMapping,
    AssociationMappingRelational, ClassMapping, ClassMappingRelational, FunctionDefinition,
    FunctionDefinitionParameterDefinition, FunctionDefinitionSynthesized, JStr, LegacyMappingDefinition,
    LegacyMappingDefinitionTableReference as TableRef, PoisonKey, PoisonKeyForAssociation, PropertyMapping,
    PropertyMappingEmbedded, PropertyMappingJoin, SynthHat, cat,
};
use lite_names::{ModelIndex, calls, synth_fqn};

use crate::error::{ILLEGAL_STATE, Thrown, require};
use crate::jdk::LinkedSet;
use crate::mapping_ledger::MappingLedger;
use crate::mapping_normalizer::{
    self, contains_target_column_ref, determine_target_table, infer_main_table_quiet, main_table_or_null,
    name_ref_or_null, require_non_view_target, resolve_view_refs_in_join_keeping,
};
use crate::rel_op_translator::{self, TableScope};
use crate::resolved_mapping::ResolvedMapping;
use crate::view_relation;

/// `AssociationSynthesis.injectMultiHopAssociationPMs` (:53): the mapping with each multi-hop / routed association
/// end injected as a class-typed Join PM on its owner's sets (an owner set hoisted from an include when needed).
pub fn inject_multi_hop_association_pms(
    pp: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<Arc<LegacyMappingDefinition>, Thrown> {
    let md = pp.raw();
    let view = pp;
    // INCLUDE-CLOSURE: the association entries, the owning class mappings and the target unions may live in
    // DIFFERENT mapping definitions; the top-level normalization injects, hoisting an included owner set into this
    // definition when needed (the hoisted copy shadows the included one for this mapping's synthesis)
    let included = pp.closures().closure(&md.qualified_name);
    let mut closure: Vec<Arc<LegacyMappingDefinition>> = Vec::new();
    closure.push(md.clone());
    closure.extend(included.mappings().iter().cloned());
    let mut rels: Vec<Arc<AssociationMappingRelational>> = Vec::new();
    for m in &closure {
        for am in m.association_mappings.iter() {
            if let AssociationMapping::AssociationMappingRelational(r) = am {
                rels.push(r.clone());
            }
        }
    }
    if rels.is_empty() {
        return Ok(md.clone());
    }
    // className -> unqualified injections; className -> setId -> per-set injections (pair entries bind to their
    // SOURCE set only)
    let mut by_class: LinkedMap<JStr, Vec<PropertyMapping>> = LinkedMap::new();
    let mut by_set: LinkedMap<JStr, LinkedMap<JStr, Vec<PropertyMapping>>> = LinkedMap::new();
    for rel in &rels {
        // an included mapping's association mapping of an association the model has not: the model's error, in
        // legend-engine's words, as MappingClosures' same lookup says it (AS:82-89)
        let ad = model.find_association(&rel.association_name).ok_or_else(|| {
            Thrown::model(
                Phase::Mapping,
                cat!("Can't find association '", &rel.association_name, "'"),
                None,
            )
        })?;
        // per (source set, property): a pair group whose TARGET class is union- or inheritance-mapped (or cannot
        // anchor a predicate) is a ROUTED group -- ALL its entries, single-hop included, inject as target-stamped
        // Join PMs on their source set. Only a plain pair between two anchorable classes takes the predicate path.
        let mut routed_union_groups: HashSet<JStr> = HashSet::new();
        for apm in rel.property_mappings.iter() {
            let PropertyMapping::PropertyMappingJoin(join) = &apm.body else {
                continue;
            };
            let Some(source_set_id) = &apm.source_set_id else {
                continue;
            };
            let Some(target) = association_target_class(ad, apm.property_name()) else {
                continue;
            };
            let union_tgt = view.union_of(Some(target)).is_some();
            let inheritance_tgt = !union_tgt && view.inheritance_of(target).is_some();
            let owner0 = association_owner_class(ad, apm.property_name());
            // the SAME anchor rule synthesizeAssociationMapping applies (a class mapped only as an embedded block
            // anchors on its owner's table)
            let binding_possible = match owner0 {
                None => false,
                Some(owner0) => {
                    anchor_table_of(view, owner0, model)?.is_some() && anchor_table_of(view, target, model)?.is_some()
                }
            };
            // the OWNER side too: every arm of a union-mapped owner carries its own navigation
            let op_owner =
                owner0.is_some_and(|o| view.union_of(Some(o)).is_some() || view.inheritance_of(o).is_some());
            // an end whose written id names a NON-ROOT set navigates to THAT set; the class-level predicate pairs the
            // classes' ROOT extents, so such a pair is routed onto its sets too
            let tgt_set_id = join.target_set_id.as_ref().or(apm.target_set_id.as_ref());
            let tgt_set = view.set(tgt_set_id)?;
            let src_set = view.set(Some(source_set_id))?;
            let non_root_end = tgt_set.as_ref().is_some_and(|s| !view.is_root_or_sole(s))
                || src_set.as_ref().is_some_and(|s| !view.is_root_or_sole(s));
            if !union_tgt && !inheritance_tgt && !op_owner && !non_root_end && binding_possible {
                continue; // plain pair: the predicate path
            }
            routed_union_groups.insert(group_key(source_set_id, apm.property_name()));
        }
        for apm in rel.property_mappings.iter() {
            let PropertyMapping::PropertyMappingJoin(join) = &apm.body else {
                continue;
            };
            let routed_union = apm
                .source_set_id
                .as_ref()
                .is_some_and(|s| routed_union_groups.contains(&group_key(s, apm.property_name())));
            if join.joins.len() < 2 && !routed_union {
                continue; // single-hop -> predicate path
            }
            let Some(owner) = association_owner_class(ad, apm.property_name()) else {
                continue;
            };
            // a per-pair entry on a UNION-mapped owner lands on its SOURCE SET's own record too
            let tgt_set = join.target_set_id.as_ref().or(apm.target_set_id.as_ref());
            let stamped = match tgt_set {
                Some(tgt_set) if routed_union && join.target_set_id.is_none() => {
                    PropertyMapping::PropertyMappingJoin(Arc::new(PropertyMappingJoin {
                        property_name: join.property_name.clone(),
                        database: join.database.clone(),
                        joins: join.joins.clone(),
                        target_set_id: Some(tgt_set.clone()),
                    }))
                }
                _ => apm.body.clone(),
            };
            match &apm.source_set_id {
                Some(source_set_id) => by_set
                    .get_or_insert_with(owner.clone(), LinkedMap::new)
                    .get_or_insert_with(source_set_id.clone(), Vec::new)
                    .push(stamped),
                None => by_class.get_or_insert_with(owner.clone(), Vec::new).push(stamped),
            }
        }
    }
    if by_class.is_empty() && by_set.is_empty() {
        return Ok(md.clone());
    }
    let mut rewritten: Vec<ClassMapping> = Vec::with_capacity(md.class_mappings.len());
    let mut own_classes: HashSet<JStr> = HashSet::new();
    let mut own_mapped: HashSet<JStr> = HashSet::new();
    let mut own_set_ids: HashSet<JStr> = HashSet::new();
    for cm in md.class_mappings.iter() {
        if let ClassMapping::ClassMappingRelational(_) = cm {
            own_classes.insert(cm.class_name().clone());
        }
        own_mapped.insert(cm.class_name().clone());
        own_set_ids.insert(ResolvedMapping::id_of(cm));
        let injected_cm = with_injected_pms(cm, &by_class, &by_set, &lineage_of(cm, view)?);
        rewritten.push(match injected_cm {
            Some(injected) => ClassMapping::ClassMappingRelational(injected),
            None => cm.clone(),
        });
    }
    // HOIST: an owner set that lives only in an INCLUDED definition is copied up with its injections (skipped when
    // this definition already maps the class through a set of its own; a class this definition maps only as an
    // OPERATION over included members hoists the members, so each arm's own function carries the navigation)
    let mut hoisted: HashSet<JStr> = HashSet::new();
    // RE-EXPRESSED (census §4): Java's `if (m == md) continue` skips exactly index 0, the mapping prepended above
    for m in closure.iter().skip(1) {
        for cm in m.class_mappings.iter() {
            if own_classes.contains(cm.class_name())
                || own_set_ids.contains(&ResolvedMapping::id_of(cm))
                || !hoisted.insert(ResolvedMapping::id_of(cm))
            {
                continue;
            }
            if let Some(injected) = with_injected_pms(cm, &by_class, &by_set, &lineage_of(cm, view)?) {
                // a hoisted member of a class THIS definition maps as an Operation is a member here: the Operation
                // is the root
                let member = if injected.root && own_mapped.contains(cm.class_name()) {
                    with_root(&injected, false)
                } else {
                    injected
                };
                rewritten.push(ClassMapping::ClassMappingRelational(member));
            }
        }
    }
    Ok(Arc::new(md.with_class_mappings(list(rewritten))))
}

/// `sourceSetId + "\u0000" + propertyName`: a routed pair group's key.
fn group_key(source_set_id: &JStr, property_name: &JStr) -> JStr {
    cat!(source_set_id, "\u{0}", property_name)
}

/// `AssociationSynthesis.lineageOf` (:232): a set's `extends` chain of set ids, itself first.
pub fn lineage_of(cm: &ClassMapping, view: &ResolvedMapping<'_>) -> Result<Vec<JStr>, Thrown> {
    let mut out: Vec<JStr> = Vec::new();
    let mut seen: HashSet<JStr> = HashSet::new();
    let mut cur = Some(ResolvedMapping::id_of(cm));
    while let Some(c) = cur
        && seen.insert(c.clone())
    {
        out.push(c.clone());
        cur = match view.set(Some(&c))? {
            Some(ClassMapping::ClassMappingRelational(ur)) => ur.extends_set_id.clone(),
            _ => None,
        };
    }
    Ok(out)
}

/// `AssociationSynthesis.withRoot` (:244): the set with its root flag replaced.
pub fn with_root(rcm: &ClassMappingRelational, root: bool) -> Arc<ClassMappingRelational> {
    Arc::new(ClassMappingRelational { root, ..rcm.clone() })
}

/// `AssociationSynthesis.withInjectedPMs` (:255): the set with the injected PMs for its class (`by_class`) and its
/// lineage's set ids (`by_set`) appended, a PM it maps itself not repeated; none when nothing is injected.
pub fn with_injected_pms(
    cm: &ClassMapping,
    by_class: &LinkedMap<JStr, Vec<PropertyMapping>>,
    by_set: &LinkedMap<JStr, LinkedMap<JStr, Vec<PropertyMapping>>>,
    lineage: &[JStr],
) -> Option<Arc<ClassMappingRelational>> {
    let ClassMapping::ClassMappingRelational(rcm) = cm else {
        return None;
    };
    let mut add: Vec<PropertyMapping> = Vec::new();
    if let Some(for_class) = by_class.get(&rcm.class_name) {
        add.extend(for_class.iter().cloned());
    }
    // per-SET injections match by SET ID under any owner key (the end's owner may be a SUPERCLASS of the set's
    // class); an ANCESTOR set's entries are inherited (extends lineage), a property the set already maps (or a
    // nearer ancestor's entry) shadowing them
    let mut have: HashSet<JStr> = HashSet::new();
    for pm in rcm.property_mappings.iter() {
        have.insert(pm.property_name().clone());
    }
    let sid = ResolvedMapping::id_of(cm);
    for id in lineage {
        let own = *id == sid;
        for (_, sets) in by_set.iter() {
            let Some(for_set) = sets.get(id) else {
                continue;
            };
            for pm in for_set {
                if own || have.insert(pm.property_name().clone()) {
                    add.push(pm.clone());
                }
            }
        }
        if own {
            for pm in &add {
                have.insert(pm.property_name().clone());
            }
        }
    }
    // EMBEDDED-set sources: an entry keyed <thisSetId>_<embProp> (or the default <classFqnUnderscored>_<embProp>)
    // belongs INSIDE this set's embedded block
    let mut pms: Vec<PropertyMapping> = rcm.property_mappings.to_vec();
    let mut nested = false;
    let class_id = default_set_id(&rcm.class_name);
    for (_, any_sets) in by_set.iter() {
        for (key, entries) in any_sets.iter() {
            let Some(prop) = suffix_after(key, &sid).or_else(|| suffix_after(key, &class_id)) else {
                continue;
            };
            for slot in pms.iter_mut() {
                if let PropertyMapping::PropertyMappingEmbedded(emb) = slot
                    && emb.property_name == prop
                {
                    let mut sub: Vec<PropertyMapping> = emb.property_mappings.to_vec();
                    sub.extend(entries.iter().cloned());
                    // Java's two-argument `new Embedded(name, sub)`: the block's own ~primaryKey is DROPPED here
                    // (AS:315-316, census note 3 / ASK 13), ported as Java has it
                    *slot = PropertyMapping::PropertyMappingEmbedded(Arc::new(PropertyMappingEmbedded::new(
                        emb.property_name.clone(),
                        list(sub),
                    )));
                    nested = true;
                }
            }
        }
    }
    if add.is_empty() && !nested {
        return None;
    }
    pms.extend(add);
    Some(Arc::new(ClassMappingRelational {
        property_mappings: list(pms),
        ..(**rcm).clone()
    }))
}

/// `key.startsWith(prefix + "_") ? key.substring(prefix.length() + 1) : null`.
fn suffix_after(key: &JStr, prefix: &JStr) -> Option<JStr> {
    let head = cat!(prefix, "_");
    key.0.starts_with(&head.0).then(|| JStr::from(&key.0[head.0.len()..]))
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
    let association_fqn = resolve_association(model, md, am)
        .map(|a| a.qualified_name.clone())
        .unwrap_or_else(|| am.association_name().clone());
    ledger.poison(
        PoisonKey::PoisonKeyForAssociation(Arc::new(PoisonKeyForAssociation { association_fqn })),
        cat!("no predicate binding was emitted: ", reason, "; mapping=", md.qualified_name()),
    );
    Ok(())
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
    let name = am.association_name();
    let direct = model.find_association(name);
    if direct.is_some() || lite_model::text::contains(&name.0, "::") {
        return direct.cloned();
    }
    // 2026-11-14 (ruling 30's measurement, the normalizer slice): the import-wildcard fallback is REACHED -- tried on
    // 241 strict / 265 tolerant asks, found 0 / 6 (the slice's own case texts); ported verbatim
    for pkg in model.imports_of(md.qualified_name()).wildcards.iter() {
        if let Some(hit) = model.find_association(&cat!(pkg, "::", name)) {
            return Some(hit.clone());
        }
    }
    // same-package fallback (an unimported sibling). 2026-11-14 (ruling 30's measurement): tried on 259 strict / 277
    // tolerant asks, found 0 / 0; ported verbatim
    let fqn = md.qualified_name();
    let cut = lite_model::text::last_index_of(&fqn.0, &JStr::from("::").0)?;
    model
        .find_association(&cat!(&fqn.0[..cut], "::", name))
        .cloned()
}

/// `AssociationSynthesis.synthesizeAssociationMapping` (:400): the association's predicate function; none for a
/// multi-hop association (navigated per end); a withheld reason poisoned.
pub fn synthesize_association_mapping(
    md: &ResolvedMapping<'_>,
    am: &AssociationMapping,
    model: &ModelIndex,
    ledger: &mut MappingLedger,
) -> Result<Option<FunctionDefinition>, Thrown> {
    // `.orElseGet(MissProbe::miss)`: an unresolved association is none here
    let ad0 = resolve_association(model, md, am);
    if let AssociationMapping::AssociationMappingModelJoin(mj) = am
        && let Some(ad0) = &ad0
    {
        let class_a = target_class_fqn(&ad0.property1)?;
        let class_b = target_class_fqn(&ad0.property2)?;
        return mapping_normalizer::synthesize_model_join_mapping(md, mj, model, &class_a, &class_b).map(Some);
    }
    if let AssociationMapping::AssociationMappingCross(xs) = am
        && let Some(ad0) = &ad0
    {
        let class_a = target_class_fqn(&ad0.property1)?;
        let class_b = target_class_fqn(&ad0.property2)?;
        return mapping_normalizer::synthesize_xstore_mapping(md, xs, model, &class_a, &class_b).map(Some);
    }
    let AssociationMapping::AssociationMappingRelational(rel) = am else {
        // an unresolved ModelJoin / Cross association lands here (census H3 note 2: its text names the kind, not the
        // unknown association), ported as Java has it
        return Err(Thrown::not_implemented(cat!(
            "Association mapping kind ",
            association_mapping_simple_name(am),
            " not supported; mapping=",
            md.qualified_name()
        )));
    };
    let ad = resolve_association(model, md, am).ok_or_else(|| {
        Thrown::model(
            Phase::Normalize,
            cat!(
                "AssociationMapping references unknown association '",
                am.association_name(),
                "'; mapping=",
                md.qualified_name()
            ),
            None,
        )
    })?;
    let class_a = target_class_fqn(&ad.property1)?;
    let class_b = target_class_fqn(&ad.property2)?;

    if rel.property_mappings.is_empty() {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "AssociationMapping for '",
                am.association_name(),
                "' has no property mappings; mapping=",
                md.qualified_name()
            ),
            None,
        ));
    }
    // the FIRST property mapping is the primary (First-PM-wins is RETAINED, audit 23: the two directions may spell
    // inverse-equivalent joins, and the first join's predicate is row-correct for both)
    let first_am = &rel.property_mappings[0];
    let PropertyMapping::PropertyMappingJoin(first_join) = &first_am.body else {
        return Err(Thrown::not_implemented(cat!(
            "AssociationMapping property body kind ",
            property_mapping_simple_name(&first_am.body),
            " not supported (only Join bodies are bridged); mapping=",
            md.qualified_name(),
            ". See docs/MAPPING_LEGACY_TO_FUNCTION.md \u{a7}5.6."
        )));
    };
    // multi-hop association: realized as per-end navigation injected into the class realizing functions (Option A);
    // a (A,B)->Boolean predicate cannot bind the intermediate row
    if first_join.joins.len() >= 2 {
        return Ok(None);
    }
    // ENGINE RULE: an end written with a set id (prop[sourceId, targetId]) anchors on THAT set; an end without one
    // keeps the class rule
    let src_id = first_am.source_set_id.as_ref();
    let tgt_id = first_join.target_set_id.as_ref().or(first_am.target_set_id.as_ref());
    let on_property1 = ad.property1.property_name == *first_am.property_name();
    let anchors = end_anchors(
        md,
        &class_a,
        &class_b,
        if on_property1 { tgt_id } else { src_id },
        if on_property1 { src_id } else { tgt_id },
        first_join,
        model,
    )?;
    let Some(anchors) = anchors else {
        // WITHHELD, with the reason recorded: navigating this association is loud at resolve time
        let unanchored = if anchor_table_of(md, &class_a, model)?.is_none() {
            &class_a
        } else {
            &class_b
        };
        record_withheld(
            md,
            am,
            model,
            ledger,
            &cat!(
                "end class '",
                unanchored,
                "' has no table to anchor a (source, target) predicate on",
                " (its properties live only as Join property mappings on the",
                " other end, or neither end's set is visible from this",
                " mapping); declaring the association is not an error, but",
                " navigating it has no step"
            ),
        )?;
        return Ok(None);
    };
    // an OPERATION-mapped end (union / inheritance) has no one table to anchor on: its pairs inject onto the member
    // sets (above); no predicate, and a navigation without a step is loud at demand
    if md.union_of(Some(&class_a)).is_some()
        || md.inheritance_of(&class_a).is_some()
        || md.union_of(Some(&class_b)).is_some()
        || md.inheritance_of(&class_b).is_some()
    {
        record_withheld(
            md,
            am,
            model,
            ledger,
            &cat!(
                "an end class is OPERATION-mapped",
                " (union or inheritance), so there is no single table to anchor a",
                " (source, target) predicate on \u{2014} the pair entries inject onto the",
                " member sets instead; a navigation that finds no step here is loud"
            ),
        )?;
        return Ok(None);
    }

    let src_row = Arc::new(nodes::variable(JStr::from("srcRow")));
    let tgt_row = Arc::new(nodes::variable(JStr::from("tgtRow")));
    // A SELF-JOIN between two DIFFERENT classes over ONE table: the join's table-named side is the first line's
    // SOURCE set and {target} its target set -- when that source set maps classB, {target} is classA's row
    let mut target_is_a = false;
    if class_a != class_b
        && let Some(source_set_id) = &first_am.source_set_id
    {
        let src_set = md.set(Some(source_set_id))?;
        target_is_a = src_set.is_some_and(|s| *s.class_name() == class_b);
    }
    let predicate_body = build_assoc_predicate_body(
        first_join,
        &class_a,
        &class_b,
        &src_row,
        &tgt_row,
        am.association_name(),
        md,
        model,
        target_is_a,
        &anchors.a.table,
        &anchors.b.table,
    )?;

    let a = Arc::new(nodes::variable(JStr::from("a")));
    let b = Arc::new(nodes::variable(JStr::from("b")));
    // the adapter lambda speaks ROW scope; its row types are the two ends' anchors, said by the relation arguments
    let src_rel = view_relation::source_ref_for(&anchors.a, model, md)?;
    let tgt_rel = view_relation::source_ref_for(&anchors.b, model, md)?;
    let condition = p::ValueSpecification::LambdaFunction(Arc::new(p::LambdaFunction {
        parameters: list(vec![src_row, tgt_row]),
        body: list(vec![predicate_body]),
        pos: None,
    }));
    let body = p::ValueSpecification::AppliedFunction(Arc::new(calls::exact(
        &JStr::from(lite::LEGACY_ASSOC_PREDICATE),
        vec![var(&a), var(&b), src_rel, tgt_rel, condition],
    )));

    let p_a = FunctionDefinitionParameterDefinition {
        name: JStr::from("a"),
        r#type: nodes::name_ref(class_a, None),
        multiplicity: pure_one(),
    };
    let p_b = FunctionDefinitionParameterDefinition {
        name: JStr::from("b"),
        r#type: nodes::name_ref(class_b, None),
        multiplicity: pure_one(),
    };
    let function = FunctionDefinition::user_written(
        synth_fqn::mapping_assoc(md.qualified_name(), am.association_name())?,
        empty(),
        empty(),
        list(vec![Arc::new(p_a), Arc::new(p_b)]),
        nodes::name_ref(JStr::from("meta::pure::metamodel::type::Boolean"), None),
        pure_one(),
        list(vec![body]),
        empty(),
        empty(),
    )
    .with_synthesized_from(Arc::new(FunctionDefinitionSynthesized {
        hat: SynthHat::ASSOC,
        owner_fqn: md.qualified_name().clone(),
        member_name: am.association_name().clone(),
        forward_property: Some(first_am.property_name().clone()),
    }));
    Ok(Some(function))
}

/// `am.getClass().getSimpleName()` (:418): the association mapping record's Java simple name.
fn association_mapping_simple_name(am: &AssociationMapping) -> &'static str {
    match am {
        AssociationMapping::AssociationMappingRelational(_) => "Relational",
        AssociationMapping::AssociationMappingCross(_) => "Cross",
        AssociationMapping::AssociationMappingModelJoin(_) => "ModelJoin",
    }
}

/// `firstAm.body().getClass().getSimpleName()` (:441): the property mapping record's Java simple name.
fn property_mapping_simple_name(pm: &PropertyMapping) -> &'static str {
    match pm {
        PropertyMapping::PropertyMappingEnumeratedExpression(_) => "EnumeratedExpression",
        PropertyMapping::PropertyMappingColumn(_) => "Column",
        PropertyMapping::PropertyMappingEnumeratedColumn(_) => "EnumeratedColumn",
        PropertyMapping::PropertyMappingJoin(_) => "Join",
        PropertyMapping::PropertyMappingJoinTerminalColumn(_) => "JoinTerminalColumn",
        PropertyMapping::PropertyMappingExpression(_) => "Expression",
        PropertyMapping::PropertyMappingEmbedded(_) => "Embedded",
        PropertyMapping::PropertyMappingInlineEmbedded(_) => "InlineEmbedded",
        PropertyMapping::PropertyMappingOtherwiseEmbedded(_) => "OtherwiseEmbedded",
        PropertyMapping::PropertyMappingLocalProperty(_) => "LocalProperty",
    }
}

/// `end.targetClassFqn()` (lite-model's): its one throw, the `IllegalStateException`, converted by hand (MAP §2:
/// lite-model's `Thrown` names its class by a `String`), class and message kept.
fn target_class_fqn(end: &AssociationDefinitionAssociationEndDefinition) -> Result<JStr, Thrown> {
    match end.target_class_fqn() {
        Ok(fqn) => Ok(fqn.clone()),
        Err(lite_model::Thrown::Java(e)) if e.class == ILLEGAL_STATE => Err(Thrown::java(ILLEGAL_STATE, e.message)),
        // the method throws nothing else; kept total by carrying the throw's description in the same class
        Err(other) => Err(Thrown::illegal_state(JStr::from(format!("{other:?}")))),
    }
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
    let a = end_anchor(md, class_a, id_a, model)?;
    let b = end_anchor(md, class_b, id_b, model)?;
    if let (Some(a), Some(b)) = (&a, &b) {
        return Ok(Some(EndAnchors {
            a: a.clone(),
            b: b.clone(),
        }));
    }
    if (a.is_none() && b.is_none()) || join.joins.len() != 1 {
        return Ok(None);
    }
    // an end class mapped OUTSIDE the closure: the table the single-hop join names for that end
    let hop = &join.joins[0];
    let db = hop.database_name.clone().unwrap_or_else(|| join.database.clone());
    let jd = model.find_join(Some(&db), &hop.join_name).ok_or_else(|| {
        Thrown::model(
            Phase::Normalize,
            cat!(
                "AssociationMapping join '",
                &hop.join_name,
                "' not found in db '",
                &db,
                "'; mapping=",
                md.qualified_name()
            ),
            None,
        )
    })?;
    let known = require(a.as_ref().or(b.as_ref()), None)?;
    let other = if contains_target_column_ref(&jd.operation)? {
        known.table.clone() // a self-join: {target} is the same table
    } else {
        let mut tables: LinkedSet<JStr> = LinkedSet::new();
        rel_op_translator::collect_tables_in(&jd.operation, &mut tables)?;
        tables.remove(&known.table);
        match (tables.len(), tables.iter().next()) {
            (1, Some(t)) => t.clone(),
            _ => return Ok(None),
        }
    };
    let derived = Arc::new(TableRef {
        database: db,
        table: other,
    });
    Ok(Some(match a {
        Some(a) => EndAnchors { a, b: derived },
        None => EndAnchors {
            a: derived,
            b: require(b, None)?,
        },
    }))
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
    if join.joins.is_empty() {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "AssociationMapping for '",
                association_name,
                "' has empty join chain; mapping=",
                md.qualified_name()
            ),
            None,
        ));
    }
    if join.joins.len() == 1 {
        let hop = &join.joins[0];
        let hop_db = hop.database_name.clone().unwrap_or_else(|| join.database.clone());
        let jd = model.find_join(Some(&hop_db), &hop.join_name).ok_or_else(|| {
            Thrown::model(
                Phase::Normalize,
                cat!(
                    "AssociationMapping join '",
                    &hop.join_name,
                    "' not found in db '",
                    &hop_db,
                    "'; association='",
                    association_name,
                    "', mapping=",
                    md.qualified_name()
                ),
                None,
            )
        })?;
        // the synthesized legacyAssocPredicate call declares tgtRow's row type as classB's ~mainTable; the join must
        // actually land there, or the lambda's column reads would silently mistype
        let backing_view = model.find_view(&hop_db, source_table).is_some().then_some(source_table);
        let keep_target_view = model.find_view(&hop_db, class_b_table).is_some().then_some(class_b_table);
        let cond2 = resolve_view_refs_in_join_keeping(
            &jd.operation,
            &hop_db,
            Some(source_table),
            model,
            Some(md),
            backing_view,
            None,
            keep_target_view,
            true, // anySide
        )?;
        let target_table = determine_target_table(
            &cond2,
            Some(source_table),
            &hop.join_name,
            Some(association_name),
            1,
            md.qualified_name(),
        )?;
        if target_table != *class_b_table {
            // a join landing on classB's OWN main source is fine even when that source is a VIEW; any OTHER view
            // target stays a named wall
            require_non_view_target(&target_table, &hop_db, &hop.join_name, model, md)?;
        }
        if target_table != *class_b_table {
            return Err(Thrown::not_implemented(cat!(
                "AssociationMapping join '",
                &hop.join_name,
                "' lands on table '",
                &target_table,
                "' but the target end class '",
                class_b,
                "' is mapped to ~mainTable '",
                class_b_table,
                "'; an ",
                "association joining through a non-mainTable row is not ",
                "supported. Association='",
                association_name,
                "', mapping=",
                md.qualified_name()
            )));
        }
        let swap = target_is_a && contains_target_column_ref(&cond2)?;
        let table_side = if swap { tgt_row } else { src_row };
        let target_side = if swap { src_row } else { tgt_row };
        let mut cond_scope = TableScope::new();
        cond_scope.put(source_table.clone(), var(table_side));
        if target_table != *source_table {
            cond_scope.put(target_table, var(target_side));
        }
        // translate the SAME view-resolved tree the target was picked from (T1.10)
        return rel_op_translator::translate(&cond2, &cond_scope, Some(&var(target_side)), None, None);
    }
    // Unreachable: multi-hop associations are intercepted in synthesizeAssociationMapping (returns none); this guard
    // fires only if that interception is bypassed -- a compiler invariant violation
    Err(Thrown::model(
        Phase::Normalize,
        cat!(
            "Multi-hop AssociationMapping for '",
            association_name,
            "' (",
            join.joins.len().to_string().as_str(),
            " join hops) reached the predicate ",
            "builder; it should have been handled by per-end injection. ",
            "Mapping=",
            md.qualified_name()
        ),
        None,
    ))
}

/// `AssociationSynthesis.anchorTableOf` (:709): the table a class's association end anchors on, over the closure.
pub fn anchor_table_of(
    md: &ResolvedMapping<'_>,
    class_fqn: &JStr,
    model: &ModelIndex,
) -> Result<Option<Arc<TableRef>>, Thrown> {
    if let Some(own) = main_table_or_null(md, Some(class_fqn))? {
        return Ok(Some(own));
    }
    // a class mapped ONLY as an EMBEDDED block anchors on the OWNING set's main table
    for m in &md.closure() {
        for cm in m.class_mappings.iter() {
            let ClassMapping::ClassMappingRelational(rcm) = cm else {
                continue;
            };
            for pm in rcm.property_mappings.iter() {
                if !matches!(
                    pm,
                    PropertyMapping::PropertyMappingEmbedded(_) | PropertyMapping::PropertyMappingOtherwiseEmbedded(_)
                ) {
                    continue;
                }
                let knowledge = model.knowledge();
                let owner = knowledge.hierarchy_class(Some(&rcm.class_name)).ok_or_else(|| {
                    Thrown::illegal_state(cat!(
                        "F7.8: class unresolved at AssociationSynthesis#1 (this default NEVER fired on the corpus \
                         census; a miss here is a real model gap): ",
                        &rcm.class_name
                    ))
                })?;
                // (Java's `owner == null ? null : ...` is dead after the orElseThrow: census H3)
                let pt = knowledge.property_type(Some(owner.as_ref()), pm.property_name())?;
                if let Some(p::TypeExpression::TypeExpressionNameRef(nr)) = &pt
                    && nr.name == *class_fqn
                {
                    let mt = match &rcm.main_table {
                        Some(mt) => Some(mt.clone()),
                        None => infer_main_table_quiet(rcm)?,
                    };
                    if mt.is_some() {
                        return Ok(mt);
                    }
                }
            }
        }
    }
    Ok(None)
}

/// `AssociationSynthesis.endAnchor` (:753): an end's anchor -- its set's (`set_id`), else its class's.
pub fn end_anchor(
    md: &ResolvedMapping<'_>,
    class_fqn: &JStr,
    set_id: Option<&JStr>,
    model: &ModelIndex,
) -> Result<Option<Arc<TableRef>>, Thrown> {
    if let Some(set_id) = set_id
        && let Some(by_id) = anchor_of_set(md, set_id)?
    {
        return Ok(Some(by_id));
    }
    anchor_table_of(md, class_fqn, model)
}

/// `AssociationSynthesis.anchorOfSet` (:769): the anchor table of the set `set_id`.
pub fn anchor_of_set(md: &ResolvedMapping<'_>, set_id: &JStr) -> Result<Option<Arc<TableRef>>, Thrown> {
    if let Some(ClassMapping::ClassMappingRelational(rcm)) = md.set(Some(set_id))? {
        return main_table_of_set(&rcm);
    }
    // an EMBEDDED set, id <ownerId>_<property>: its owner's table (`for (cut = lastIndexOf('_'); cut > 0;
    // cut = lastIndexOf('_', cut - 1))`)
    let units = &set_id.0;
    let mut cut = units.iter().rposition(|&c| c == u16::from(b'_'));
    while let Some(at) = cut
        && at > 0
    {
        let prop = JStr::from(&units[at + 1..]);
        if let Some(ClassMapping::ClassMappingRelational(owner)) = md.set(Some(&JStr::from(&units[..at])))?
            && owner.property_mappings.iter().any(|pm| {
                matches!(
                    pm,
                    PropertyMapping::PropertyMappingEmbedded(_) | PropertyMapping::PropertyMappingOtherwiseEmbedded(_)
                ) && *pm.property_name() == prop
            })
        {
            return main_table_of_set(&owner);
        }
        cut = units[..at].iter().rposition(|&c| c == u16::from(b'_'));
    }
    Ok(None)
}

/// `AssociationSynthesis.mainTableOfSet` (:787): a set's main table, declared or inferred quietly.
pub fn main_table_of_set(rcm: &ClassMappingRelational) -> Result<Option<Arc<TableRef>>, Thrown> {
    match &rcm.main_table {
        Some(mt) => Ok(Some(mt.clone())),
        None => infer_main_table_quiet(rcm),
    }
}

/// A variable as a value.
fn var(v: &Arc<p::Variable>) -> p::ValueSpecification {
    p::ValueSpecification::Variable(v.clone())
}

/// `Multiplicity.Concrete.PURE_ONE`.
fn pure_one() -> p::Multiplicity {
    nodes::concrete(1, Some(1))
}
