// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ModelNormalizer` (ModelNormalizer.java at ad86fa895): Phase E's entry -- for every Pure body site it lifts the body
//! into an ordinary top-level `FunctionDefinition` appended to the `NormalizedModel`'s element list, tagged with its
//! provenance: E.5 the store views (first: every consumer reads one owner of a view's body), E.1 the mappings
//! (`MappingNormalizer`), E.2 derived properties (`$prop$`), E.3 constraints (`$constraint$`, `$constraintMsg$`), E.3b
//! property defaults (`$default$`), E.3c a function's pre- and post-conditions, E.4 service queries (`$query`); then
//! E.6, every synthesized body resolved like text (`NameResolver.resolveQueryIn`, under its owner's imports, over
//! one universe built once).
//!
//! RE-EXPRESSED (census §4): `receiverType` is lite-names' `derived_props::receiver_type` (an identical body; not a
//! function here); `requireQualifiedPropertiesAdopted`'s `d == dp` (:220, an internal pipeline-order assertion) is
//! `lite_base::same` on the definitions, never a derived `==` (an equal un-adopted property would pass).

use std::sync::Arc;

use lite_front::records as p;
use lite_model::linked::{LinkedMap, list};
use lite_model::{
    ClassDefinition, FunctionDefinition, JStr, NormalizedModel, PackageableElement, ParsedModel, ServiceDefinition,
};
use lite_names::ModelIndex;

use crate::error::Thrown;
use crate::lifted_views::LiftedViews;
use crate::mapping_normalizer;

/// `ModelNormalizer.normalize(parsed, model, wallSink)` (:110): the name-resolved model normalized; `walls` (a
/// tolerant build's sink, lite-names' wall map) collects per-element walls instead of throwing on the first.
pub fn normalize(
    parsed: &ParsedModel,
    model: &ModelIndex,
    mut walls: Option<&mut LinkedMap<JStr, JStr>>,
) -> Result<NormalizedModel, Thrown> {
    require_qualified_properties_adopted(parsed, walls.as_deref())?;
    // E.5 FIRST: the store views as lifted functions -- ONE owner of every view's body
    let views = LiftedViews::new(parsed, model);
    views.lift_all(walls.as_deref_mut())?;
    let normalized = mapping_normalizer::normalize(parsed, model, walls, &views)?;
    let mut lifted: Vec<Arc<FunctionDefinition>> = Vec::new();
    lift_derived_properties(parsed, &mut lifted)?; // E.2
    lift_constraints(parsed, &mut lifted)?; // E.3
    lift_defaults(parsed, &mut lifted)?; // E.3b
    lift_function_constraints(parsed, &mut lifted)?; // E.3c
    lift_service_queries(parsed, &mut lifted)?; // E.4
    lifted.extend(views.functions()); // E.5
    let mut elements: Vec<PackageableElement> = Vec::with_capacity(normalized.elements.len() + lifted.len());
    elements.extend(normalized.elements.iter().cloned());
    elements.extend(lifted.into_iter().map(PackageableElement::FunctionDefinition));
    Ok(NormalizedModel {
        elements: list(resolve_synthesized(parsed, elements)?),
        imports: normalized.imports.clone(),
        legacy_surfaces: normalized.legacy_surfaces.clone(),
    })
}

/// `ModelNormalizer.resolveSynthesized` (:151): E.6 -- each synthesized function's body resolved under its owner's
/// imports over ONE universe (the platform's FQNs and every element's, built once); a function rebuilt only when a
/// statement changed.
pub fn resolve_synthesized(
    parsed: &ParsedModel,
    elements: Vec<PackageableElement>,
) -> Result<Vec<PackageableElement>, Thrown> {
    Err(Thrown::Unported("ModelNormalizer.resolveSynthesized"))
}

/// `ModelNormalizer.requireQualifiedPropertiesAdopted` (:199): every association qualified property is held, by
/// identity, by its owning class (the knowledge layer ran); an internal error otherwise (an association the tolerant
/// build walled is exempt).
pub fn require_qualified_properties_adopted(
    parsed: &ParsedModel,
    wall_sink: Option<&LinkedMap<JStr, JStr>>,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("ModelNormalizer.requireQualifiedPropertiesAdopted"))
}

/// `ModelNormalizer.liftDerivedProperties` (:242): E.2, each inline derived property lifted
/// (`lite_names::derived_props::lift`).
pub fn lift_derived_properties(parsed: &ParsedModel, lifted: &mut Vec<Arc<FunctionDefinition>>) -> Result<(), Thrown> {
    Err(Thrown::Unported("ModelNormalizer.liftDerivedProperties"))
}

/// `ModelNormalizer.liftConstraints` (:283): E.3, each inline constraint's predicate and its message's function.
pub fn lift_constraints(parsed: &ParsedModel, lifted: &mut Vec<Arc<FunctionDefinition>>) -> Result<(), Thrown> {
    Err(Thrown::Unported("ModelNormalizer.liftConstraints"))
}

/// `ModelNormalizer.liftFunctionConstraints` (:309): E.3c, a function's pre- and post-conditions as `Boolean[1]`
/// predicates over its parameters (`$return` too for a post-condition), named by the function's id and the
/// condition's place; an internal error for a condition not written inline.
pub fn lift_function_constraints(
    parsed: &ParsedModel,
    lifted: &mut Vec<Arc<FunctionDefinition>>,
) -> Result<(), Thrown> {
    Err(Thrown::Unported("ModelNormalizer.liftFunctionConstraints"))
}

/// `ModelNormalizer.liftDefaults` (:361): E.3b, each property default as `<owner>$default$<property>(): T[m]`.
pub fn lift_defaults(parsed: &ParsedModel, lifted: &mut Vec<Arc<FunctionDefinition>>) -> Result<(), Thrown> {
    Err(Thrown::Unported("ModelNormalizer.liftDefaults"))
}

/// `ModelNormalizer.synthConstraintFunction` (:394): `<owner>$constraint$<name>(this: Owner[1]): Boolean[1]`.
pub fn synth_constraint_function(
    cd: &ClassDefinition,
    c: &p::ConstraintDefinition,
    this_type: &p::TypeExpression,
) -> Result<FunctionDefinition, Thrown> {
    Err(Thrown::Unported("ModelNormalizer.synthConstraintFunction"))
}

/// `ModelNormalizer.synthConstraintMsgFunction` (:414): `<owner>$constraintMsg$<name>(this: Owner[1]): String[1]`.
pub fn synth_constraint_msg_function(
    cd: &ClassDefinition,
    c: &p::ConstraintDefinition,
    this_type: &p::TypeExpression,
) -> Result<FunctionDefinition, Thrown> {
    Err(Thrown::Unported("ModelNormalizer.synthConstraintMsgFunction"))
}

/// `ModelNormalizer.liftServiceQueries` (:441): E.4, each service's query lifted (a bare-FQN query binds a user
/// function and lifts nothing).
pub fn lift_service_queries(parsed: &ParsedModel, lifted: &mut Vec<Arc<FunctionDefinition>>) -> Result<(), Thrown> {
    Err(Thrown::Unported("ModelNormalizer.liftServiceQueries"))
}

/// `ModelNormalizer.synthServiceQuery` (:464): `<svc>$query(<params>): Any[*]`; an untyped lambda parameter is the
/// model's error (E.4's text escapes even a tolerant build).
pub fn synth_service_query(sd: &ServiceDefinition) -> Result<FunctionDefinition, Thrown> {
    Err(Thrown::Unported("ModelNormalizer.synthServiceQuery"))
}
