// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `FunctionDefinition`'s methods and convenience constructors the normalizer calls (2026-11-14, phase 3's normalizer
//! slice, step 3: the model methods, docs/rust-port/EF_CENSUS.md §8): `isSynthesized`, `withSynthesizedFrom`, the
//! nine- and ten-argument constructors, and `Synthesized`'s three-argument one. Java's overloads by arity are named
//! here by what each leaves out: [`FunctionDefinition::user_written`] (nine: no provenance, no constraints block) and
//! [`FunctionDefinition::without_constraints`] (ten: no constraints block -- a rebuild through it DROPS the function's
//! pre- and post-conditions, as `ModelNormalizer`'s body rewrite does).

use std::sync::Arc;

use lite_front::jtypes::JStr;
use lite_front::records::{Multiplicity, TypeExpression, ValueSpecification};

use crate::linked::empty;
use crate::records::{
    FunctionDefinition, FunctionDefinitionParameterDefinition, FunctionDefinitionSynthesized, StereotypeApplication,
    SynthHat, TaggedValue,
};

impl FunctionDefinition {
    /// `new FunctionDefinition(qualifiedName, ..., taggedValues, synthesizedFrom)`: a function with no constraints
    /// block (its pre- and post-conditions the one empty list).
    #[allow(clippy::too_many_arguments)] // Java's ten-argument constructor, argument for argument
    pub fn without_constraints(
        qualified_name: JStr,
        type_parameters: Arc<[JStr]>,
        multiplicity_parameters: Arc<[JStr]>,
        parameters: Arc<[Arc<FunctionDefinitionParameterDefinition>]>,
        return_type: TypeExpression,
        return_multiplicity: Multiplicity,
        body: Arc<[ValueSpecification]>,
        stereotypes: Arc<[Arc<StereotypeApplication>]>,
        tagged_values: Arc<[Arc<TaggedValue>]>,
        synthesized_from: Option<Arc<FunctionDefinitionSynthesized>>,
    ) -> FunctionDefinition {
        FunctionDefinition {
            qualified_name,
            type_parameters,
            multiplicity_parameters,
            parameters,
            return_type,
            return_multiplicity,
            body,
            stereotypes,
            tagged_values,
            synthesized_from,
            pre_constraints: empty(),
            post_constraints: empty(),
        }
    }

    /// `new FunctionDefinition(qualifiedName, ..., taggedValues)`: a user-written (non-synthesized) function with no
    /// constraints block; a synthesizer tags its provenance after, through
    /// [`with_synthesized_from`](Self::with_synthesized_from).
    #[allow(clippy::too_many_arguments)] // Java's nine-argument constructor, argument for argument
    pub fn user_written(
        qualified_name: JStr,
        type_parameters: Arc<[JStr]>,
        multiplicity_parameters: Arc<[JStr]>,
        parameters: Arc<[Arc<FunctionDefinitionParameterDefinition>]>,
        return_type: TypeExpression,
        return_multiplicity: Multiplicity,
        body: Arc<[ValueSpecification]>,
        stereotypes: Arc<[Arc<StereotypeApplication>]>,
        tagged_values: Arc<[Arc<TaggedValue>]>,
    ) -> FunctionDefinition {
        FunctionDefinition::without_constraints(
            qualified_name,
            type_parameters,
            multiplicity_parameters,
            parameters,
            return_type,
            return_multiplicity,
            body,
            stereotypes,
            tagged_values,
            None,
        )
    }

    /// `isSynthesized()`: whether this function was synthesized from a body site in phase E.
    pub fn is_synthesized(&self) -> bool {
        self.synthesized_from.is_some()
    }

    /// `withSynthesizedFrom(provenance)`: a copy of this function tagged with phase E's provenance, every other
    /// component kept (its constraints included).
    pub fn with_synthesized_from(&self, provenance: Arc<FunctionDefinitionSynthesized>) -> FunctionDefinition {
        FunctionDefinition {
            qualified_name: self.qualified_name.clone(),
            type_parameters: self.type_parameters.clone(),
            multiplicity_parameters: self.multiplicity_parameters.clone(),
            parameters: self.parameters.clone(),
            return_type: self.return_type.clone(),
            return_multiplicity: self.return_multiplicity.clone(),
            body: self.body.clone(),
            stereotypes: self.stereotypes.clone(),
            tagged_values: self.tagged_values.clone(),
            synthesized_from: Some(provenance),
            pre_constraints: self.pre_constraints.clone(),
            post_constraints: self.post_constraints.clone(),
        }
    }
}

impl FunctionDefinitionSynthesized {
    /// `new Synthesized(hat, ownerFqn, memberName)`: provenance with no forward property (only an association's
    /// synthesis names one).
    pub fn new(hat: SynthHat, owner_fqn: JStr, member_name: JStr) -> FunctionDefinitionSynthesized {
        FunctionDefinitionSynthesized {
            hat,
            owner_fqn,
            member_name,
            forward_property: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lite_front::codec;
    use lite_front::records::{
        ConstraintDefinition, MultiplicityConcrete, Realization, RealizationInline, TypeExpressionNameRef,
    };

    fn json<T: codec::Codec + ?Sized>(v: &T) -> String {
        codec::write(v, false)
    }

    fn boolean() -> TypeExpression {
        TypeExpression::TypeExpressionNameRef(Box::new(TypeExpressionNameRef {
            name: JStr::from("meta::pure::metamodel::type::Boolean"),
            pos: None,
        }))
    }

    fn one() -> Multiplicity {
        Multiplicity::MultiplicityConcrete(Box::new(MultiplicityConcrete {
            lower_bound: 1,
            upper_bound: Some(1),
        }))
    }

    #[test]
    fn the_short_forms_leave_out_provenance_and_constraints() {
        let f = FunctionDefinition::user_written(
            JStr::from("a::f"),
            empty(),
            empty(),
            empty(),
            boolean(),
            one(),
            empty(),
            empty(),
            empty(),
        );
        assert!(!f.is_synthesized());
        assert!(f.pre_constraints.is_empty() && f.post_constraints.is_empty());
        let provenance = Arc::new(FunctionDefinitionSynthesized::new(
            SynthHat::ASSOC,
            JStr::from("a::M"),
            JStr::from("a::A"),
        ));
        assert_eq!(provenance.forward_property, None);
        let tagged = f.with_synthesized_from(provenance.clone());
        assert!(tagged.is_synthesized());
        assert_eq!(
            json(&tagged),
            json(&FunctionDefinition {
                synthesized_from: Some(provenance),
                ..f
            })
        );
    }

    #[test]
    fn with_synthesized_from_keeps_the_constraints_and_the_ten_argument_rebuild_drops_them() {
        let pre: Arc<[Box<ConstraintDefinition>]> = Arc::from(vec![Box::new(ConstraintDefinition {
            name: JStr::from("c0"),
            realization: Realization::RealizationInline(Box::new(RealizationInline { body: Vec::new() })),
            message: None,
            enforcement_level: None,
            external_id: None,
            owner: None,
            pos: None,
        })]);
        let f = FunctionDefinition {
            pre_constraints: pre.clone(),
            ..FunctionDefinition::user_written(
                JStr::from("a::f"),
                empty(),
                empty(),
                empty(),
                boolean(),
                one(),
                empty(),
                empty(),
                empty(),
            )
        };
        let tagged = f.with_synthesized_from(Arc::new(FunctionDefinitionSynthesized::new(
            SynthHat::PROP,
            JStr::from("a::P"),
            JStr::from("p"),
        )));
        assert_eq!(tagged.pre_constraints, pre);
        let rebuilt = FunctionDefinition::without_constraints(
            tagged.qualified_name.clone(),
            tagged.type_parameters.clone(),
            tagged.multiplicity_parameters.clone(),
            tagged.parameters.clone(),
            tagged.return_type.clone(),
            tagged.return_multiplicity.clone(),
            tagged.body.clone(),
            tagged.stereotypes.clone(),
            tagged.tagged_values.clone(),
            tagged.synthesized_from.clone(),
        );
        assert!(
            rebuilt.pre_constraints.is_empty(),
            "Java's ten-argument form has no constraints block"
        );
        assert_eq!(rebuilt.synthesized_from, tagged.synthesized_from);
    }
}
