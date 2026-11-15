// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The model's methods the normalizer reads that the records do not carry yet (2026-11-14, the normalizer slice's
//! skeleton): the interface accessors two sealed families declare in Java, each variant answering its own component.
//!
//! - `PropertyMapping.propertyName()`: every one of the ten variants has its property's name.
//! - `AssociationMapping.associationName()`: every one of the three variants has its association's name, as written.
//! - `AssociationPropertyMapping.propertyName()`: its body's property name.
//! - `AssociationDefinition.AssociationEndDefinition.targetClassFqn()` (AssociationDefinition.java:110): the end's
//!   target class name, the internal error for an end whose target type is not a name (the fill, 2026-11-14:
//!   AssociationSynthesis, ModelJoinNesting and the clean-sheet lift read it);
//! - `FilterPointer.name()` (FilterPointer.java:35): every variant's filter name.

use lite_front::jtypes::JStr;
use lite_front::records::TypeExpression;

use crate::cat;
use crate::error::{Thrown, illegal_state};
use crate::records::{
    AssociationDefinitionAssociationEndDefinition, AssociationMapping, AssociationPropertyMapping, FilterPointer,
    PropertyMapping,
};

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

impl AssociationDefinitionAssociationEndDefinition {
    /// `targetClassFqn()`: the target class's name when it is a `NameRef`; otherwise the `IllegalStateException`
    /// naming the end and the target type's Java simple name.
    pub fn target_class_fqn(&self) -> Result<&JStr, Thrown> {
        let kind = match &self.target_class {
            TypeExpression::TypeExpressionNameRef(nr) => return Ok(&nr.name),
            // `targetClass.getClass().getSimpleName()`: the protocol record's own name
            TypeExpression::TypeExpressionGeneric(_) => "Generic",
            TypeExpression::TypeExpressionFunctionType(_) => "FunctionType",
            TypeExpression::TypeExpressionRelationType(_) => "RelationType",
            TypeExpression::TypeExpressionSchemaAlgebra(_) => "SchemaAlgebra",
        };
        Err(illegal_state(cat!(
            "association end '",
            &self.property_name,
            "' has unresolved target class type ",
            kind,
            " \u{2014} normalizer bug"
        )))
    }
}

impl FilterPointer {
    /// `name()`: the filter's name, on every variant.
    pub fn name(&self) -> &JStr {
        match self {
            FilterPointer::FilterPointerLocal(l) => &l.name,
            FilterPointer::FilterPointerCross(c) => &c.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lite_front::records::{TypeExpressionGeneric, TypeExpressionNameRef};

    use super::*;
    use crate::linked::empty;
    use crate::records::{
        AssociationMappingRelational, FilterPointerCross, FilterPointerLocal, PropertyMappingColumn,
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

    // the fill, 2026-11-14: an end's target class and a filter pointer's name
    fn end(target_class: TypeExpression) -> AssociationDefinitionAssociationEndDefinition {
        AssociationDefinitionAssociationEndDefinition {
            property_name: j("firm"),
            target_class,
            multiplicity: lite_front::records::Multiplicity::MultiplicityConcrete(Box::new(
                lite_front::records::MultiplicityConcrete {
                    lower_bound: 1,
                    upper_bound: Some(1),
                },
            )),
            stereotypes: empty(),
            tagged_values: empty(),
        }
    }

    #[test]
    fn an_ends_target_is_its_name_ref_or_the_internal_error() {
        let named = end(TypeExpression::TypeExpressionNameRef(Box::new(TypeExpressionNameRef {
            name: j("a::Firm"),
            pos: None,
        })));
        assert_eq!(named.target_class_fqn(), Ok(&j("a::Firm")));
        let generic = end(TypeExpression::TypeExpressionGeneric(Box::new(TypeExpressionGeneric {
            name: j("a::Box"),
            arguments: Vec::new(),
            multiplicity_arguments: Vec::new(),
            type_variable_values: Vec::new(),
            pos: None,
        })));
        assert_eq!(
            generic.target_class_fqn(),
            Err(illegal_state(j(
                "association end 'firm' has unresolved target class type Generic \u{2014} normalizer bug"
            )))
        );
    }

    #[test]
    fn a_filter_pointer_names_its_filter() {
        let local = FilterPointer::FilterPointerLocal(Arc::new(FilterPointerLocal { name: j("F") }));
        let cross = FilterPointer::FilterPointerCross(Arc::new(FilterPointerCross {
            db: j("a::db"),
            name: j("G"),
        }));
        assert_eq!((local.name(), cross.name()), (&j("F"), &j("G")));
    }
}
