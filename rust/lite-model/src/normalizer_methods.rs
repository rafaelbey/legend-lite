// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The model's methods the normalizer reads that the records do not carry yet (2026-11-14, the normalizer slice's
//! skeleton): the interface accessors two sealed families declare in Java, each variant answering its own component.
//!
//! - `PropertyMapping.propertyName()`: every one of the ten variants has its property's name.
//! - `AssociationMapping.associationName()`: every one of the three variants has its association's name, as written.
//! - `AssociationPropertyMapping.propertyName()`: its body's property name.

use lite_front::jtypes::JStr;

use crate::records::{AssociationMapping, AssociationPropertyMapping, PropertyMapping};

impl PropertyMapping {
    /// `propertyName()`.
    pub fn property_name(&self) -> &JStr {
        match self {
            PropertyMapping::PropertyMappingEnumeratedExpression(x) => &x.property_name,
            PropertyMapping::PropertyMappingColumn(x) => &x.property_name,
            PropertyMapping::PropertyMappingEnumeratedColumn(x) => &x.property_name,
            PropertyMapping::PropertyMappingJoin(x) => &x.property_name,
            PropertyMapping::PropertyMappingJoinTerminalColumn(x) => &x.property_name,
            PropertyMapping::PropertyMappingExpression(x) => &x.property_name,
            PropertyMapping::PropertyMappingEmbedded(x) => &x.property_name,
            PropertyMapping::PropertyMappingInlineEmbedded(x) => &x.property_name,
            PropertyMapping::PropertyMappingOtherwiseEmbedded(x) => &x.property_name,
            PropertyMapping::PropertyMappingLocalProperty(x) => &x.property_name,
        }
    }
}

impl AssociationMapping {
    /// `associationName()`.
    pub fn association_name(&self) -> &JStr {
        match self {
            AssociationMapping::AssociationMappingRelational(x) => &x.association_name,
            AssociationMapping::AssociationMappingCross(x) => &x.association_name,
            AssociationMapping::AssociationMappingModelJoin(x) => &x.association_name,
        }
    }
}

impl AssociationPropertyMapping {
    /// `propertyName()`: `body.propertyName()`.
    pub fn property_name(&self) -> &JStr {
        self.body.property_name()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::linked::empty;
    use crate::records::{
        AssociationMapping, AssociationMappingRelational, PropertyMapping, PropertyMappingColumn,
        PropertyMappingInlineEmbedded,
    };
    use crate::text::j;

    #[test]
    fn each_variant_answers_its_own_name() {
        let col = PropertyMapping::PropertyMappingColumn(Arc::new(PropertyMappingColumn {
            property_name: j("name"),
            database: j("db"),
            table: j("T"),
            column: j("NAME"),
        }));
        assert_eq!(col.property_name(), &j("name"));
        let inline = PropertyMapping::PropertyMappingInlineEmbedded(Arc::new(PropertyMappingInlineEmbedded {
            property_name: j("firm"),
            set_id: j("f1"),
        }));
        assert_eq!(inline.property_name(), &j("firm"));
        let am = AssociationMapping::AssociationMappingRelational(Arc::new(AssociationMappingRelational {
            association_name: j("Employment"),
            property_mappings: empty(),
        }));
        assert_eq!(am.association_name(), &j("Employment"));
    }
}
