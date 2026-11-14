// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The mapping definitions' constructors and methods the normalizer calls (2026-11-14, phase 3's normalizer slice,
//! step 3: the model methods, docs/rust-port/EF_CENSUS.md §8): `MappingDefinition`'s canonical constructor and its
//! common five-argument form, `NormalizationFacts.NONE`, `ClassBinding.DeclaredKeys.NONE`,
//! `LegacyMappingDefinition.withClassMappings`, and `KeyThread`'s four-argument constructor.
//!
//! Java's canonical constructor wraps the class bindings in `ClassBindings` (lookups by class, set id and function, and
//! an identity `position` map) and the association bindings in `AssociationBindings` (by association). Those are not
//! here: the lookups are the resolver's (a later slice), and `position` is re-expressed as a binding's index in
//! `class_bindings` (EF_CENSUS §8). What the constructor keeps -- the lists in their order, the stores' map in its
//! iteration order -- is the record's own components, which a Rust caller hands over already frozen (`Arc<[T]>`, a map
//! `Arc<[(K, V)]>` in insertion order: `linked`), so Java's copies (`List.copyOf`, a `LinkedHashMap` copy) are the
//! sharing of an `Arc`. Java's `NONE`s are one object each; their identity is never asked (no `==` reads them), so each
//! call here answers a fresh, equal record.

use std::sync::Arc;

use lite_front::jtypes::JStr;

use crate::linked::empty;
use crate::records::{
    ClassMapping, EnumerationMapping, KeyThread, LegacyMappingDefinition, MappingDefinition,
    MappingDefinitionAssociationBinding, MappingDefinitionClassBinding, MappingDefinitionClassBindingDeclaredKeys,
    MappingDefinitionNormalizationFacts, MappingInclude,
};

impl MappingDefinition {
    /// `new MappingDefinition(qualifiedName, includes, classBindings, associationBindings, enumerationMappings,
    /// resolvedStores, facts)`: the canonical constructor, its bindings' lookups left to the resolver.
    pub fn new(
        qualified_name: JStr,
        includes: Arc<[Arc<MappingInclude>]>,
        class_bindings: Arc<[MappingDefinitionClassBinding]>,
        association_bindings: Arc<[Arc<MappingDefinitionAssociationBinding>]>,
        enumeration_mappings: Arc<[Arc<EnumerationMapping>]>,
        resolved_stores: Arc<[(JStr, JStr)]>,
        facts: Arc<MappingDefinitionNormalizationFacts>,
    ) -> MappingDefinition {
        MappingDefinition {
            qualified_name,
            includes,
            class_bindings,
            association_bindings,
            enumeration_mappings,
            resolved_stores,
            facts,
        }
    }

    /// `new MappingDefinition(qualifiedName, includes, classBindings, associationBindings, enumerationMappings)`: the
    /// common form -- no store substitutions (the one empty map), no facts ([`NormalizationFacts::none`]).
    ///
    /// [`NormalizationFacts::none`]: MappingDefinitionNormalizationFacts::none
    pub fn common(
        qualified_name: JStr,
        includes: Arc<[Arc<MappingInclude>]>,
        class_bindings: Arc<[MappingDefinitionClassBinding]>,
        association_bindings: Arc<[Arc<MappingDefinitionAssociationBinding>]>,
        enumeration_mappings: Arc<[Arc<EnumerationMapping>]>,
    ) -> MappingDefinition {
        MappingDefinition::new(
            qualified_name,
            includes,
            class_bindings,
            association_bindings,
            enumeration_mappings,
            empty(),
            MappingDefinitionNormalizationFacts::none(),
        )
    }
}

impl MappingDefinitionNormalizationFacts {
    /// `NormalizationFacts.NONE`: no poison, no mixed union, no key thread, no union member, no routed target class.
    pub fn none() -> Arc<MappingDefinitionNormalizationFacts> {
        Arc::new(MappingDefinitionNormalizationFacts {
            poisons: empty(),
            mixed_unions: empty(),
            union_key_threads: empty(),
            union_members: empty(),
            routed_target_classes: empty(),
        })
    }
}

impl MappingDefinitionClassBindingDeclaredKeys {
    /// `ClassBinding.DeclaredKeys.NONE`: not distinct, and no group-by, primary-key or mapped column, no own property.
    pub fn none() -> Arc<MappingDefinitionClassBindingDeclaredKeys> {
        Arc::new(MappingDefinitionClassBindingDeclaredKeys {
            distinct: false,
            group_by_columns: empty(),
            primary_key_columns: empty(),
            mapped_columns: empty(),
            own_properties: empty(),
        })
    }
}

impl LegacyMappingDefinition {
    /// `withClassMappings(newClassMappings)`: this mapping with its class mappings replaced, its name, includes,
    /// association and enumeration mappings kept.
    pub fn with_class_mappings(&self, new_class_mappings: Arc<[ClassMapping]>) -> LegacyMappingDefinition {
        LegacyMappingDefinition {
            qualified_name: self.qualified_name.clone(),
            includes: self.includes.clone(),
            class_mappings: new_class_mappings,
            association_mappings: self.association_mappings.clone(),
            enumeration_mappings: self.enumeration_mappings.clone(),
        }
    }
}

impl KeyThread {
    /// `new KeyThread(name, pureKind, column, ordinal)`: a key thread with no store and no table.
    pub fn new(name: JStr, pure_kind: Option<JStr>, column: JStr, ordinal: i32) -> KeyThread {
        KeyThread {
            name,
            pure_kind,
            column,
            ordinal,
            store: None,
            table: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lite_front::codec;

    use crate::records::{ClassMappingInheritance, MappingDefinitionClassBindingPure};

    fn json<T: codec::Codec + ?Sized>(v: &T) -> String {
        codec::write(v, false)
    }

    #[test]
    fn the_common_form_has_no_stores_and_no_facts() {
        let binding = MappingDefinitionClassBinding::MappingDefinitionClassBindingPure(Arc::new(
            MappingDefinitionClassBindingPure {
                class_fqn: JStr::from("a::P"),
                set_id: None,
                extends_set_id: None,
                root: true,
                function_fqn: JStr::from("a::M$class$a::P"),
                primary_key_columns: empty(),
            },
        ));
        let m = MappingDefinition::common(JStr::from("a::M"), empty(), Arc::from(vec![binding]), empty(), empty());
        assert!(m.resolved_stores.is_empty());
        assert_eq!(m.facts, MappingDefinitionNormalizationFacts::none());
        assert_eq!(m.class_bindings.len(), 1);
        assert_eq!(
            json(&*m.facts),
            "{\"@\":\"model.MappingDefinition$NormalizationFacts\",\"poisons\":[],\"mixedUnions\":{},\
             \"unionKeyThreads\":{},\"unionMembers\":{},\"routedTargetClasses\":{}}"
        );
    }

    #[test]
    fn declared_keys_none_is_empty_and_not_distinct() {
        assert_eq!(
            json(&*MappingDefinitionClassBindingDeclaredKeys::none()),
            "{\"@\":\"model.MappingDefinition$ClassBinding$DeclaredKeys\",\"distinct\":false,\"groupByColumns\":[],\
             \"primaryKeyColumns\":[],\"mappedColumns\":[],\"ownProperties\":[]}"
        );
    }

    #[test]
    fn with_class_mappings_replaces_the_class_mappings_alone() {
        let legacy = LegacyMappingDefinition {
            qualified_name: JStr::from("a::M"),
            includes: Arc::from(vec![Arc::new(MappingInclude {
                mapping_path: JStr::from("a::Base"),
                substitutions: empty(),
            })]),
            class_mappings: empty(),
            association_mappings: empty(),
            enumeration_mappings: empty(),
        };
        let cm = ClassMapping::ClassMappingInheritance(Arc::new(ClassMappingInheritance {
            class_name: JStr::from("a::V"),
            set_id: None,
            extends_set_id: None,
            root: false,
        }));
        let out = legacy.with_class_mappings(Arc::from(vec![cm.clone()]));
        assert_eq!(
            out,
            LegacyMappingDefinition {
                class_mappings: Arc::from(vec![cm]),
                ..legacy.clone()
            }
        );
        assert_eq!(out.includes, legacy.includes);
    }

    #[test]
    fn a_four_argument_key_thread_has_no_store_or_table() {
        let t = KeyThread::new(JStr::from("ID_0"), Some(JStr::from("Integer")), JStr::from("ID"), 0);
        assert_eq!((t.store.clone(), t.table.clone()), (None, None));
        assert_eq!(
            json(&t),
            "{\"@\":\"model.KeyThread\",\"name\":\"ID_0\",\"pureKind\":\"Integer\",\"column\":\"ID\",\"ordinal\":0,\
             \"store\":null,\"table\":null}"
        );
    }
}
