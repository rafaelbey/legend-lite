// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `DerivedProps` (2026-11-12): a derived property lifted to its `<owner>$prop$<name>` function, the one synthesis the
//! normalizer's E.2 pass and the function resolver's on-demand arm share.

use std::sync::Arc;

use lite_model::protocol as p;
use lite_model::{
    ClassDefinition, FunctionDefinition, FunctionDefinitionParameterDefinition, FunctionDefinitionSynthesized, JStr,
    SynthHat, cat,
};

use crate::error::Thrown;
use crate::synth_fqn;

/// `Multiplicity.Concrete.PURE_ONE`: `[1]`.
fn pure_one() -> p::Multiplicity {
    p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
        lower_bound: 1,
        upper_bound: Some(1),
    }))
}

/// `lift(cd, dp)`: `<owner>$prop$<name>(this: Owner[1], <params>): T[m]` carrying the derived property's body, the
/// class's type and multiplicity parameters propagated, tagged as synthesized from the property.
pub fn lift(cd: &ClassDefinition, dp: &p::DerivedPropertyDefinition) -> Result<FunctionDefinition, Thrown> {
    let mut params: Vec<Arc<FunctionDefinitionParameterDefinition>> = Vec::with_capacity(dp.parameters.len() + 1);
    params.push(Arc::new(FunctionDefinitionParameterDefinition {
        name: JStr::from("this"),
        r#type: receiver_type(cd),
        multiplicity: pure_one(),
    }));
    for pd in &dp.parameters {
        params.push(Arc::new(FunctionDefinitionParameterDefinition {
            name: pd.name.clone(),
            r#type: pd.r#type.clone(),
            multiplicity: pd.multiplicity.clone(),
        }));
    }
    // the constructor's arguments in Java's order: the name (its refusal first), then the body
    let name = synth_fqn::prop(&cd.qualified_name, &dp.name)?;
    // `dp.expression()`: an inline body, else Java's IllegalStateException
    let body = match &dp.realization {
        p::Realization::RealizationInline(inl) => inl.body.clone(),
        p::Realization::RealizationRef(_) => {
            return Err(Thrown::illegal_state(cat!(
                "derived property '",
                &dp.name,
                "' is a function-ref binding, not an inline body"
            )));
        }
    };
    Ok(FunctionDefinition {
        qualified_name: name,
        type_parameters: cd.type_params.clone(),
        multiplicity_parameters: cd.multiplicity_params.clone(),
        parameters: params.into(),
        return_type: dp.r#type.clone(),
        return_multiplicity: dp.multiplicity.clone(),
        body: body.into(),
        stereotypes: Arc::default(),
        tagged_values: Arc::default(),
        synthesized_from: Some(Arc::new(FunctionDefinitionSynthesized {
            hat: SynthHat::PROP,
            owner_fqn: cd.qualified_name.clone(),
            member_name: dp.name.clone(),
            forward_property: None,
        })),
        pre_constraints: Arc::default(),
        post_constraints: Arc::default(),
    })
}

/// `receiverType(cd)`: the class's FQN, or `Owner<T, ...>` when the owner is generic.
pub fn receiver_type(cd: &ClassDefinition) -> p::TypeExpression {
    if cd.type_params.is_empty() && cd.multiplicity_params.is_empty() {
        return p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
            name: cd.qualified_name.clone(),
            pos: None,
        }));
    }
    let args = cd
        .type_params
        .iter()
        .map(|tp| {
            p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
                name: tp.clone(),
                pos: None,
            }))
        })
        .collect();
    p::TypeExpression::TypeExpressionGeneric(Box::new(p::TypeExpressionGeneric {
        name: cd.qualified_name.clone(),
        arguments: args,
        multiplicity_arguments: cd.multiplicity_params.to_vec(),
        type_variable_values: Vec::new(),
        pos: None,
    }))
}

/// `splitPropFqn(fqn)`: `{owner, name}` of a `$prop$` reference, `None` if it is none.
pub fn split_prop_fqn(fqn: &JStr) -> Option<(JStr, JStr)> {
    lite_model::names::split_derived_property(fqn)
}
