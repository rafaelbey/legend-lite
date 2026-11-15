// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `SetKeyFacts` (core/src/main/java/com/legend/normalizer/SetKeyFacts.java at ad86fa895, ported line for line): the
//! key text a relational set DECLARES itself -- the metamodel's per-set facts (`ClassBinding.DeclaredKeys`), captured
//! from the parsed class mapping BEFORE the extends pre-pass merges the parent's in.

use std::sync::Arc;

use lite_model::linked::list;
use lite_model::names::set_id_of;
use lite_model::{
    ClassMappingRelational, JStr, MappingDefinitionClassBindingDeclaredKeys as DeclaredKeys, PropertyMapping,
    RelationalOperation,
};

/// `SetKeyFacts.setKey` (:27): the set's EFFECTIVE id (`SetId`), the declared-keys capture's key.
pub fn set_key(r: &ClassMappingRelational) -> JStr {
    set_id_of(r.set_id.as_ref(), &r.class_name)
}

/// `SetKeyFacts.declaredKeysOf` (:32): the key text a set declares itself (pre-extends-merge).
pub fn declared_keys_of(r: &ClassMappingRelational) -> Arc<DeclaredKeys> {
    Arc::new(DeclaredKeys {
        distinct: r.distinct,
        group_by_columns: list(column_names(&r.group_by)),
        primary_key_columns: list(column_names(&r.primary_key)),
        mapped_columns: list(mapped_column_names(r)),
        own_properties: list(own_property_names(r)),
    })
}

/// `SetKeyFacts.ownPropertyNames` (:40): the property names the set maps itself (pre-inheritance-merge).
pub fn own_property_names(rcm: &ClassMappingRelational) -> Vec<JStr> {
    rcm.property_mappings
        .iter()
        .map(|pm| pm.property_name().clone())
        .collect()
}

/// `SetKeyFacts.columnNames` (:49): column-ref operations' column names (other shapes: no key fact).
pub fn column_names(ops: &[RelationalOperation]) -> Vec<JStr> {
    let mut out = Vec::new();
    for op in ops {
        if let RelationalOperation::RelationalOperationColumnRef(cr) = op {
            out.push(cr.column.clone());
        }
    }
    out
}

/// `SetKeyFacts.mappedColumnNames` (:60): direct column property mappings' columns -- a `~distinct` set's key.
pub fn mapped_column_names(rcm: &ClassMappingRelational) -> Vec<JStr> {
    let mut out = Vec::new();
    for pm in rcm.property_mappings.iter() {
        if let PropertyMapping::PropertyMappingColumn(pc) = pm {
            out.push(pc.column.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{column_pm, relational};
    use lite_model::RelationalOperationColumnRef;
    use lite_model::text::j;

    #[test]
    fn a_sets_declared_keys_are_its_own_text() {
        let mut r = relational(
            "a::P",
            Some("p1"),
            vec![column_pm("name", "T", "NAME"), column_pm("id", "T", "ID")],
        );
        r.distinct = true;
        r.primary_key = Arc::from(vec![RelationalOperation::RelationalOperationColumnRef(Arc::new(
            RelationalOperationColumnRef {
                database_name: None,
                table: j("T"),
                column: j("ID"),
            },
        ))]);
        let k = declared_keys_of(&r);
        assert!(k.distinct);
        assert_eq!(&*k.primary_key_columns, &[j("ID")]);
        assert_eq!(&*k.mapped_columns, &[j("NAME"), j("ID")]);
        assert_eq!(&*k.own_properties, &[j("name"), j("id")]);
        assert!(k.group_by_columns.is_empty());
        assert_eq!(set_key(&r), j("p1"));
        assert_eq!(set_key(&relational("a::P", None, vec![])), j("a_P"));
    }
}
