// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `MappingPrePass` (MappingPrePass.java at ad86fa895): every legacy mapping's RESOLVED record, element order --
//! surface (JSON identity sets) -> M2M `~src` cycles -> declared keys -> extends -> implicit inheritance -> store refs
//! -> implicit operation sets; then the validation (a strict build rejects the first invalid set IN DECLARATION ORDER,
//! a module build poisons them by set id). A mapping whose pre-pass fails is walled (first line, attributed) under a
//! tolerant build, thrown under a strict one.
//!
//! The M2M index is a `LinkedHashMap` in Java now (declaration order, register row S380: the cycle's text starts at
//! the first-declared class of the first chain that closes) -- a `LinkedMap` here. RE-EXPRESSED (census §4): :265's
//! `next != pcm` is `pcm.source_class != pcm.class_name` (the index holds one set per class, and every set walked
//! comes from it). The E-local include facts are a parameter here (where Java asks `MappingClosures.of(model)`).

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use lite_error::Phase;
use lite_model::linked::{LinkedMap, list};
use lite_model::{
    ClassMapping, ClassMappingPure, ClassMappingRelational, JStr, LegacyMappingDefinition,
    MappingDefinitionClassBindingDeclaredKeys as DeclaredKeys, PackageableElement, ParsedModel, PropertyMapping, cat,
};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::implicit_inheritance;
use crate::jdk::LinkedSet;
use crate::lifted_views::LiftedViews;
use crate::mapping_closures::MappingClosures;
use crate::mapping_normalizer::with_element;
use crate::mapping_validation;
use crate::resolved_mapping::ResolvedMapping;
use crate::set_key_facts::{declared_keys_of, set_key};
use crate::store_substitution_rewrite;
use crate::union_synthesis::pm_identity;

/// Java's `String.join(" -> ", items)`.
fn arrows<'s>(items: impl Iterator<Item = &'s JStr>) -> JStr {
    let mut out: Vec<u16> = Vec::new();
    for (i, s) in items.enumerate() {
        if i > 0 {
            out.extend(" -> ".encode_utf16());
        }
        out.extend_from_slice(&s.0);
    }
    JStr(out)
}

/// `MappingPrePass.run` (:47): each legacy mapping's resolved record (absent when walled), element order.
pub fn run<'a>(
    parsed: &ParsedModel,
    model: &ModelIndex,
    closures: &'a MappingClosures<'a>,
    mut wall_sink: Option<&mut LinkedMap<JStr, JStr>>,
    views: &'a LiftedViews<'a>,
) -> Result<LinkedMap<JStr, ResolvedMapping<'a>>, Thrown> {
    let strict = wall_sink.is_none();
    let mut pre: LinkedMap<JStr, ResolvedMapping<'a>> = LinkedMap::new();
    for el in parsed.elements.iter() {
        let PackageableElement::LegacyMappingDefinition(md) = el else {
            continue;
        };
        let attempt = with_element(&md.qualified_name, || {
            let r = pre_pass(md, model, closures, views)?;
            // VALIDATION before synthesis (step 5): the translator records every invalid set; THE DRIVER'S policy
            // (B4) -- a strict build rejects the first IN DECLARATION ORDER, a module build poisons them (by set id:
            // the record survives the construction steps that rebuild the sets)
            let invalid = mapping_validation::run(&r, model)?;
            if strict && let Some((_, first)) = invalid.iter().next() {
                return Err(first.clone());
            }
            let mut reasons: LinkedMap<JStr, JStr> = LinkedMap::new();
            for (set_id, e) in invalid.iter() {
                reasons.put(set_id.clone(), e.message_or_null());
            }
            Ok(r.with_invalid(reasons))
        });
        match attempt {
            Ok(r) => {
                pre.put(md.qualified_name.clone(), r);
            }
            Err(e) if e.is_model() => match (wall_sink.as_deref_mut(), e.element()) {
                (Some(sink), Some(element)) => {
                    sink.put_if_absent(element, e.first_line());
                }
                _ => return Err(e),
            },
            Err(e) => return Err(e),
        }
    }
    Ok(pre)
}

/// `MappingPrePass.prePass` (:84): one mapping's construction, every step a rewrite of the record under construction.
pub fn pre_pass<'a>(
    authored: &Arc<LegacyMappingDefinition>,
    model: &ModelIndex,
    closures: &'a MappingClosures<'a>,
    views: &'a LiftedViews<'a>,
) -> Result<ResolvedMapping<'a>, Thrown> {
    let surface = closures.surface(authored);
    detect_m2m_cycles(&surface)?;
    // the sets' OWN key text, captured BEFORE the extends pre-pass merges the parent's in (metamodel facts,
    // ClassBinding.declared)
    let mut declared_keys: HashMap<JStr, Arc<DeclaredKeys>> = HashMap::new();
    for cm0 in surface.class_mappings.iter() {
        if let ClassMapping::ClassMappingRelational(r0) = cm0 {
            declared_keys.insert(set_key(r0), declared_keys_of(r0));
            // an AggregationAware set's aggregate views' own key text too (2026-11-15, the normalizer batch, item 2,
            // ASK 13: they answered NONE always)
            if let Some(agg) = &r0.aggregation {
                for v in agg.views.iter() {
                    declared_keys.insert(set_key(&v.set), declared_keys_of(&v.set));
                }
            }
        }
    }
    // ONE construction (B3.3): every step rewrites the record under construction and asks its closure questions of
    // that record
    let mut r = ResolvedMapping::new(
        surface.clone(),
        surface.clone(),
        Rc::new(declared_keys),
        Rc::new(LinkedMap::new()),
        closures.closure(&surface.qualified_name),
        views,
    );
    r = r.with_mapping(resolve_extends(&r, model)?);
    r = r.with_mapping(implicit_inheritance::apply(&r, model)?);
    // IMPORT-SCOPE store-ref qualification (StoreSubstitutionRewrite.qualifyStoreRefs)
    r = r.with_mapping(store_substitution_rewrite::qualify_store_refs(r.raw(), model)?);
    // implicit inheritance OPS for unmapped routed targets (association ends, routed class-typed properties) -- must
    // precede the multi-hop injection (op visibility)
    Ok(r.with_mapping(implicit_inheritance::implicit_ops_for_routed_targets(&r, model)?))
}

/// `MappingPrePass.resolveExtends` (:126): `extends [parentSetId]` flattened into each Relational child (set ids
/// resolved within the mapping and its includes); loud for a Pure child. (Java's `model` parameter is unread.)
pub fn resolve_extends(r: &ResolvedMapping<'_>, _model: &ModelIndex) -> Result<Arc<LegacyMappingDefinition>, Thrown> {
    let md = r.raw();
    let any = md.class_mappings.iter().any(|cm| cm.extends_set_id().is_some());
    if !any {
        return Ok(md.clone());
    }
    // set-ids resolve within this mapping AND its includes (transitive, own definitions win) -- extends [set] across
    // an include is the union::extend corpus family's normal shape
    let by_set_id = r.visible_sets()?;
    let mut rewritten: Vec<ClassMapping> = Vec::with_capacity(md.class_mappings.len());
    for cm in md.class_mappings.iter() {
        match (cm, cm.extends_set_id()) {
            (_, None) => rewritten.push(cm.clone()),
            (ClassMapping::ClassMappingRelational(rcm), Some(_)) => {
                rewritten.push(ClassMapping::ClassMappingRelational(flatten_extends(
                    rcm,
                    &by_set_id,
                    &mut LinkedSet::new(),
                    md,
                )?));
            }
            // Pure (M2M) extends is not covered by §5.2.3; reject loudly rather than silently ignore the inheritance
            (_, Some(ext)) => {
                return Err(Thrown::not_implemented(cat!(
                    "Class mapping for '",
                    cm.class_name(),
                    "' uses extends [",
                    ext,
                    "] on ",
                    kind_of(cm),
                    " class mapping; only Relational extends is supported. Mapping=",
                    &md.qualified_name
                )));
            }
        }
    }
    Ok(Arc::new(md.with_class_mappings(list(rewritten))))
}

/// `MappingPrePass.flattenExtends` (:160): one child's `extends` chain merged (parent PMs first, the child's by
/// `(name, targetSetId)` overriding; table attributes inherit-if-absent); loud for a cycle (`chain`'s order printed),
/// a missing parent or a non-Relational one.
pub fn flatten_extends(
    child: &ClassMappingRelational,
    by_set_id: &LinkedMap<JStr, ClassMapping>,
    chain: &mut LinkedSet<JStr>,
    md: &LegacyMappingDefinition,
) -> Result<Arc<ClassMappingRelational>, Thrown> {
    // both callers pass a child that extends (Java's null id would find no parent: "set id 'null'")
    let parent_set_id = child.extends_set_id.clone().unwrap_or_else(|| JStr::from("null"));
    if !chain.add(parent_set_id.clone()) {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "Circular 'extends' chain in mapping '",
                &md.qualified_name,
                "': ",
                arrows(chain.iter()),
                " -> ",
                &parent_set_id
            ),
            None,
        ));
    }
    let parent = if child.extends_set_id.is_some() {
        by_set_id.get(&parent_set_id)
    } else {
        None
    };
    let Some(parent) = parent else {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "Class mapping for '",
                &child.class_name,
                "' extends [",
                &parent_set_id,
                "] but no class mapping with set id '",
                &parent_set_id,
                "' exists in mapping=",
                &md.qualified_name
            ),
            None,
        ));
    };
    let ClassMapping::ClassMappingRelational(parent_rcm) = parent else {
        return Err(Thrown::not_implemented(cat!(
            "Class mapping for '",
            &child.class_name,
            "' extends [",
            &parent_set_id,
            "] which is not a Relational mapping; only Relational extends is supported. Mapping=",
            &md.qualified_name
        )));
    };
    let flat_parent = if parent_rcm.extends_set_id.is_some() {
        flatten_extends(parent_rcm, by_set_id, chain, md)?
    } else {
        parent_rcm.clone()
    };
    // Parent PMs first (declaration order), child overrides by property IDENTITY = (name, targetSetId): a routed
    // property's per-set duplicates are DISTINCT mappings (audit 11). prop[setId] routes do NOT inherit specially;
    // table-level attributes INHERIT-IF-ABSENT, child REPLACES (never ANDs) -- legend-pure resolveFilter /
    // resolveGroupBy / resolveDistinct and the pk priority ladder
    let mut merged: LinkedMap<JStr, PropertyMapping> = LinkedMap::new();
    for pm in flat_parent.property_mappings.iter() {
        merged.put(pm_identity(pm), pm.clone());
    }
    for pm in child.property_mappings.iter() {
        merged.put(pm_identity(pm), pm.clone());
    }
    Ok(Arc::new(ClassMappingRelational {
        class_name: child.class_name.clone(),
        set_id: child.set_id.clone(),
        extends_set_id: child.extends_set_id.clone(),
        root: child.root,
        main_table: child.main_table.clone().or_else(|| flat_parent.main_table.clone()),
        filter: child.filter.clone().or_else(|| flat_parent.filter.clone()),
        distinct: child.distinct || flat_parent.distinct,
        group_by: if !child.group_by.is_empty() {
            child.group_by.clone()
        } else {
            flat_parent.group_by.clone()
        },
        primary_key: if !child.primary_key.is_empty() {
            child.primary_key.clone()
        } else {
            flat_parent.primary_key.clone()
        },
        property_mappings: list(merged.iter().map(|(_, pm)| pm.clone()).collect()),
        source_url: child.source_url.clone(),
        property_target_sets: child.property_target_sets.clone(),
        aggregation: child.aggregation.clone(),
    }))
}

/// `MappingPrePass.detectM2MCycles` (:229): loud for an M2M `~src` cycle (`A ~src B`, `B ~src A`, or longer).
pub fn detect_m2m_cycles(md: &LegacyMappingDefinition) -> Result<(), Thrown> {
    // the walk starts from each target class in DECLARATION order (a later Pure set of the same class replaces the
    // value, its slot kept): register row S380
    let mut pure_by_target: LinkedMap<JStr, Arc<ClassMappingPure>> = LinkedMap::new();
    for cm in md.class_mappings.iter() {
        if let ClassMapping::ClassMappingPure(pcm) = cm {
            pure_by_target.put(pcm.class_name.clone(), pcm.clone());
        }
    }
    for (_, root) in pure_by_target.iter() {
        let mut visiting: LinkedSet<JStr> = LinkedSet::new();
        walk_m2m_chain(root, &pure_by_target, &mut visiting, md)?;
    }
    Ok(())
}

/// `MappingPrePass.walkM2MChain` (:249): one chain from `pcm`; a self-sourced set is a leaf (module note).
pub fn walk_m2m_chain(
    pcm: &Arc<ClassMappingPure>,
    pure_by_target: &LinkedMap<JStr, Arc<ClassMappingPure>>,
    visiting: &mut LinkedSet<JStr>,
    md: &LegacyMappingDefinition,
) -> Result<(), Thrown> {
    if !visiting.add(pcm.class_name.clone()) {
        return Err(Thrown::model(
            Phase::Normalize,
            cat!(
                "Circular M2M ~src chain detected in mapping '",
                &md.qualified_name,
                "': ",
                arrows(visiting.iter()),
                " -> ",
                &pcm.class_name
            ),
            None,
        ));
    }
    let next = pcm.source_class.as_ref().and_then(|s| pure_by_target.get(s));
    // SELF-SOURCED Pure mapping (~src X on X): the engine's identity / pass-through idiom -- a self-edge is a LEAF,
    // not a cycle. Java's `next != pcm` (:265), RE-EXPRESSED: the index holds one set per class and `pcm` came from
    // it, so `next` is `pcm` exactly when the set is sourced from its own class
    if let Some(next) = next
        && pcm.source_class.as_ref() != Some(&pcm.class_name)
    {
        walk_m2m_chain(next, pure_by_target, visiting, md)?;
    }
    visiting.remove(&pcm.class_name);
    Ok(())
}

/// `MappingPrePass.kindOf` (:272): a class mapping's kind, as its grammar names it (for a message).
pub fn kind_of(cm: &ClassMapping) -> &'static str {
    match cm {
        ClassMapping::ClassMappingRelational(_) => "a Relational",
        ClassMapping::ClassMappingPure(_) => "a Pure (M2M)",
        ClassMapping::ClassMappingUnion(_) => "an Operation (union)",
        ClassMapping::ClassMappingInheritance(_) => "an Operation (inheritance)",
        ClassMapping::ClassMappingRelationFunction(_) => "a Relation",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use lite_model::linked::empty;
    use lite_model::text::j;

    use crate::test_support::{cm, column_pm, index, legacy, mapping, model, relational, union};

    const MODEL: &str = r#"
###Pure
Class m::P
{
  name: String[1];
}
Class m::S
{
  name: String[1];
}
Class m::A
{
  name: String[1];
}
Class m::B
{
  name: String[1];
}

###Mapping
Mapping m::Ok
(
  *m::P[u]: Operation
  {
    meta::pure::router::operations::union_OperationSetImplementation_1__SetImplementation_MANY_(p1, nope)
  }
  m::P[p1]: Pure
  {
    ~src m::S
  }
)
Mapping m::Cyc
(
  m::A: Pure
  {
    ~src m::B
  }
  m::B: Pure
  {
    ~src m::A
  }
)
Mapping m::Missing
(
  include m::Nope
)
"#;

    fn pure(class: &str, source: Option<&str>) -> ClassMapping {
        ClassMapping::ClassMappingPure(Arc::new(ClassMappingPure {
            class_name: j(class),
            set_id: None,
            extends_set_id: None,
            root: false,
            source_class: source.map(j),
            filter: None,
            property_bindings: empty(),
        }))
    }

    fn cycle_text(md: &LegacyMappingDefinition) -> Option<JStr> {
        detect_m2m_cycles(md).err().and_then(|e| {
            assert!(e.is_model());
            assert_eq!(e.phase(), Some(Phase::Normalize));
            assert_eq!(e.element(), None);
            e.message()
        })
    }

    #[test]
    fn an_m2m_cycle_is_spelled_from_the_first_declared_class() {
        let ab = legacy("m::M", vec![pure("m::A", Some("m::B")), pure("m::B", Some("m::A"))]);
        assert_eq!(
            cycle_text(&ab),
            Some(j(
                "Circular M2M ~src chain detected in mapping 'm::M': m::A -> m::B -> m::A"
            ))
        );
        // declaration order, not hash order (register row S380)
        let ba = legacy("m::M", vec![pure("m::B", Some("m::A")), pure("m::A", Some("m::B"))]);
        assert_eq!(
            cycle_text(&ba),
            Some(j(
                "Circular M2M ~src chain detected in mapping 'm::M': m::B -> m::A -> m::B"
            ))
        );
        let longer = legacy(
            "m::M",
            vec![
                pure("m::A", Some("m::B")),
                pure("m::B", Some("m::C")),
                pure("m::C", Some("m::A")),
            ],
        );
        assert_eq!(
            cycle_text(&longer),
            Some(j(
                "Circular M2M ~src chain detected in mapping 'm::M': m::A -> m::B -> m::C -> m::A"
            ))
        );
    }

    #[test]
    fn a_self_sourced_set_a_chain_out_and_a_shared_source_are_no_cycles() {
        assert_eq!(cycle_text(&legacy("m::M", vec![pure("m::A", Some("m::A"))])), None);
        assert_eq!(
            cycle_text(&legacy(
                "m::M",
                vec![pure("m::A", Some("m::B")), pure("m::B", Some("m::X"))]
            )),
            None
        );
        // two chains through one set: the walk releases the set after each
        let diamond = legacy(
            "m::M",
            vec![
                pure("m::A", Some("m::C")),
                pure("m::B", Some("m::C")),
                pure("m::C", None),
            ],
        );
        assert_eq!(cycle_text(&diamond), None);
        // a later Pure set of a class replaces the earlier as the walk's set (its slot kept)
        let replaced = legacy(
            "m::M",
            vec![
                pure("m::A", Some("m::B")),
                pure("m::B", Some("m::Z")),
                pure("m::A", Some("m::A")),
            ],
        );
        assert_eq!(cycle_text(&replaced), None);
    }

    #[test]
    fn each_kind_is_named_as_its_grammar_names_it() {
        assert_eq!(kind_of(&cm(relational("m::A", None, Vec::new()))), "a Relational");
        assert_eq!(kind_of(&pure("m::A", None)), "a Pure (M2M)");
        assert_eq!(kind_of(&union("m::A", None, &[])), "an Operation (union)");
        let ih = ClassMapping::ClassMappingInheritance(Arc::new(lite_model::ClassMappingInheritance {
            class_name: j("m::A"),
            set_id: None,
            extends_set_id: None,
            root: false,
        }));
        assert_eq!(kind_of(&ih), "an Operation (inheritance)");
    }

    fn extending(class: &str, id: &str, parent: &str) -> ClassMappingRelational {
        ClassMappingRelational {
            extends_set_id: Some(j(parent)),
            ..relational(class, Some(id), vec![column_pm("name", "T", "NAME")])
        }
    }

    fn by_id(cms: &[ClassMapping]) -> LinkedMap<JStr, ClassMapping> {
        LinkedMap::of(cms.iter().map(|c| (ResolvedMapping::id_of(c), c.clone())))
    }

    #[test]
    fn an_extends_chain_is_loud_for_a_cycle_a_missing_parent_and_a_non_relational_parent() {
        let md = legacy("m::M", Vec::new());
        let c1 = extending("m::A", "c1", "p");
        let p = cm(extending("m::A", "p", "c1"));
        let e = flatten_extends(&c1, &by_id(&[cm(c1.clone()), p]), &mut LinkedSet::new(), &md).unwrap_err();
        assert!(e.is_model() && e.phase() == Some(Phase::Normalize) && e.element().is_none());
        assert_eq!(
            e.message(),
            Some(j("Circular 'extends' chain in mapping 'm::M': p -> c1 -> p"))
        );

        let orphan = extending("m::A", "c", "nope");
        let e = flatten_extends(&orphan, &by_id(&[]), &mut LinkedSet::new(), &md).unwrap_err();
        assert!(e.is_model());
        assert_eq!(
            e.message(),
            Some(j(
                "Class mapping for 'm::A' extends [nope] but no class mapping with set id 'nope' exists in \
                 mapping=m::M"
            ))
        );

        let over_pure = extending("m::A", "c", "a");
        let parent = ClassMapping::ClassMappingPure(Arc::new(ClassMappingPure {
            set_id: Some(j("a")),
            ..match pure("m::A", None) {
                ClassMapping::ClassMappingPure(x) => (*x).clone(),
                _ => unreachable!(),
            }
        }));
        let e = flatten_extends(&over_pure, &by_id(&[parent]), &mut LinkedSet::new(), &md).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "Class mapping for 'm::A' extends [a] which is not a Relational mapping; only Relational extends is \
                 supported. Mapping=m::M"
            ))
        );
    }

    /// A record over `md` (its own surface), as the pre-pass builds it.
    fn resolved<'a>(
        md: Arc<LegacyMappingDefinition>,
        closures: &'a MappingClosures<'a>,
        views: &'a LiftedViews<'a>,
    ) -> ResolvedMapping<'a> {
        let closure = closures.closure(&md.qualified_name);
        ResolvedMapping::new(
            md.clone(),
            md,
            Rc::new(HashMap::new()),
            Rc::new(LinkedMap::new()),
            closure,
            views,
        )
    }

    #[test]
    fn extends_reads_the_visible_sets_only_when_a_set_extends() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        // an include the model has not: never asked when no set extends -- the very mapping back
        let missing = mapping(&parsed, "m::Missing");
        let r = resolved(missing.clone(), &closures, &views);
        assert!(lite_base::same(&resolve_extends(&r, &idx).unwrap(), &missing));
        // ... and loud once one does
        let extends = Arc::new(LegacyMappingDefinition {
            class_mappings: list(vec![cm(extending("m::A", "c", "p"))]),
            ..(*missing).clone()
        });
        let e = resolve_extends(&resolved(extends, &closures, &views), &idx).unwrap_err();
        assert_eq!(e.message(), Some(j("Can't find mapping 'm::Nope'")));
        // a Pure set that extends: the roadmap's refusal, naming its kind
        let pure_child = Arc::new(LegacyMappingDefinition {
            class_mappings: list(vec![ClassMapping::ClassMappingPure(Arc::new(ClassMappingPure {
                extends_set_id: Some(j("p1")),
                ..match pure("m::P", Some("m::S")) {
                    ClassMapping::ClassMappingPure(x) => (*x).clone(),
                    _ => unreachable!(),
                }
            }))]),
            ..legacy("m::Ok", Vec::new())
        });
        let e = resolve_extends(&resolved(pure_child, &closures, &views), &idx).unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "Class mapping for 'm::P' uses extends [p1] on a Pure (M2M) class mapping; only Relational extends \
                 is supported. Mapping=m::Ok"
            ))
        );
    }

    #[test]
    fn a_strict_build_throws_the_first_invalid_set_and_a_tolerant_one_records_it() {
        let parsed = model(MODEL);
        let only_ok = ParsedModel {
            elements: list(
                parsed
                    .elements
                    .iter()
                    .filter(|e| e.qualified_name() != &j("m::Cyc") && e.qualified_name() != &j("m::Missing"))
                    .cloned()
                    .collect(),
            ),
            ..parsed.clone()
        };
        let idx = index(&only_ok);
        let views = LiftedViews::new(&only_ok, &idx);
        let text = j("Can't find class mapping 'nope' in mapping 'm::Ok'");

        let closures = MappingClosures::new(&idx);
        let e = run(&only_ok, &idx, &closures, None, &views)
            .err()
            .expect("strict: thrown");
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Mapping));
        assert_eq!(e.element(), Some(j("m::Ok")));
        assert_eq!(e.message(), Some(text.clone()));

        let closures = MappingClosures::new(&idx);
        let mut walls = LinkedMap::new();
        let pre = run(&only_ok, &idx, &closures, Some(&mut walls), &views).unwrap();
        assert!(walls.is_empty());
        let r = pre.get(&j("m::Ok")).expect("recorded, not walled");
        let u = r
            .class_mappings()
            .iter()
            .find(|c| matches!(c, ClassMapping::ClassMappingUnion(_)))
            .cloned()
            .unwrap();
        assert_eq!(r.invalid_reason(&u), Some(&text));
        let p1 = r.set(Some(&j("p1"))).unwrap().unwrap();
        assert_eq!(r.invalid_reason(&p1), None);
    }

    #[test]
    fn a_failed_pre_pass_is_walled_with_its_first_line_under_a_tolerant_build() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let views = LiftedViews::new(&parsed, &idx);
        let closures = MappingClosures::new(&idx);
        let mut walls = LinkedMap::new();
        let pre = run(&parsed, &idx, &closures, Some(&mut walls), &views).unwrap();
        let walled: Vec<(JStr, JStr)> = walls.iter().cloned().collect();
        assert_eq!(
            walled,
            vec![
                (
                    j("m::Cyc"),
                    j("Circular M2M ~src chain detected in mapping 'm::Cyc': m::A -> m::B -> m::A")
                ),
                // the include walk the implicit inheritance asks first
                (j("m::Missing"), j("Can't find mapping 'm::Nope'")),
            ]
        );
        let kept: Vec<JStr> = pre.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(kept, vec![j("m::Ok")]);
        // strict: the first mapping's failure, attributed to it
        let closures = MappingClosures::new(&idx);
        let e = run(&parsed, &idx, &closures, None, &views)
            .err()
            .expect("strict: thrown");
        assert_eq!(e.element(), Some(j("m::Ok")));
    }
}
