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
use lite_front::records as p;

use crate::linked;
use crate::records::{
    Function, FunctionDefinition, FunctionDefinitionParameterDefinition, KeyThread,
    MappingDefinitionNormalizationFacts, NativeFunctionDefinition, PoisonKey,
};

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

/// `Function.equals` as Java's records define it (2026-11-15, the element slice's integration; found by its
/// differential): the model's function records (`FunctionDefinition`, `NativeFunctionDefinition`, the
/// `ParameterDefinition` they hold) use Java's default, all-component record equality, but the protocol records they
/// hold do not -- a `TypeExpression`'s, a `ValueSpecification`'s and a `ConstraintDefinition`'s `equals` leave out the
/// source position (and, per record, the parser's spelling marks: [`p::ValueSpecification::equality_key`]'s docs),
/// a `Realization.Ref`'s reads its function's name alone -- where the generated records derive `PartialEq` over every
/// field. Two declarations parsed at two places are one function to Java: `DeclarationTable.of`'s `!f.equals(prior)`
/// (a graph's copy of a catalog native, or of a boot function) and `PureModelContext.findFunctionById`'s
/// `d.equals(tf.definition())` read this equality.
impl Function {
    /// `this.equals(other)` in Java.
    pub fn java_equals(&self, other: &Function) -> bool {
        match (self, other) {
            (Function::FunctionDefinition(a), Function::FunctionDefinition(b)) => a.java_equals(b),
            (Function::NativeFunctionDefinition(a), Function::NativeFunctionDefinition(b)) => a.java_equals(b),
            _ => false,
        }
    }
}

impl FunctionDefinition {
    /// `FunctionDefinition.equals` in Java: its components in order, each by its own Java equality.
    pub fn java_equals(&self, other: &FunctionDefinition) -> bool {
        self.qualified_name == other.qualified_name
            && self.type_parameters == other.type_parameters
            && self.multiplicity_parameters == other.multiplicity_parameters
            && parameters_java_equal(&self.parameters, &other.parameters)
            && self.return_type.equality_key() == other.return_type.equality_key()
            && self.return_multiplicity == other.return_multiplicity
            && self.body.len() == other.body.len()
            && self.body.iter().zip(other.body.iter()).all(|(a, b)| a.java_equals(b))
            && self.stereotypes == other.stereotypes
            && self.tagged_values == other.tagged_values
            && self.synthesized_from == other.synthesized_from
            && constraints_java_equal(&self.pre_constraints, &other.pre_constraints)
            && constraints_java_equal(&self.post_constraints, &other.post_constraints)
    }
}

impl NativeFunctionDefinition {
    /// `NativeFunctionDefinition.equals` in Java: its components in order, each by its own Java equality.
    pub fn java_equals(&self, other: &NativeFunctionDefinition) -> bool {
        self.qualified_name == other.qualified_name
            && self.type_parameters == other.type_parameters
            && self.multiplicity_parameters == other.multiplicity_parameters
            && parameters_java_equal(&self.parameters, &other.parameters)
            && self.return_type.equality_key() == other.return_type.equality_key()
            && self.return_multiplicity == other.return_multiplicity
            && self.stereotypes == other.stereotypes
            && self.tagged_values == other.tagged_values
    }
}

/// `FunctionDefinition.ParameterDefinition`'s default record equality: its name, its type by `TypeExpression.equals`,
/// its multiplicity (a protocol record of no position: its derived equality is Java's).
fn parameters_java_equal(
    a: &[Arc<FunctionDefinitionParameterDefinition>],
    b: &[Arc<FunctionDefinitionParameterDefinition>],
) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(x, y)| {
            x.name == y.name && x.r#type.equality_key() == y.r#type.equality_key() && x.multiplicity == y.multiplicity
        })
}

/// `ConstraintDefinition.equals` (ConstraintDefinition.java:46): its name, realization, message, enforcement level,
/// external id and owner -- not its position; a `Realization.Ref` by its function's name alone (Realization.java:63),
/// a `Realization.Inline` by its body's `ValueSpecification.equals`.
fn constraints_java_equal(a: &[Box<p::ConstraintDefinition>], b: &[Box<p::ConstraintDefinition>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(x, y)| {
            x.name == y.name
                && match (&x.realization, &y.realization) {
                    (p::Realization::RealizationRef(r), p::Realization::RealizationRef(s)) => {
                        r.function_fqn == s.function_fqn
                    }
                    (p::Realization::RealizationInline(r), p::Realization::RealizationInline(s)) => {
                        r.body.len() == s.body.len() && r.body.iter().zip(s.body.iter()).all(|(u, v)| u.java_equals(v))
                    }
                    _ => false,
                }
                && match (&x.message, &y.message) {
                    (None, None) => true,
                    (Some(u), Some(v)) => u.java_equals(v),
                    _ => false,
                }
                && x.enforcement_level == y.enforcement_level
                && x.external_id == y.external_id
                && x.owner == y.owner
        })
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
