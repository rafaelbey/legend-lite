// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer families 1 and 2 (EF_CENSUS §1.1): the entry, the per-mapping wall (`withElement`), and the
//! legacy mapping's binding table (`normalizeMapping`, its per-set, per-class and per-association catches). The
//! catches are the answer (§9.4): port their placement, which kinds reach `strictErrors`, the first-line wall and
//! the element attribution exactly.
//!
//! Tolerant mode's quirks are kept as Java has them (ASK 10, ruling 30): a mapping walled after some of its functions
//! entered `lifted` leaves them there (orphans); `AggregateViewLift.lift` runs outside the per-class catch (a view
//! set's error walls the whole mapping); the per-association catch puts a `NotImplementedException` into
//! `strictErrors` too (against its comment), so a strict build fails on an association's roadmap gap.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use lite_error::Phase;
use lite_model::linked::{LinkedMap, list};
use lite_model::{
    AssociationMapping, ClassMapping, ClassMappingRelational, FunctionDefinition, JStr, LegacyMappingDefinition,
    MappingDefinition, MappingDefinitionAssociationBinding, MappingDefinitionClassBinding,
    MappingDefinitionClassBindingAggregateViewFacts, MappingDefinitionClassBindingDeclaredKeys as DeclaredKeys,
    MappingDefinitionClassBindingOperation, MappingDefinitionClassBindingPure, MappingDefinitionClassBindingRelational,
    NormalizedModel, PackageableElement, ParsedModel, PoisonKey, PoisonKeyForAssociation, PoisonKeyForClass,
    PoisonKeyForSet, cat,
};
use lite_names::ModelIndex;

use super::{
    clean_sheet_to_canonical, declared_primary_key_columns, is_operation, property_pins_of, relational_source_of,
    synthesize_class_mapping,
};
use crate::aggregate_view_lift;
use crate::association_synthesis;
use crate::error::{Thrown, require};
use crate::lifted_views::LiftedViews;
use crate::mapping_closures::MappingClosures;
use crate::mapping_ledger::MappingLedger;
use crate::mapping_pre_pass;
use crate::resolved_mapping::ResolvedMapping;
use crate::set_key_facts;
use crate::store_substitution_rewrite;

/// `MappingNormalizer.normalize(parsed, model, wallSink, views)` (:152): E.1 over every legacy and clean-sheet
/// mapping, element order. Store substitutions resolved once (`StoreSubstitutionRewrite.resolveAllStores`, outside
/// every catch), every mapping's pre-pass run to completion first (`MappingPrePass.run`, with the E-local
/// `MappingClosures` made here: `MappingClosures::new(model)`), then each mapping normalized under `withElement`, a
/// `ModelException` with an element walled (first line, `putIfAbsent`) when `wall_sink` is given; the lifted functions
/// appended after the structural elements.
pub fn normalize(
    parsed: &ParsedModel,
    model: &ModelIndex,
    mut wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
    views: &LiftedViews<'_>,
) -> Result<NormalizedModel, Thrown> {
    // Java's requireNonNull(parsed / model): references here
    let mut out: Vec<PackageableElement> = Vec::with_capacity(parsed.elements.len());
    let mut lifted: Vec<Arc<FunctionDefinition>> = Vec::new();
    let mut legacy_surfaces: LinkedMap<JStr, Arc<LegacyMappingDefinition>> = LinkedMap::new();
    // store substitutions resolved ONCE for every mapping, include order (stamped on each)
    let legacy: Vec<Arc<LegacyMappingDefinition>> = parsed
        .elements
        .iter()
        .filter_map(|el| match el {
            PackageableElement::LegacyMappingDefinition(md) => Some(md.clone()),
            _ => None,
        })
        .collect();
    // a tolerant build walls the mapping resolve_all_stores refuses and resolves the rest without it (2026-11-15, the
    // normalizer batch, item 4 (b))
    let mut store_walled: HashSet<JStr> = HashSet::new();
    let resolved_stores = loop {
        let left: Vec<Arc<LegacyMappingDefinition>> = legacy
            .iter()
            .filter(|m| !store_walled.contains(&m.qualified_name))
            .cloned()
            .collect();
        match store_substitution_rewrite::resolve_all_stores(&left, model) {
            Ok(r) => break r,
            Err(e) => match (wall_sink.as_deref_mut(), e.element()) {
                (Some(sink), Some(element)) if e.is_model() && store_walled.insert(element.clone()) => {
                    sink.put_if_absent(element, e.first_line());
                }
                _ => return Err(e),
            },
        }
    };
    // EVERY mapping's pre-pass runs to completion first, then the graph-wide mapped-class fact is fixed -- a mapping's
    // synthesis never depends on which mappings normalized before it
    let closures = MappingClosures::new(model);
    let resolved = mapping_pre_pass::run(parsed, model, &closures, wall_sink.as_deref_mut(), views)?;
    let no_stores: LinkedMap<JStr, JStr> = LinkedMap::new();
    for el in parsed.elements.iter() {
        match el {
            PackageableElement::LegacyMappingDefinition(md) => {
                if store_walled.contains(&md.qualified_name) {
                    continue; // walled by resolve_all_stores (tolerant build)
                }
                let Some(pp) = resolved.get(&md.qualified_name) else {
                    continue; // walled by the pre-pass (tolerant build)
                };
                legacy_surfaces.put(md.qualified_name.clone(), pp.surface().clone());
                // the legacy record does NOT flow past Phase E; what this mapping's synthesis learns rides its own
                // ledger, stamped on the compiled mapping -- never the shared index
                let mut ledger = MappingLedger::new(MappingLedger::mapped_in_closure(pp, &resolved));
                let stores = resolved_stores.get(&md.qualified_name).unwrap_or(&no_stores);
                let strict = wall_sink.is_none();
                let normalized = with_element(&md.qualified_name, || {
                    let m = normalize_mapping(pp, model, &mut lifted, stores, &mut ledger)?;
                    // THE DRIVER'S policy (B4): a STRICT build rejects what the engine's compiler rejects -- the first
                    // recorded error; a MODULE build keeps the poisons
                    if strict && let Some(first) = ledger.strict_errors.first() {
                        return Err(first.clone());
                    }
                    Ok(m)
                });
                match normalized {
                    Ok(m) => out.push(PackageableElement::MappingDefinition(Arc::new(m))),
                    // a walled mapping has lifted nothing (`MappingNormalizer.normalize`'s note): the truncation the
                    // normalizer batch's item 4 (c) put here was unreachable, deleted 2026-11-15 (the normalizer's
                    // second batch, item 4 (c))
                    Err(e) if e.is_model() => wall(wall_sink.as_deref_mut(), e)?,
                    Err(e) => return Err(e),
                }
            }
            PackageableElement::CleanSheetMappingDefinition(canonical) => {
                // Clean-sheet (Door 1/3) mapping: the pre-E surface tree is translated to the compiled binding table
                // here -- inline bodies lambda-lift, ref bindings keep the user's FQN, and EVERY relational binding
                // stamps its source
                match with_element(&canonical.qualified_name, || {
                    clean_sheet_to_canonical(canonical, model, &mut lifted)
                }) {
                    Ok(m) => out.push(PackageableElement::MappingDefinition(Arc::new(m))),
                    Err(e) if e.is_model() => wall(wall_sink.as_deref_mut(), e)?,
                    Err(e) => return Err(e),
                }
            }
            other => out.push(other.clone()),
        }
    }
    // lifted realizing functions are ordinary top-level elements -- appended after the structural elements, never
    // stored on the mapping record
    out.extend(lifted.into_iter().map(PackageableElement::FunctionDefinition));
    Ok(NormalizedModel {
        elements: list(out),
        imports: parsed.imports.clone(),
        legacy_surfaces: legacy_surfaces.freeze(),
    })
}

/// The `catch (ModelException e)` of `normalize` (:198, :214): rethrown under a strict build or when it names no
/// element; otherwise its first line walls its element (`putIfAbsent`).
fn wall(wall_sink: Option<&mut LinkedMap<JStr, JStr>>, e: Thrown) -> Result<(), Thrown> {
    let (Some(sink), Some(element)) = (wall_sink, e.element()) else {
        return Err(e);
    };
    sink.put_if_absent(element, e.first_line());
    Ok(())
}

/// `MappingNormalizer.withElement(elementFqn, work)` (:238): attach the element's FQN to what escapes `work` -- ONE
/// wrap covers every throw inside a mapping's normalization ([`Thrown::with_element`]: a `ModelException` without an
/// element gets this one, a `NotImplementedException` becomes `ModelException(NORMALIZE)`, a bug stays raw).
pub fn with_element<T>(element_fqn: &JStr, work: impl FnOnce() -> Result<T, Thrown>) -> Result<T, Thrown> {
    work().map_err(|e| e.with_element(element_fqn))
}

/// The poison every non-root set of a multi-set class without a union root records against its class (:306-309).
const MULTI_SET_POISON: &str = "class is mapped through multiple set IDs; .all() over multi-set mappings (implicit \
                                union) is a roadmap feature";

/// `MappingNormalizer.normalizeMapping` (:258): the legacy mapping -> its canonical binding table (`ClassBinding`s in
/// declaration order, association bindings, flattened enumeration mappings, `resolved_stores`, the ledger's facts);
/// the lifted functions appended to `lifted`. `mixedUnionRooted` (:280) is not ported (written, never read).
pub fn normalize_mapping(
    pp: &ResolvedMapping<'_>,
    model: &ModelIndex,
    lifted: &mut Vec<Arc<FunctionDefinition>>,
    resolved_stores: &LinkedMap<JStr, JStr>,
    ledger: &mut MappingLedger,
) -> Result<MappingDefinition, Thrown> {
    // the pre-pass (MappingPrePass) already ran for every mapping
    let declared_keys = pp.declared_keys();
    // inject MULTI-HOP association ends as class-typed Join PMs (Option A, MAPPING_LEGACY_TO_FUNCTION.md §5.6.1b)
    let md = pp.with_mapping(association_synthesis::inject_multi_hop_association_pms(pp, model)?);

    // a class mapped through MULTIPLE set IDs synthesizes its ROOT set only (.all() dispatches to the root); every
    // non-root set realizes as its own function and binds by set id
    let mut mappings_per_class: HashMap<JStr, i64> = HashMap::new();
    for cm in md.class_mappings().iter() {
        *mappings_per_class.entry(cm.class_name().clone()).or_insert(0) += 1;
    }
    let mut class_bindings: Vec<MappingDefinitionClassBinding> = Vec::with_capacity(md.class_mappings().len());
    let mut union_rooted: HashSet<JStr> = HashSet::new();
    for cm in md.class_mappings().iter() {
        if let ClassMapping::ClassMappingUnion(un) = cm {
            union_rooted.insert(un.class_name.clone());
        }
    }
    for cm in md.class_mappings().iter() {
        if *require(mappings_per_class.get(cm.class_name()), None)? > 1 && !cm.root() {
            let union_member = union_rooted.contains(cm.class_name());
            // EVERY non-root set realizes as its own function and binds by set id (legacy routes as composition): a
            // union member too -- a route names the member's function, and the union is those functions stacked
            if !union_member {
                // multi-set class without a UNION root: .all() is undefined (poisoned); the SET itself still realizes
                // via the set-discriminated binding
                ledger.poison(class_key(cm.class_name()), JStr::from(MULTI_SET_POISON));
            }
            if let Some(invalid_set) = pp.invalid_reason(cm) {
                ledger.poison(set_key(cm), invalid_set.clone());
                continue;
            }
            // the per-SET catch covers the synthesis, the lift and the binding's construction
            let attempt = (|| -> Result<(), Thrown> {
                let set_fn = Arc::new(synthesize_class_mapping(&md, cm, model, true, ledger)?);
                lifted.push(set_fn.clone());
                let binding = class_binding(cm, false, &set_fn.qualified_name, declared_keys, ledger, |_| {
                    Ok(Vec::new())
                })?;
                class_bindings.push(binding);
                Ok(())
            })();
            match attempt {
                Ok(()) => {}
                // per-SET fault isolation, same line as per-class
                Err(e) if e.is_model_or_not_implemented() => {
                    let reason = e.message_or_null();
                    if e.is_model() {
                        ledger.strict_errors.push(e);
                    }
                    ledger.poison(set_key(cm), reason);
                }
                Err(e) => return Err(e),
            }
            continue;
        }
        if let Some(invalid) = pp.invalid_reason(cm) {
            ledger.poison(class_key(cm.class_name()), invalid.clone());
            continue;
        }
        let f = match synthesize_class_mapping(&md, cm, model, false, ledger) {
            Ok(f) => Arc::new(f),
            // a USER-model error the engine rejects at compile time is rejected by a STRICT build too (the driver
            // throws the recorded error); only a MODULE build defers it, and a ROADMAP gap defers in both. PER-CLASS
            // fault isolation: the binding is withheld; fetching THIS class raises the recorded reason (the full
            // message rides on the poison)
            Err(e) if e.is_model_or_not_implemented() => {
                let reason = e.message_or_null();
                if e.is_model() {
                    ledger.strict_errors.push(e);
                }
                ledger.poison(class_key(cm.class_name()), reason);
                continue;
            }
            Err(e) => return Err(e),
        };
        lifted.push(f.clone());
        // inside the per-class fault isolation: a view set's failure poisons the class, the class's and its views'
        // functions and bindings withdrawn (2026-11-15, the normalizer batch, item 4 (b): it walled the whole mapping)
        if let ClassMapping::ClassMappingRelational(agg_main) = cm
            && agg_main.aggregation.is_some()
        {
            let lifted_at = lifted.len() - 1;
            let bound_at = class_bindings.len();
            match aggregate_view_lift::lift(&md, agg_main, model, lifted, &mut class_bindings, declared_keys, ledger) {
                Ok(()) => {}
                Err(e) if e.is_model_or_not_implemented() => {
                    let reason = e.message_or_null();
                    if e.is_model() {
                        ledger.strict_errors.push(e);
                    }
                    lifted.truncate(lifted_at);
                    class_bindings.truncate(bound_at);
                    ledger.poison(class_key(cm.class_name()), reason);
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        let binding = class_binding(
            cm,
            cm.root(),
            &f.qualified_name,
            declared_keys,
            ledger,
            aggregate_view_lift::facts,
        )?;
        class_bindings.push(binding);
    }
    let mut assoc_bindings: Vec<Arc<MappingDefinitionAssociationBinding>> =
        Vec::with_capacity(md.association_mappings().len());
    for am in md.association_mappings().iter() {
        // none => multi-hop association (per-end navigation above)
        let f = match association_synthesis::synthesize_association_mapping(&md, am, model, ledger) {
            Ok(f) => f,
            // PER-ASSOCIATION fault isolation: the class bindings stay queryable; navigating THIS association raises
            // the recorded reason via the poison channel. Both kinds reach strictErrors here (its comment says
            // "TOLERANT (module) builds only"; a strict build throws the recorded error, an NIE included)
            Err(e) if e.is_model_or_not_implemented() => {
                let reason = e.message_or_null();
                // a NotImplementedException defers in both builds, as the per-class catch has it (2026-11-15, the
                // normalizer batch, item 4 (d): it reached strictErrors too)
                if e.is_model() {
                    ledger.strict_errors.push(e);
                }
                // the poison key's `.orElse(am.associationName())` (MN:432), a Phase-D fallback ruling 30's measuring
                // run found reached (2026-11-14): 207 strict and 218 tolerant asks of the front oracle's; ported
                // verbatim, the owner's to rule (its sibling at the binding, MN:441, reached by none, is a refusal)
                let name = association_synthesis::resolve_association(model, &md, am)
                    .map(|a| a.qualified_name.clone())
                    .unwrap_or_else(|| am.association_name().clone());
                ledger.poison(association_key(name), reason);
                continue;
            }
            Err(e) => return Err(e),
        };
        if let Some(f) = f {
            let f = Arc::new(f);
            lifted.push(f.clone());
            // the binding is keyed by the association synthesis found; one it did not find is the model's error, in
            // MappingClosures' words for the same lookup
            let association_fqn = association_name(&md, am, model)?;
            assoc_bindings.push(Arc::new(MappingDefinitionAssociationBinding {
                association_fqn,
                predicate_function_fqn: f.qualified_name.clone(),
            }));
        }
    }
    // includes survive the rewrite unchanged; enumeration mappings are FLATTENED here (own + includes', transitively)
    Ok(MappingDefinition::new(
        md.qualified_name().clone(),
        md.includes().clone(),
        list(class_bindings),
        list(assoc_bindings),
        list(md.enumeration_mappings_with_includes()?),
        list(resolved_stores.iter().cloned().collect()),
        Arc::new(ledger.facts(pp.surface())),
    ))
}

/// The association a binding is keyed by: the one synthesis resolved, else "Can't find association" (:438-442).
fn association_name(md: &ResolvedMapping<'_>, am: &AssociationMapping, model: &ModelIndex) -> Result<JStr, Thrown> {
    association_synthesis::resolve_association(model, md, am)
        .map(|a| a.qualified_name.clone())
        .ok_or_else(|| {
            Thrown::model(
                Phase::Mapping,
                cat!("Can't find association '", am.association_name(), "'"),
                None,
            )
        })
}

/// The class binding of a lifted set (:321-344 for a non-root set, :387-407 otherwise): Relational for a Relational
/// set, Operation for a union or inheritance, Pure for everything else (a Relation-function set included). The
/// arguments in Java's order; `aggregate_views` is the Relational binding's (`List.of()` for a non-root set,
/// `AggregateViewLift.facts` otherwise), read between the source stamp and the pins.
fn class_binding(
    cm: &ClassMapping,
    root: bool,
    function_fqn: &JStr,
    declared_keys: &HashMap<JStr, Arc<DeclaredKeys>>,
    ledger: &MappingLedger,
    aggregate_views: impl FnOnce(
        &ClassMappingRelational,
    ) -> Result<Vec<Arc<MappingDefinitionClassBindingAggregateViewFacts>>, Thrown>,
) -> Result<MappingDefinitionClassBinding, Thrown> {
    if let ClassMapping::ClassMappingRelational(r_src) = cm {
        let primary_key_columns = list(declared_primary_key_columns(cm));
        let declared = declared_keys
            .get(&set_key_facts::set_key(r_src))
            .cloned()
            .unwrap_or_else(DeclaredKeys::none);
        let source = relational_source_of(r_src)?;
        let aggregate_views = list(aggregate_views(r_src)?);
        let property_pins = property_pins_of(r_src).freeze_with(list);
        return Ok(MappingDefinitionClassBinding::MappingDefinitionClassBindingRelational(
            Arc::new(MappingDefinitionClassBindingRelational {
                class_fqn: cm.class_name().clone(),
                set_id: cm.set_id().cloned(),
                extends_set_id: cm.extends_set_id().cloned(),
                root,
                function_fqn: function_fqn.clone(),
                primary_key_columns,
                declared,
                source,
                aggregate_views,
                property_pins,
            }),
        ));
    }
    if is_operation(cm) {
        let primary_key_columns = list(declared_primary_key_columns(cm));
        let member_set_ids = ledger
            .operation_members
            .get(&ResolvedMapping::id_of(cm))
            .cloned()
            .unwrap_or_default();
        return Ok(MappingDefinitionClassBinding::MappingDefinitionClassBindingOperation(
            Arc::new(MappingDefinitionClassBindingOperation {
                class_fqn: cm.class_name().clone(),
                set_id: cm.set_id().cloned(),
                extends_set_id: cm.extends_set_id().cloned(),
                root,
                function_fqn: function_fqn.clone(),
                primary_key_columns,
                inheritance: matches!(cm, ClassMapping::ClassMappingInheritance(_)),
                member_set_ids: list(member_set_ids),
            }),
        ));
    }
    Ok(MappingDefinitionClassBinding::MappingDefinitionClassBindingPure(
        Arc::new(MappingDefinitionClassBindingPure {
            class_fqn: cm.class_name().clone(),
            set_id: cm.set_id().cloned(),
            extends_set_id: cm.extends_set_id().cloned(),
            root,
            function_fqn: function_fqn.clone(),
            primary_key_columns: list(declared_primary_key_columns(cm)),
        }),
    ))
}

/// `new PoisonKey.ForClass(classFqn)`.
fn class_key(class_fqn: &JStr) -> PoisonKey {
    PoisonKey::PoisonKeyForClass(Arc::new(PoisonKeyForClass {
        class_fqn: class_fqn.clone(),
    }))
}

/// `new PoisonKey.ForSet(cm.className(), ResolvedMapping.idOf(cm))`.
fn set_key(cm: &ClassMapping) -> PoisonKey {
    PoisonKey::PoisonKeyForSet(Arc::new(PoisonKeyForSet {
        class_fqn: cm.class_name().clone(),
        set_id: ResolvedMapping::id_of(cm),
    }))
}

/// `new PoisonKey.ForAssociation(associationFqn)`.
fn association_key(association_fqn: JStr) -> PoisonKey {
    PoisonKey::PoisonKeyForAssociation(Arc::new(PoisonKeyForAssociation { association_fqn }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lite_model::text::j;

    #[test]
    fn a_wall_is_the_first_line_of_an_attributed_model_exception_first_kept() {
        let e = Thrown::model(Phase::Normalize, j("first\nsecond"), Some(&j("a::M")));
        // strict: rethrown
        assert_eq!(wall(None, e.clone()), Err(e.clone()));
        // tolerant: the first line, under its element, the first wall kept
        let mut sink = LinkedMap::new();
        assert_eq!(wall(Some(&mut sink), e), Ok(()));
        let again = Thrown::model(Phase::Normalize, j("other"), Some(&j("a::M")));
        assert_eq!(wall(Some(&mut sink), again), Ok(()));
        assert_eq!(sink.iter().cloned().collect::<Vec<_>>(), vec![(j("a::M"), j("first"))]);
        // one that names no element is rethrown even under a sink
        let anonymous = Thrown::model(Phase::Normalize, j("x"), None);
        assert_eq!(wall(Some(&mut sink), anonymous.clone()), Err(anonymous));
    }
}
