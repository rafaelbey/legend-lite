// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The unit tests' builders: records by hand, and models from text (lite-builtin's door, lite-names' resolver and
//! index, as the pipeline runs them before E).

use std::sync::Arc;

use lite_front::doors::Dialect;
use lite_model::linked::{empty, list};
use lite_model::text::j;
use lite_model::{
    ClassMapping, ClassMappingRelational, ClassMappingUnion, LegacyMappingDefinition, PackageableElement, ParsedModel,
    PropertyMapping, PropertyMappingColumn, PropertyMappingJoin,
};
use lite_names::ModelIndex;

/// A Relational set of `class` (`set_id` its declared id) with `pms`, everything else empty.
pub fn relational(class: &str, set_id: Option<&str>, pms: Vec<PropertyMapping>) -> ClassMappingRelational {
    ClassMappingRelational {
        class_name: j(class),
        set_id: set_id.map(j),
        extends_set_id: None,
        root: false,
        main_table: None,
        filter: None,
        distinct: false,
        group_by: empty(),
        primary_key: empty(),
        property_mappings: list(pms),
        source_url: None,
        property_target_sets: empty(),
        aggregation: None,
    }
}

/// The set as a class mapping.
pub fn cm(r: ClassMappingRelational) -> ClassMapping {
    ClassMapping::ClassMappingRelational(Arc::new(r))
}

/// A union operation of `members`.
pub fn union(class: &str, set_id: Option<&str>, members: &[&str]) -> ClassMapping {
    ClassMapping::ClassMappingUnion(Arc::new(ClassMappingUnion {
        class_name: j(class),
        set_id: set_id.map(j),
        extends_set_id: None,
        root: false,
        member_set_ids: list(members.iter().map(|m| j(m)).collect()),
    }))
}

/// `prop: [db] table.column`.
pub fn column_pm(prop: &str, table: &str, column: &str) -> PropertyMapping {
    PropertyMapping::PropertyMappingColumn(Arc::new(PropertyMappingColumn {
        property_name: j(prop),
        database: j("db"),
        table: j(table),
        column: j(column),
    }))
}

/// `prop[target]: [db] @J`.
pub fn join_pm(prop: &str, target: Option<&str>) -> PropertyMapping {
    PropertyMapping::PropertyMappingJoin(Arc::new(PropertyMappingJoin {
        property_name: j(prop),
        database: j("db"),
        joins: empty(),
        target_set_id: target.map(j),
    }))
}

/// A legacy mapping of `cms`, no includes.
pub fn legacy(fqn: &str, cms: Vec<ClassMapping>) -> LegacyMappingDefinition {
    LegacyMappingDefinition {
        qualified_name: j(fqn),
        includes: empty(),
        class_mappings: list(cms),
        association_mappings: empty(),
        enumeration_mappings: empty(),
    }
}

/// A model text parsed and its names resolved (strict), as E receives it.
pub fn model(text: &str) -> ParsedModel {
    let units: Vec<u16> = text.encode_utf16().collect();
    let parsed = lite_builtin::door::parse(&units, Dialect::LegendLite).expect("the model parses");
    lite_names::resolver::resolve(&parsed).expect("the model's names resolve")
}

/// The model's index.
pub fn index(parsed: &ParsedModel) -> ModelIndex {
    ModelIndex::from(parsed).expect("the model indexes")
}

/// The legacy mapping `fqn` of the model.
pub fn mapping(parsed: &ParsedModel, fqn: &str) -> Arc<LegacyMappingDefinition> {
    parsed
        .elements
        .iter()
        .find_map(|e| match e {
            PackageableElement::LegacyMappingDefinition(m) if m.qualified_name == j(fqn) => Some(m.clone()),
            _ => None,
        })
        .expect("the mapping")
}
