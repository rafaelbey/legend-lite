// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! A model method the normalizer's clean-sheet door reads that the records do not carry yet (2026-11-14, the
//! normalizer slice's fill, helper `a`): `AssociationDefinition.AssociationEndDefinition.targetClassFqn()`, the ONE
//! extraction site of an association end's target class (MappingNormalizer.liftAssocInline; AssociationSynthesis reads
//! it too).

use lite_front::jtypes::JStr;
use lite_front::records::TypeExpression;

use crate::cat;
use crate::error::{Thrown, illegal_state};
use crate::records::AssociationDefinitionAssociationEndDefinition;

impl AssociationDefinitionAssociationEndDefinition {
    /// `targetClassFqn()`: the end's target class FQN -- its `NameRef`'s name. Anything else is Java's
    /// `IllegalStateException` (a normalizer bug: name resolution leaves a `NameRef` on every end), the type spelled
    /// by its Java simple name (`targetClass.getClass().getSimpleName()`).
    pub fn target_class_fqn(&self) -> Result<&JStr, Thrown> {
        let simple = match &self.target_class {
            TypeExpression::TypeExpressionNameRef(nr) => return Ok(&nr.name),
            TypeExpression::TypeExpressionGeneric(_) => "Generic",
            TypeExpression::TypeExpressionFunctionType(_) => "FunctionType",
            TypeExpression::TypeExpressionRelationType(_) => "RelationType",
            TypeExpression::TypeExpressionSchemaAlgebra(_) => "SchemaAlgebra",
        };
        Err(illegal_state(cat!(
            "association end '",
            &self.property_name,
            "' has unresolved target class type ",
            simple,
            " \u{2014} normalizer bug"
        )))
    }
}

#[cfg(test)]
mod tests {
    use lite_front::records::{TypeExpressionNameRef, TypeExpressionRelationType};

    use super::*;
    use crate::linked::empty;
    use crate::text::j;

    fn end(target: TypeExpression) -> AssociationDefinitionAssociationEndDefinition {
        AssociationDefinitionAssociationEndDefinition {
            property_name: j("firm"),
            target_class: target,
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
    fn a_name_ref_end_answers_its_name() {
        let e = end(TypeExpression::TypeExpressionNameRef(Box::new(TypeExpressionNameRef {
            name: j("a::Firm"),
            pos: None,
        })));
        assert_eq!(e.target_class_fqn(), Ok(&j("a::Firm")));
    }

    #[test]
    fn any_other_end_is_the_normalizer_bug_by_its_java_simple_name() {
        let e = end(TypeExpression::TypeExpressionRelationType(Box::new(
            TypeExpressionRelationType { columns: Vec::new() },
        )));
        assert_eq!(
            e.target_class_fqn(),
            Err(illegal_state(j(
                "association end 'firm' has unresolved target class type RelationType \u{2014} normalizer bug"
            )))
        );
    }
}
