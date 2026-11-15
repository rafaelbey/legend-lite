// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The model's reads the element compiler makes that the records do not carry yet (2026-11-15, the element slice's
//! step 3: docs/rust-port/EF_CENSUS.md §8): `NormalizationFacts`' keyed lookups, which `PureModelContext`'s mapping
//! fact readers make (PMC:513-565 at 18feff3fe; the census's PMC:488-535 are 7c438f7b3's numbers).
//!
//! Java's maps are `LinkedHashMap`s (a `Map` key is a `String` or a `PoisonKey` record, its equality derived); the
//! record holds each as an ordered `Arc<[(K, V)]>` whose keys are distinct (the normalizer's `MappingLedger` and
//! `MappingFacts` build them through [`LinkedMap`](crate::linked::LinkedMap)), so a `get` is the entry whose key is
//! equal ([`linked::get`]): the same answer, by the same equality.
//!
//! `DatabaseDefinition`'s lookups beyond the default schema (census §8: MI:606, :618, PMC:737-755, :882 at 18feff3fe)
//! need nothing here: every one Java makes is `ModelBuilder`'s (`findTableDefinition`, `findOwnTableDefinition`,
//! `findView` by name and by schema, `viewMainTable`, `findJoin`, `findDatabase`), which lite-names' `ModelIndex`
//! answers, and Java's `DatabaseDefinition` has no lookup of its own but `defaultSchemaTables` and
//! `defaultSchemaViews` (here since slice 1b: `database.rs`).

use std::sync::Arc;

use lite_front::jtypes::JStr;

use crate::linked;
use crate::records::{KeyThread, MappingDefinitionNormalizationFacts, PoisonKey};

impl MappingDefinitionNormalizationFacts {
    /// `unionMembers().get(classFqn)` (PMC:516): the member classes of the Operation union that maps `class_fqn`, in
    /// the union's order; none when the mapping maps it by no union.
    pub fn union_members_of(&self, class_fqn: &JStr) -> Option<&Arc<[JStr]>> {
        linked::get(&self.union_members, class_fqn)
    }

    /// `routedTargetClasses().getOrDefault(ownerClass, Map.of()).get(prop)` (PMC:523): the class a class-typed
    /// property of `owner_class` is routed to; none when the owner has no routed property, or not this one.
    pub fn routed_target_class(&self, owner_class: &JStr, prop: &JStr) -> Option<&JStr> {
        linked::get(&self.routed_target_classes, owner_class).and_then(|by_prop| linked::get(by_prop, prop))
    }

    /// `mixedUnions().get(classFqn)` (PMC:532): the members of the union mapping `class_fqn` whose kinds mix; none
    /// when it is no mixed union.
    pub fn mixed_union_of(&self, class_fqn: &JStr) -> Option<&Arc<[JStr]>> {
        linked::get(&self.mixed_unions, class_fqn)
    }

    /// `unionKeyThreads().get(classFqn)` (PMC:539): the key threads of the union mapping `class_fqn`; none when it
    /// threads none.
    pub fn union_key_threads_of(&self, class_fqn: &JStr) -> Option<&Arc<[Arc<KeyThread>]>> {
        linked::get(&self.union_key_threads, class_fqn)
    }

    /// `poisons().get(key)` (PMC:548, :555, :563: a class's, a set's, an association's `PoisonKey`): the poison text
    /// recorded under `key`; none when nothing is poisoned there.
    pub fn poison(&self, key: &PoisonKey) -> Option<&JStr> {
        linked::get(&self.poisons, key)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lite_front::jtypes::JStr;

    use crate::linked::list;
    use crate::records::{
        KeyThread, MappingDefinitionNormalizationFacts, PoisonKey, PoisonKeyForAssociation, PoisonKeyForClass,
        PoisonKeyForSet,
    };

    fn j(s: &str) -> JStr {
        JStr::from(s)
    }

    fn for_class(c: &str) -> PoisonKey {
        PoisonKey::PoisonKeyForClass(Arc::new(PoisonKeyForClass { class_fqn: j(c) }))
    }

    fn for_set(c: &str, s: &str) -> PoisonKey {
        PoisonKey::PoisonKeyForSet(Arc::new(PoisonKeyForSet {
            class_fqn: j(c),
            set_id: j(s),
        }))
    }

    fn for_association(a: &str) -> PoisonKey {
        PoisonKey::PoisonKeyForAssociation(Arc::new(PoisonKeyForAssociation { association_fqn: j(a) }))
    }

    fn facts() -> MappingDefinitionNormalizationFacts {
        let thread = Arc::new(KeyThread::new(j("id"), Some(j("Integer")), j("ID"), 0));
        MappingDefinitionNormalizationFacts {
            poisons: list(vec![
                (for_class("a::P"), j("class poison")),
                (for_set("a::P", "p1"), j("set poison")),
                (for_association("a::PA"), j("association poison")),
            ]),
            mixed_unions: list(vec![(j("a::U"), list(vec![j("a::A"), j("a::B")]))]),
            union_key_threads: list(vec![(j("a::U"), list(vec![thread]))]),
            union_members: list(vec![
                (j("a::V"), list(vec![j("a::C")])),
                (j("a::U"), list(vec![j("a::A"), j("a::B")])),
            ]),
            routed_target_classes: list(vec![
                (
                    j("a::P"),
                    list(vec![(j("firm"), j("a::Firm")), (j("boss"), j("a::Person"))]),
                ),
                (j("a::Q"), list(vec![])),
            ]),
        }
    }

    #[test]
    fn the_keyed_lookups_answer_the_entry_at_the_key() {
        let f = facts();
        assert_eq!(
            f.union_members_of(&j("a::U")).map(|m| m.to_vec()),
            Some(vec![j("a::A"), j("a::B")])
        );
        assert_eq!(
            f.union_members_of(&j("a::V")).map(|m| m.to_vec()),
            Some(vec![j("a::C")])
        );
        assert_eq!(f.union_members_of(&j("a::W")), None);
        assert_eq!(f.mixed_union_of(&j("a::U")).map(|m| m.len()), Some(2));
        assert_eq!(f.mixed_union_of(&j("a::V")), None);
        assert_eq!(
            f.union_key_threads_of(&j("a::U")).map(|t| t[0].column.clone()),
            Some(j("ID"))
        );
        assert_eq!(f.union_key_threads_of(&j("a::P")), None);
    }

    #[test]
    fn a_routed_target_is_none_for_an_owner_with_no_routes_or_another_property() {
        let f = facts();
        assert_eq!(f.routed_target_class(&j("a::P"), &j("boss")), Some(&j("a::Person")));
        assert_eq!(f.routed_target_class(&j("a::P"), &j("firm")), Some(&j("a::Firm")));
        // getOrDefault(owner, Map.of()).get(prop): an unknown owner, an owner with no routes, an unrouted property
        assert_eq!(f.routed_target_class(&j("a::Z"), &j("firm")), None);
        assert_eq!(f.routed_target_class(&j("a::Q"), &j("firm")), None);
        assert_eq!(f.routed_target_class(&j("a::P"), &j("name")), None);
    }

    #[test]
    fn a_poison_is_found_by_its_keys_kind_and_components() {
        let f = facts();
        assert_eq!(f.poison(&for_class("a::P")), Some(&j("class poison")));
        assert_eq!(f.poison(&for_set("a::P", "p1")), Some(&j("set poison")));
        assert_eq!(f.poison(&for_association("a::PA")), Some(&j("association poison")));
        // the same name under another kind of key, another set id: no poison
        assert_eq!(f.poison(&for_association("a::P")), None);
        assert_eq!(f.poison(&for_set("a::P", "p2")), None);
        assert_eq!(f.poison(&for_class("a::PA")), None);
        assert_eq!(
            MappingDefinitionNormalizationFacts::none().poison(&for_class("a::P")),
            None
        );
    }
}
