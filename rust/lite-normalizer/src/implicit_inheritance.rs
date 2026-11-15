// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ImplicitInheritance` (ImplicitInheritance.java at ad86fa895): IMPLICIT same-extent inheritance (an ancestor's
//! unqualified PMs merged into a child set over the same main table) and implicit `Inheritance` operations for
//! unmapped routed targets; "Can't find association". Java iterates two `HashMap`s (:43, :123) with no visible effect
//! (a size-1 test; a membership set): iterate `visibleSets` in its own order.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use lite_error::Phase;
use lite_front::records as p;
use lite_model::linked::{LinkedMap, list};
use lite_model::{
    AssociationMapping, ClassMapping, ClassMappingInheritance, ClassMappingRelational, JStr, LegacyMappingDefinition,
    LegacyMappingDefinitionTableReference as TableRef, PropertyMapping, PropertyMappingJoin, cat,
};
use lite_names::ModelIndex;

use crate::association_synthesis::{association_target_class, resolve_association};
use crate::error::Thrown;
use crate::jdk::LinkedSet;
use crate::mapping_normalizer::infer_main_table_quiet;
use crate::resolved_mapping::ResolvedMapping;
use crate::union_synthesis::collect_routed_joins;

/// `ImplicitInheritance.apply` (:38): the mapping with each eligible child set's inherited PMs merged.
pub fn apply(resolved: &ResolvedMapping<'_>, model: &ModelIndex) -> Result<Arc<LegacyMappingDefinition>, Thrown> {
    let md = resolved.raw();
    let by_set_id = resolved.visible_sets()?;
    // class fqn -> its Relational mappings in scope
    let mut by_class: HashMap<JStr, Vec<Arc<ClassMappingRelational>>> = HashMap::new();
    for (_, cm) in by_set_id.iter() {
        if let ClassMapping::ClassMappingRelational(r) = cm {
            by_class.entry(r.class_name.clone()).or_default().push(r.clone());
        }
    }
    let mut any = false;
    let mut rewritten: Vec<ClassMapping> = Vec::with_capacity(md.class_mappings.len());
    for cm in md.class_mappings.iter() {
        let ClassMapping::ClassMappingRelational(child) = cm else {
            rewritten.push(cm.clone());
            continue;
        };
        if child.extends_set_id.is_some() {
            rewritten.push(cm.clone());
            continue;
        }
        let child_main = match &child.main_table {
            Some(t) => Some(t.clone()),
            None => infer_main_table_quiet(child)?,
        };
        let Some(child_main) = child_main else {
            rewritten.push(cm.clone());
            continue;
        };
        let Some(ancestor) = nearest_mapped_ancestor(&child_main, &child.class_name, model, &by_class)? else {
            rewritten.push(cm.clone());
            continue;
        };
        let own: HashSet<&JStr> = child.property_mappings.iter().map(|pm| pm.property_name()).collect();
        let mut merged: Vec<PropertyMapping> = child.property_mappings.to_vec();
        let mut added = false;
        for pm in ancestor.property_mappings.iter() {
            let routed = matches!(pm, PropertyMapping::PropertyMappingJoin(j) if j.target_set_id.is_some());
            if !own.contains(pm.property_name()) && !routed {
                merged.push(pm.clone());
                added = true;
            }
        }
        if !added {
            rewritten.push(cm.clone());
            continue;
        }
        any = true;
        rewritten.push(ClassMapping::ClassMappingRelational(Arc::new(ClassMappingRelational {
            property_mappings: list(merged),
            ..(**child).clone()
        })));
    }
    Ok(if any {
        Arc::new(md.with_class_mappings(list(rewritten)))
    } else {
        md.clone()
    })
}

/// `ImplicitInheritance.implicitOpsForRoutedTargets` (:118): the mapping with an `Inheritance` operation appended for
/// each unmapped association end or routed property type with a routed strict-subclass set.
pub fn implicit_ops_for_routed_targets(
    r: &ResolvedMapping<'_>,
    model: &ModelIndex,
) -> Result<Arc<LegacyMappingDefinition>, Thrown> {
    let md = r.raw();
    let knowledge = model.knowledge();
    let by_set_id = r.visible_sets()?;
    let mut mapped_classes: HashSet<JStr> = HashSet::new();
    for (_, cm) in by_set_id.iter() {
        mapped_classes.insert(cm.class_name().clone());
    }
    let mut implied: LinkedSet<JStr> = LinkedSet::new();
    // (a) per-pair association mapping entries
    for am in md.association_mappings.iter() {
        let AssociationMapping::AssociationMappingRelational(rel) = am else {
            continue;
        };
        // a mapping of an association the model has not is that model's error, in legend-engine's words
        let Some(ad) = resolve_association(model, r, am) else {
            return Err(Thrown::model(
                Phase::Mapping,
                cat!("Can't find association '", am.association_name(), "'"),
                None,
            ));
        };
        for apm in rel.property_mappings.iter() {
            let tgt_set = match &apm.body {
                PropertyMapping::PropertyMappingJoin(j) if j.target_set_id.is_some() => j.target_set_id.as_ref(),
                _ => apm.target_set_id.as_ref(),
            };
            let Some(tgt_set) = tgt_set else {
                continue;
            };
            let end = association_target_class(&ad, apm.property_name());
            let tgt = by_set_id.get(tgt_set);
            let (Some(end), Some(tgt)) = (end, tgt) else {
                continue;
            };
            if mapped_classes.contains(end) || tgt.class_name() == end || !knowledge.is_subtype(tgt.class_name(), end) {
                continue;
            }
            implied.add(end.clone());
        }
    }
    // (b) routed class-typed property mappings
    for cm in md.class_mappings.iter() {
        let ClassMapping::ClassMappingRelational(rcm) = cm else {
            continue;
        };
        let mut routed_by_prop: LinkedMap<JStr, Vec<Arc<PropertyMappingJoin>>> = LinkedMap::new();
        let mut owner_by_prop: LinkedMap<JStr, JStr> = LinkedMap::new();
        collect_routed_joins(
            &rcm.property_mappings,
            &rcm.class_name,
            r,
            model,
            &mut routed_by_prop,
            &mut owner_by_prop,
        )?;
        for (prop, joins) in routed_by_prop.iter() {
            // `MissProbe.miss`: an owner the model has not reads no property type
            let owner = knowledge.hierarchy_class(Some(owner_by_prop.get(prop).unwrap_or(&rcm.class_name)));
            let pt = match owner {
                None => None,
                Some(owner) => knowledge.property_type(Some(owner), prop)?,
            };
            let Some(p::TypeExpression::TypeExpressionNameRef(nr)) = pt else {
                continue;
            };
            if knowledge.hierarchy_class(Some(&nr.name)).is_none() || mapped_classes.contains(&nr.name) {
                continue;
            }
            for j in joins {
                let tgt = j.target_set_id.as_ref().and_then(|t| by_set_id.get(t));
                if let Some(tgt) = tgt
                    && tgt.class_name() != &nr.name
                    && knowledge.is_subtype(tgt.class_name(), &nr.name)
                {
                    implied.add(nr.name.clone());
                    break;
                }
            }
        }
    }
    if implied.is_empty() {
        return Ok(md.clone());
    }
    let mut rewritten: Vec<ClassMapping> = md.class_mappings.to_vec();
    for cls in implied.iter() {
        rewritten.push(ClassMapping::ClassMappingInheritance(Arc::new(
            ClassMappingInheritance {
                class_name: cls.clone(),
                set_id: None,
                extends_set_id: None,
                root: false,
            },
        )));
    }
    Ok(Arc::new(md.with_class_mappings(list(rewritten))))
}

/// `ImplicitInheritance.nearestMappedAncestor` (:200): the NEAREST ancestor with exactly ONE Relational set in scope
/// over the child's main table; none otherwise (ambiguity included).
pub fn nearest_mapped_ancestor(
    child_main: &TableRef,
    child_class: &JStr,
    model: &ModelIndex,
    by_class: &HashMap<JStr, Vec<Arc<ClassMappingRelational>>>,
) -> Result<Option<Arc<ClassMappingRelational>>, Thrown> {
    // ancestors nearest-first (the kernel's breadth-first order)
    for cls in model.knowledge().ancestors_and_self(child_class)? {
        if &cls == child_class {
            continue;
        }
        let mut cands: Vec<&Arc<ClassMappingRelational>> = Vec::new();
        for a in by_class.get(&cls).map(Vec::as_slice).unwrap_or_default() {
            let am = match &a.main_table {
                Some(t) => Some(t.clone()),
                None => infer_main_table_quiet(a)?,
            };
            if am.is_some_and(|am| *am == *child_main) {
                cands.push(a);
            }
        }
        if cands.len() == 1 {
            return Ok(Some(cands[0].clone()));
        }
        if !cands.is_empty() {
            return Ok(None); // ambiguous -- stay loud downstream
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::rc::Rc;

    use lite_model::linked::empty;
    use lite_model::text::j;
    use lite_model::{AssociationMappingCross, ClassMappingPure};

    use crate::lifted_views::LiftedViews;
    use crate::mapping_closures::MappingClosures;
    use crate::test_support::{cm, column_pm, index, join_pm, legacy, model, relational};

    const MODEL: &str = r#"
###Pure
Class m::Base
{
  name: String[1];
  code: String[1];
}
Class m::Kid extends m::Base
{
  age: Integer[1];
}
Class m::Grandkid extends m::Kid
{
  tag: String[1];
}
"#;

    fn over(table: &str, set: ClassMappingRelational) -> ClassMapping {
        cm(ClassMappingRelational {
            main_table: Some(Arc::new(TableRef {
                database: j("db"),
                table: j(table),
            })),
            ..set
        })
    }

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

    fn names(cm: &ClassMapping) -> Vec<JStr> {
        match cm {
            ClassMapping::ClassMappingRelational(r) => r
                .property_mappings
                .iter()
                .map(|pm| pm.property_name().clone())
                .collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn a_child_over_its_ancestors_table_inherits_the_unqualified_pms_it_does_not_map() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let base = over(
            "T",
            relational(
                "m::Base",
                Some("b"),
                vec![
                    column_pm("name", "T", "NAME"),
                    column_pm("code", "T", "CODE"),
                    join_pm("x", Some("s")),
                ],
            ),
        );
        let kid = over(
            "T",
            relational(
                "m::Kid",
                Some("k"),
                vec![column_pm("age", "T", "AGE"), column_pm("name", "T", "N2")],
            ),
        );
        // the nearest mapped ancestor serves a grandchild too (Kid is nearer than Base)
        let grandkid = over(
            "T",
            relational("m::Grandkid", Some("g"), vec![column_pm("tag", "T", "TAG")]),
        );
        let md = Arc::new(legacy("m::M", vec![base.clone(), kid, grandkid]));
        let out = apply(&resolved(md.clone(), &closures, &views), &idx).unwrap();
        assert!(!lite_base::same(&out, &md));
        // the ancestor's own set is untouched (the very record)
        assert_eq!(out.class_mappings[0], base);
        assert_eq!(names(&out.class_mappings[1]), vec![j("age"), j("name"), j("code")]);
        // the grandchild reads the kid's set as it was declared (the scope is the mapping's, not the rewrite's)
        assert_eq!(names(&out.class_mappings[2]), vec![j("tag"), j("age"), j("name")]);
    }

    #[test]
    fn another_table_an_ambiguous_ancestor_or_nothing_to_add_inherits_nothing() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let kid = over("T", relational("m::Kid", Some("k"), vec![column_pm("age", "T", "AGE")]));
        // another main table: distinct extents stay loud
        let elsewhere = Arc::new(legacy(
            "m::M",
            vec![
                over(
                    "U",
                    relational("m::Base", Some("b"), vec![column_pm("code", "U", "CODE")]),
                ),
                kid.clone(),
            ],
        ));
        assert!(lite_base::same(
            &apply(&resolved(elsewhere.clone(), &closures, &views), &idx).unwrap(),
            &elsewhere
        ));
        // two sets of the ancestor over the table: ambiguous
        let two = Arc::new(legacy(
            "m::M",
            vec![
                over(
                    "T",
                    relational("m::Base", Some("b1"), vec![column_pm("code", "T", "CODE")]),
                ),
                over(
                    "T",
                    relational("m::Base", Some("b2"), vec![column_pm("code", "T", "CODE")]),
                ),
                kid.clone(),
            ],
        ));
        assert!(lite_base::same(
            &apply(&resolved(two.clone(), &closures, &views), &idx).unwrap(),
            &two
        ));
        // only a routed PM to inherit: nothing added
        let routed = Arc::new(legacy(
            "m::M",
            vec![
                over("T", relational("m::Base", Some("b"), vec![join_pm("x", Some("s"))])),
                kid,
            ],
        ));
        assert!(lite_base::same(
            &apply(&resolved(routed.clone(), &closures, &views), &idx).unwrap(),
            &routed
        ));
    }

    #[test]
    fn no_relational_set_and_no_relational_association_mapping_implies_no_operation() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let closures = MappingClosures::new(&idx);
        let views = LiftedViews::new(&parsed, &idx);
        let pure = ClassMapping::ClassMappingPure(Arc::new(ClassMappingPure {
            class_name: j("m::Kid"),
            set_id: None,
            extends_set_id: None,
            root: false,
            source_class: Some(j("m::Base")),
            filter: None,
            property_bindings: empty(),
        }));
        let md = Arc::new(LegacyMappingDefinition {
            association_mappings: list(vec![AssociationMapping::AssociationMappingCross(Arc::new(
                AssociationMappingCross {
                    association_name: j("m::Nowhere"),
                    property_mappings2: empty(),
                },
            ))]),
            ..legacy("m::M", vec![pure])
        });
        let r = resolved(md.clone(), &closures, &views);
        assert!(lite_base::same(
            &implicit_ops_for_routed_targets(&r, &idx).unwrap(),
            &md
        ));
    }
}
