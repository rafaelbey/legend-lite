// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `MappingFacts` (MappingFacts.java at ad86fa895, ported line for line, EF_CENSUS §1: "the raw id and last-wins
//! included"): the SURFACE facts Phase F used to re-read off the authored mapping, computed once at Phase E and stamped
//! on the compiled mapping as `MappingDefinition.NormalizationFacts`. Each rule is the F-side reader's, verbatim, over
//! the mapping's surface. Members are matched by the RAW `setId()` (absent when the user wrote no brackets), not the
//! effective id, and `unionMembers` keeps the LAST set of a member id (ASK 13 lists both; ported as Java has them).

use lite_model::linked::LinkedMap;
use lite_model::{ClassMapping, JStr, LegacyMappingDefinition, PropertyMapping};

use crate::jdk::LinkedSet;

/// `MappingFacts.unionMembers` (:31): class -> an Operation union's member CLASSES in member order; absent when a
/// member set does not resolve within the mapping.
pub fn union_members(surface: &LegacyMappingDefinition) -> LinkedMap<JStr, Vec<JStr>> {
    let mut out: LinkedMap<JStr, Vec<JStr>> = LinkedMap::new();
    for cm in surface.class_mappings.iter() {
        let ClassMapping::ClassMappingUnion(u) = cm else {
            continue;
        };
        if out.contains_key(&u.class_name) {
            continue; // the reader took the FIRST union of the class
        }
        let mut members = Vec::new();
        let mut complete = true;
        for sid in u.member_set_ids.iter() {
            let mut member_class: Option<&JStr> = None;
            for m2 in surface.class_mappings.iter() {
                if m2.set_id() == Some(sid) {
                    member_class = Some(m2.class_name());
                }
            }
            let Some(member_class) = member_class else {
                complete = false;
                break;
            };
            members.push(member_class.clone());
        }
        if complete {
            out.put(u.class_name.clone(), members);
        }
    }
    out
}

/// `MappingFacts.routedTargetClasses` (:63): owner class -> property -> the ONE class every route of the property lands
/// on (the first Relational set of the owner with a routed answer; a set whose routes disagree yields no answer for
/// the owner at all -- the reader's early return).
pub fn routed_target_classes(surface: &LegacyMappingDefinition) -> LinkedMap<JStr, LinkedMap<JStr, JStr>> {
    let mut out: LinkedMap<JStr, LinkedMap<JStr, JStr>> = LinkedMap::new();
    let mut props_by_owner: LinkedMap<JStr, LinkedSet<JStr>> = LinkedMap::new();
    for cm in surface.class_mappings.iter() {
        if let ClassMapping::ClassMappingRelational(rcm) = cm {
            for pm in rcm.property_mappings.iter() {
                if let PropertyMapping::PropertyMappingJoin(j) = pm
                    && j.target_set_id.is_some()
                {
                    props_by_owner
                        .get_or_insert_with(rcm.class_name.clone(), LinkedSet::new)
                        .add(j.property_name.clone());
                }
            }
        }
    }
    for (owner, props) in props_by_owner.iter() {
        for prop in props.iter() {
            if let Some(answer) = routed_target_class(surface, owner, prop) {
                out.get_or_insert_with(owner.clone(), LinkedMap::new)
                    .put(prop.clone(), answer);
            }
        }
    }
    out
}

/// `MappingFacts.routedTargetClass` (:87).
fn routed_target_class(lm: &LegacyMappingDefinition, owner_class: &JStr, prop: &JStr) -> Option<JStr> {
    for cm in lm.class_mappings.iter() {
        let ClassMapping::ClassMappingRelational(rcm) = cm else {
            continue;
        };
        if &rcm.class_name != owner_class {
            continue;
        }
        let mut routed: Option<JStr> = None;
        for pm in rcm.property_mappings.iter() {
            if let PropertyMapping::PropertyMappingJoin(j) = pm
                && &j.property_name == prop
                && let Some(target) = &j.target_set_id
            {
                for m2 in lm.class_mappings.iter() {
                    if Some(target) == m2.set_id() {
                        if routed.as_ref().is_some_and(|r| r != m2.class_name()) {
                            return None;
                        }
                        routed = Some(m2.class_name().clone());
                    }
                }
            }
        }
        if routed.is_some() {
            return routed;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{cm, join_pm, legacy, relational, union};
    use lite_model::text::j;

    #[test]
    fn union_members_match_raw_ids_and_keep_the_last_set() {
        let a1 = relational("a::A", Some("m1"), vec![]);
        let b1 = relational("a::B", Some("m2"), vec![]);
        let b2 = relational("a::C", Some("m2"), vec![]);
        let u = union("a::U", Some("u"), &["m1", "m2"]);
        let missing = union("a::V", None, &["m1", "nope"]);
        let md = legacy("a::M", vec![cm(a1), cm(b1), cm(b2), u, missing]);
        let um = union_members(&md);
        let rows: Vec<(JStr, Vec<JStr>)> = um.iter().cloned().collect();
        assert_eq!(rows, vec![(j("a::U"), vec![j("a::A"), j("a::C")])]);
    }

    #[test]
    fn routed_target_classes_answer_one_class_or_none() {
        let owner = relational(
            "a::Firm",
            Some("f"),
            vec![
                join_pm("emps", Some("p1")),
                join_pm("emps", Some("p2")),
                join_pm("boss", Some("p1")),
            ],
        );
        let p1 = relational("a::Person", Some("p1"), vec![]);
        let p2 = relational("a::Person", Some("p2"), vec![]);
        let md = legacy("a::M", vec![cm(owner), cm(p1), cm(p2)]);
        let rt = routed_target_classes(&md);
        let firm = rt.get(&j("a::Firm")).expect("the owner's answers");
        assert_eq!(firm.get(&j("emps")), Some(&j("a::Person")));
        assert_eq!(firm.get(&j("boss")), Some(&j("a::Person")));
        // two routes landing on two classes: no answer for the property
        let owner2 = relational(
            "a::Firm",
            Some("f"),
            vec![join_pm("x", Some("p1")), join_pm("x", Some("q"))],
        );
        let q = relational("a::Other", Some("q"), vec![]);
        let md2 = legacy(
            "a::M",
            vec![cm(owner2), cm(relational("a::Person", Some("p1"), vec![])), cm(q)],
        );
        assert!(routed_target_classes(&md2).get(&j("a::Firm")).is_none());
    }
}
