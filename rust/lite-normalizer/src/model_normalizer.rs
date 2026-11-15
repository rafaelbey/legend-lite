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
//! identity, never a derived `==` (an equal un-adopted property would pass) -- the very definition's `Arc`, which
//! lite-names' adoption shares, compared by address ([`adopted_as`], lite-base's `same`; 2026-11-15, the normalizer
//! batch, item 10).
//!
//! E.4's `ModelException` and E.3c's `IllegalStateException` are not walled under a wall sink: they escape a tolerant
//! build, as in Java (census H4 §1, note (d)).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use lite_base::{Addr, Identity, same};
use lite_error::Phase;
use lite_front::records as p;
use lite_model::function_id::mangle;
use lite_model::linked::{LinkedMap, empty, list};
use lite_model::parsed_model::ImportScopeBuilder;
use lite_model::text::replace;
use lite_model::{
    AssociationDefinition, ClassDefinition, Function, FunctionDefinition, FunctionDefinitionParameterDefinition,
    FunctionDefinitionSynthesized, ImportScope, JStr, NormalizedModel, PackageableElement, ParsedModel,
    ServiceDefinition, SynthHat, cat,
};
use lite_names::resolver::{Universe, platform_fqns, resolve_query_in};
use lite_names::{ModelIndex, derived_props, knowledge, synth_fqn};
use lite_types::Primitive;

use crate::error::{ILLEGAL_STATE, Thrown};
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
    let normalized = mapping_normalizer::normalize(parsed, model, walls.as_deref_mut(), &views)?;
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
        elements: list(resolve_synthesized(parsed, elements, walls)?),
        imports: normalized.imports.clone(),
        legacy_surfaces: normalized.legacy_surfaces.clone(),
    })
}

/// E.6's universe: the platform's FQNs and every element's (Java copies both into one set; membership only).
struct SynthesizedUniverse {
    platform: &'static HashSet<JStr>,
    elements: HashSet<JStr>,
}

impl Universe for SynthesizedUniverse {
    fn contains(&self, name: &JStr) -> bool {
        self.platform.contains(name) || self.elements.contains(name)
    }
}

/// A statement's node identity: the `Arc` of every variant but the two the record holds inline, which have none.
fn node(v: &p::ValueSpecification) -> Option<Addr> {
    use p::ValueSpecification as V;
    Some(match v {
        V::AppliedFunction(x) => x.address(),
        V::AppliedProperty(x) => x.address(),
        V::CBoolean(x) => x.address(),
        V::CDate(x) => x.address(),
        V::CDecimal(x) => x.address(),
        V::CFloat(x) => x.address(),
        V::CInteger(x) => x.address(),
        V::CLatestDate(x) => x.address(),
        V::GraphFetchLiteral(x) => x.address(),
        V::QuotedTreeCall(x) => x.address(),
        V::QuotedGrammarCall(x) => x.address(),
        V::PathLiteral(x) => x.address(),
        V::SqlIsland(x) => x.address(),
        V::GqlIsland(x) => x.address(),
        V::TdsLiteral(x) => x.address(),
        V::CString(x) => x.address(),
        V::CByteArray(x) => x.address(),
        V::CTime(x) => x.address(),
        V::EnumValue(x) => x.address(),
        V::LambdaFunction(x) => x.address(),
        V::NewInstance(x) => x.address(),
        V::NewInstanceCast(x) => x.address(),
        V::PackageableElementPtr(x) => x.address(),
        V::PureCollection(x) => x.address(),
        V::Variable(x) => x.address(),
        V::ColumnInstance(_) | V::TypeAnnotation(_) => return None,
    })
}

/// Java's `a == b` on two statements: the same node; a statement held inline (no node) by its value.
fn same_statement(a: &p::ValueSpecification, b: &p::ValueSpecification) -> bool {
    match (node(a), node(b)) {
        (Some(x), Some(y)) => x == y,
        (None, None) => a == b,
        _ => false,
    }
}

/// `ModelNormalizer.resolveSynthesized` (:151): E.6 -- each synthesized function's body resolved under its owner's
/// imports over ONE universe (the platform's FQNs and every element's, built once); a function rebuilt only when a
/// statement changed.
pub fn resolve_synthesized(
    parsed: &ParsedModel,
    elements: Vec<PackageableElement>,
    mut walls: Option<&mut LinkedMap<JStr, JStr>>,
) -> Result<Vec<PackageableElement>, Thrown> {
    // THE UNIVERSE ONCE per normalization, not once per statement
    let mut universe = SynthesizedUniverse {
        platform: platform_fqns(),
        elements: HashSet::new(),
    };
    for el in &elements {
        universe.elements.insert(el.qualified_name().clone());
    }
    let none: Arc<ImportScope> = ImportScopeBuilder::new().build();
    // `elementImports().getOrDefault(..)`: the side map by key (the first entry of a key, as `linked::get` reads it)
    let mut element_imports: HashMap<&JStr, &Arc<ImportScope>> = HashMap::new();
    for (key, scope) in parsed.element_imports.iter() {
        element_imports.entry(key).or_insert(scope);
    }
    let mut out: Vec<PackageableElement> = Vec::with_capacity(elements.len());
    for el in elements {
        let PackageableElement::FunctionDefinition(fd) = &el else {
            out.push(el);
            continue;
        };
        let Some(from) = &fd.synthesized_from else {
            out.push(el);
            continue;
        };
        // the body was written in its OWNER's section: it resolves under the owner's imports, as the owner's own
        // text did (a PRE/POST body's owner is the function's name, not its key: `none`, as in Java -- census H4 §1)
        let scope = element_imports.get(&from.owner_fqn).copied().unwrap_or(&none);
        let mut body: Vec<p::ValueSpecification> = Vec::with_capacity(fd.body.len());
        let mut changed = false;
        let mut unresolved: Option<Thrown> = None;
        for stmt in fd.body.iter() {
            let resolved = match resolve_query_in(stmt, scope.clone(), &universe) {
                Ok(r) => r,
                Err(e) => {
                    unresolved = Some(e.into());
                    break;
                }
            };
            // Java's `resolved != stmt`, a rebuild detector (census §9.1: answer-neutral either way)
            changed |= !same_statement(&resolved, stmt);
            body.push(resolved);
        }
        if let Some(e) = unresolved {
            // a ResolutionException is the error of the element the body came from: strict, the model's error naming
            // it; tolerant, a wall and the function dropped (2026-11-15, the normalizer batch, item 7c)
            if !e.is_resolution() {
                return Err(e);
            }
            match walls.as_deref_mut() {
                None => {
                    return Err(Thrown::model(
                        Phase::Resolve,
                        e.message_or_null(),
                        Some(&from.owner_fqn),
                    ));
                }
                Some(w) => {
                    w.put_if_absent(from.owner_fqn.clone(), e.first_line());
                    continue;
                }
            }
        }
        if changed {
            // Java's ten-argument constructor (no constraints block; a synthesized function has none)
            out.push(PackageableElement::FunctionDefinition(Arc::new(
                FunctionDefinition::without_constraints(
                    fd.qualified_name.clone(),
                    fd.type_parameters.clone(),
                    fd.multiplicity_parameters.clone(),
                    fd.parameters.clone(),
                    fd.return_type.clone(),
                    fd.return_multiplicity.clone(),
                    list(body),
                    fd.stereotypes.clone(),
                    fd.tagged_values.clone(),
                    fd.synthesized_from.clone(),
                ),
            )));
        } else {
            out.push(el);
        }
    }
    Ok(out)
}

/// Java's `d == dp` (:220): `d` is the association's very definition `dp`, adopted -- the same `Arc`, as lite-names'
/// adoption (`knowledge::adopt_association_qualified_properties`) appends the very definition to its owner (the model
/// holds a derived property definition by `Arc` since 2026-11-15, the normalizer batch, item 10; until then it held a
/// `Box`, the adoption appended a copy, and this was the identity of the body's statement nodes, by value where none).
pub fn adopted_as(d: &Arc<p::DerivedPropertyDefinition>, dp: &Arc<p::DerivedPropertyDefinition>) -> bool {
    same(d, dp)
}

/// `ModelNormalizer.requireQualifiedPropertiesAdopted` (:199): every association qualified property is held, by
/// identity, by its owning class (the knowledge layer ran); an internal error otherwise (an association the tolerant
/// build walled is exempt).
pub fn require_qualified_properties_adopted(
    parsed: &ParsedModel,
    wall_sink: Option<&LinkedMap<JStr, JStr>>,
) -> Result<(), Thrown> {
    let mut classes: Option<LinkedMap<JStr, Arc<ClassDefinition>>> = None;
    for el in parsed.elements.iter() {
        let PackageableElement::AssociationDefinition(ad) = el else {
            continue;
        };
        if ad.derived_properties.is_empty() || wall_sink.is_some_and(|w| w.contains_key(&ad.qualified_name)) {
            continue;
        }
        let classes = classes.get_or_insert_with(|| {
            let mut by_fqn = LinkedMap::new();
            for e in parsed.elements.iter() {
                if let PackageableElement::ClassDefinition(cd) = e {
                    by_fqn.put(cd.qualified_name.clone(), cd.clone());
                }
            }
            by_fqn
        });
        for dp in ad.derived_properties.iter() {
            require_adopted(ad, dp, classes)?;
        }
    }
    Ok(())
}

/// One qualified property of `requireQualifiedPropertiesAdopted`'s loop (:216-225).
fn require_adopted(
    ad: &AssociationDefinition,
    dp: &Arc<p::DerivedPropertyDefinition>,
    classes: &LinkedMap<JStr, Arc<ClassDefinition>>,
) -> Result<(), Thrown> {
    let owner = knowledge::qualified_property_owner(ad, dp).ok_or_else(|| Thrown::never_fired("ModelNormalizer#1"))?;
    // Java's `owner == null` arm is dead (an Optional never holds null)
    if let Some(cd) = classes.get(&owner)
        && !cd.derived_properties.iter().any(|d| adopted_as(d, dp))
    {
        return Err(Thrown::illegal_state(cat!(
            "normalize before the knowledge layer: association '",
            &ad.qualified_name,
            "' qualified property '",
            &dp.name,
            "' is not adopted by its owner (KnowledgeLayer.adoptAssociationQualifiedProperties)"
        )));
    }
    Ok(())
}

/// `ModelNormalizer.liftDerivedProperties` (:242): E.2, each inline derived property lifted
/// (`lite_names::derived_props::lift`).
pub fn lift_derived_properties(parsed: &ParsedModel, lifted: &mut Vec<Arc<FunctionDefinition>>) -> Result<(), Thrown> {
    for el in parsed.elements.iter() {
        let PackageableElement::ClassDefinition(cd) = el else {
            continue;
        };
        for dp in cd.derived_properties.iter() {
            // Only the sugar (inline) form lifts; a Door-4 function-ref binding is already realized by the user's
            // function.
            if matches!(dp.realization, p::Realization::RealizationInline(_)) {
                lifted.push(Arc::new(derived_props::lift(cd, dp)?));
            }
        }
    }
    Ok(())
}

/// `ConstraintDefinition.expression()`: the inline predicate, or Java's `IllegalStateException`.
fn expression(c: &p::ConstraintDefinition) -> Result<&p::ValueSpecification, Thrown> {
    c.expression().map_err(|e| Thrown::java(ILLEGAL_STATE, e.message))
}

/// `Multiplicity.Concrete.PURE_ONE`: `[1]`.
fn pure_one() -> p::Multiplicity {
    p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
        lower_bound: 1,
        upper_bound: Some(1),
    }))
}

/// `Multiplicity.Concrete.ZERO_MANY`: `[*]`.
fn zero_many() -> p::Multiplicity {
    p::Multiplicity::MultiplicityConcrete(Box::new(p::MultiplicityConcrete {
        lower_bound: 0,
        upper_bound: None,
    }))
}

/// `new TypeExpression.NameRef(name)`.
fn name_ref(name: &str) -> p::TypeExpression {
    p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
        name: JStr::from(name),
        pos: None,
    }))
}

/// `new FunctionDefinition.ParameterDefinition(name, type, multiplicity)`.
fn parameter(
    name: JStr,
    r#type: p::TypeExpression,
    multiplicity: p::Multiplicity,
) -> Arc<FunctionDefinitionParameterDefinition> {
    Arc::new(FunctionDefinitionParameterDefinition {
        name,
        r#type,
        multiplicity,
    })
}

/// `new FunctionDefinition(fqn, ..., body, List.of(), List.of()).withSynthesizedFrom(new Synthesized(hat, owner,
/// member))`: a lifted function with no stereotypes, no tagged values and no constraints block.
fn synthesized(
    fqn: JStr,
    type_parameters: Arc<[JStr]>,
    multiplicity_parameters: Arc<[JStr]>,
    parameters: Arc<[Arc<FunctionDefinitionParameterDefinition>]>,
    return_type: p::TypeExpression,
    return_multiplicity: p::Multiplicity,
    body: Vec<p::ValueSpecification>,
    from: FunctionDefinitionSynthesized,
) -> FunctionDefinition {
    FunctionDefinition::user_written(
        fqn,
        type_parameters,
        multiplicity_parameters,
        parameters,
        return_type,
        return_multiplicity,
        list(body),
        empty(),
        empty(),
    )
    .with_synthesized_from(Arc::new(from))
}

/// `ModelNormalizer.liftConstraints` (:283): E.3, each inline constraint's predicate and its message's function.
pub fn lift_constraints(parsed: &ParsedModel, lifted: &mut Vec<Arc<FunctionDefinition>>) -> Result<(), Thrown> {
    for el in parsed.elements.iter() {
        let PackageableElement::ClassDefinition(cd) = el else {
            continue;
        };
        if cd.constraints.is_empty() {
            continue;
        }
        let this_type = derived_props::receiver_type(cd);
        for c in cd.constraints.iter() {
            // Only the sugar (inline) form lifts; a Door-4 ref binding is already realized by the user's predicate
            // function.
            if !matches!(c.realization, p::Realization::RealizationInline(_)) {
                continue;
            }
            lifted.push(Arc::new(synth_constraint_function(cd, c, &this_type)?));
            if c.message.is_some() {
                lifted.push(Arc::new(synth_constraint_msg_function(cd, c, &this_type)?));
            }
        }
    }
    Ok(())
}

/// `ModelNormalizer.liftFunctionConstraints` (:309): E.3c, a function's pre- and post-conditions as `Boolean[1]`
/// predicates over its parameters (`$return` too for a post-condition), named by the function's id and the
/// condition's place; an internal error for a condition not written inline.
pub fn lift_function_constraints(
    parsed: &ParsedModel,
    lifted: &mut Vec<Arc<FunctionDefinition>>,
) -> Result<(), Thrown> {
    for el in parsed.elements.iter() {
        let PackageableElement::FunctionDefinition(fd) = el else {
            continue;
        };
        if fd.is_synthesized() {
            continue;
        }
        // named by the function's id and the condition's place, the id written with '%' for the synthesized names'
        // own sigil (legend-pure's id spells a multiplicity range `$0_1$`)
        let id = mangle(&Function::FunctionDefinition(fd.clone()));
        let owner = JStr(replace(&id.0, "$", "%"));
        for post in [false, true] {
            let conditions = if post {
                &fd.post_constraints
            } else {
                &fd.pre_constraints
            };
            for (i, c) in conditions.iter().enumerate() {
                if !matches!(c.realization, p::Realization::RealizationInline(_)) {
                    return Err(Thrown::illegal_state(cat!(
                        "a function's condition is written inline: ",
                        &c.name,
                        " of ",
                        &fd.qualified_name
                    )));
                }
                let mut params: Vec<Arc<FunctionDefinitionParameterDefinition>> = fd.parameters.to_vec();
                if post {
                    params.push(parameter(
                        JStr::from("return"), // ConstraintDefinition.RETURN
                        fd.return_type.clone(),
                        fd.return_multiplicity.clone(),
                    ));
                }
                let params = list(params);
                let place = JStr::from(i.to_string().as_str());
                let fqn = synth_fqn::condition(&owner, post, &place)?;
                lifted.push(Arc::new(synthesized(
                    fqn,
                    fd.type_parameters.clone(),
                    fd.multiplicity_parameters.clone(),
                    params.clone(),
                    name_ref(Primitive::Boolean.qualified_name()),
                    pure_one(),
                    vec![expression(c)?.clone()],
                    FunctionDefinitionSynthesized::new(
                        if post { SynthHat::POST } else { SynthHat::PRE },
                        fd.qualified_name.clone(),
                        c.name.clone(),
                    ),
                )));
                if let Some(message) = &c.message {
                    let fqn = synth_fqn::constraint_msg(&owner, &cat!(if post { "post" } else { "pre" }, &place))?;
                    lifted.push(Arc::new(synthesized(
                        fqn,
                        fd.type_parameters.clone(),
                        fd.multiplicity_parameters.clone(),
                        params,
                        name_ref(Primitive::String.qualified_name()),
                        pure_one(),
                        vec![message.clone()],
                        FunctionDefinitionSynthesized::new(
                            SynthHat::CONSTRAINT_MSG,
                            fd.qualified_name.clone(),
                            c.name.clone(),
                        ),
                    )));
                }
            }
        }
    }
    Ok(())
}

/// `ModelNormalizer.liftDefaults` (:361): E.3b, each property default as `<owner>$default$<property>(): T[m]`.
pub fn lift_defaults(parsed: &ParsedModel, lifted: &mut Vec<Arc<FunctionDefinition>>) -> Result<(), Thrown> {
    for el in parsed.elements.iter() {
        let PackageableElement::ClassDefinition(cd) = el else {
            continue;
        };
        for pd in cd.properties.iter() {
            let Some(default_value) = &pd.default_value else {
                continue;
            };
            lifted.push(Arc::new(synthesized(
                synth_fqn::default_value(&cd.qualified_name, &pd.name)?,
                cd.type_params.clone(),
                cd.multiplicity_params.clone(),
                empty(),
                pd.r#type.clone(),
                pd.multiplicity.clone(),
                vec![default_value.clone()],
                FunctionDefinitionSynthesized::new(SynthHat::DEFAULT, cd.qualified_name.clone(), pd.name.clone()),
            )));
        }
    }
    Ok(())
}

/// `ModelNormalizer.synthConstraintFunction` (:394): `<owner>$constraint$<name>(this: Owner[1]): Boolean[1]`.
pub fn synth_constraint_function(
    cd: &ClassDefinition,
    c: &p::ConstraintDefinition,
    this_type: &p::TypeExpression,
) -> Result<FunctionDefinition, Thrown> {
    let this = parameter(JStr::from("this"), this_type.clone(), pure_one());
    // the constructor's arguments in Java's order: the name (its refusal first), then the predicate
    let fqn = synth_fqn::constraint(&cd.qualified_name, &c.name)?;
    Ok(synthesized(
        fqn,
        cd.type_params.clone(),
        cd.multiplicity_params.clone(),
        list(vec![this]),
        name_ref(Primitive::Boolean.qualified_name()),
        pure_one(),
        vec![expression(c)?.clone()],
        FunctionDefinitionSynthesized::new(SynthHat::CONSTRAINT, cd.qualified_name.clone(), c.name.clone()),
    ))
}

/// `ModelNormalizer.synthConstraintMsgFunction` (:414): `<owner>$constraintMsg$<name>(this: Owner[1]): String[1]`.
pub fn synth_constraint_msg_function(
    cd: &ClassDefinition,
    c: &p::ConstraintDefinition,
    this_type: &p::TypeExpression,
) -> Result<FunctionDefinition, Thrown> {
    let this = parameter(JStr::from("this"), this_type.clone(), pure_one());
    let fqn = synth_fqn::constraint_msg(&cd.qualified_name, &c.name)?;
    // `List.of(c.message())`: its one caller lifts a constraint with a message
    let message = c.message.clone().ok_or_else(|| Thrown::null_pointer(None))?;
    Ok(synthesized(
        fqn,
        cd.type_params.clone(),
        cd.multiplicity_params.clone(),
        list(vec![this]),
        name_ref(Primitive::String.qualified_name()),
        pure_one(),
        vec![message],
        FunctionDefinitionSynthesized::new(SynthHat::CONSTRAINT_MSG, cd.qualified_name.clone(), c.name.clone()),
    ))
}

/// `ModelNormalizer.liftServiceQueries` (:441): E.4, each service's query lifted (a bare-FQN query binds a user
/// function and lifts nothing).
pub fn lift_service_queries(parsed: &ParsedModel, lifted: &mut Vec<Arc<FunctionDefinition>>) -> Result<(), Thrown> {
    for el in parsed.elements.iter() {
        let PackageableElement::ServiceDefinition(sd) = el else {
            continue;
        };
        // Door 4: a bare-FQN query binds the service to a user function and lifts nothing; any other query
        // expression is the sugar form, lifted to <svc>$query.
        if matches!(sd.function_body, p::ValueSpecification::PackageableElementPtr(_)) {
            continue;
        }
        lifted.push(Arc::new(synth_service_query(sd)?));
    }
    Ok(())
}

/// `ModelNormalizer.synthServiceQuery` (:464): `<svc>$query(<params>): Any[*]`; an untyped lambda parameter is the
/// model's error (E.4's text escapes even a tolerant build).
pub fn synth_service_query(sd: &ServiceDefinition) -> Result<FunctionDefinition, Thrown> {
    let mut params: Vec<Arc<FunctionDefinitionParameterDefinition>> = Vec::new();
    let body: Vec<p::ValueSpecification> = match &sd.function_body {
        p::ValueSpecification::LambdaFunction(lf) => {
            for v in lf.parameters.iter() {
                let (Some(t), Some(m)) = (&v.r#type, &v.multiplicity) else {
                    return Err(Thrown::model(
                        Phase::Normalize,
                        cat!(
                            "Service '",
                            &sd.qualified_name,
                            "' query parameter '",
                            &v.name,
                            "' must be typed (name: Type[mult]) to externalize into ",
                            synth_fqn::query(&sd.qualified_name)?
                        ),
                        None,
                    ));
                };
                params.push(parameter(v.name.clone(), t.clone(), m.clone()));
            }
            lf.body.to_vec()
        }
        other => vec![other.clone()],
    };
    Ok(synthesized(
        synth_fqn::query(&sd.qualified_name)?,
        empty(),
        empty(),
        list(params),
        name_ref(lite_types::platform::ANY),
        zero_many(),
        body,
        FunctionDefinitionSynthesized::new(SynthHat::QUERY, sd.qualified_name.clone(), JStr::from("query")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use lite_front::codec;
    use lite_front::doors::Dialect;
    use lite_model::text::j;

    use crate::test_support::model;

    fn class<'m>(parsed: &'m ParsedModel, fqn: &str) -> &'m Arc<ClassDefinition> {
        parsed
            .elements
            .iter()
            .find_map(|e| match e {
                PackageableElement::ClassDefinition(cd) if cd.qualified_name == j(fqn) => Some(cd),
                _ => None,
            })
            .expect("the class")
    }

    fn names(lifted: &[Arc<FunctionDefinition>]) -> Vec<JStr> {
        lifted.iter().map(|f| f.qualified_name.clone()).collect()
    }

    fn type_name(t: &p::TypeExpression) -> JStr {
        match t {
            p::TypeExpression::TypeExpressionNameRef(n) => n.name.clone(),
            _ => panic!("a name"),
        }
    }

    fn provenance(f: &FunctionDefinition) -> (SynthHat, JStr, JStr) {
        let s = f.synthesized_from.as_ref().expect("synthesized");
        (s.hat, s.owner_fqn.clone(), s.member_name.clone())
    }

    const CLASS: &str = "Class a::P\n[\n  adult: $this.age >= 18,\n  named(~function: $this.name != '' ~message: \
                         'no name')\n]\n{\n  name: String[1];\n  age: Integer[1];\n  nick: String[1] = 'x';\n  \
                         greeting() { $this.name }: String[1];\n}\n";

    #[test]
    fn derived_properties_constraints_and_defaults_lift_in_declaration_order() {
        let parsed = model(CLASS);
        let cd = class(&parsed, "a::P");
        let mut lifted = Vec::new();
        lift_derived_properties(&parsed, &mut lifted).unwrap();
        assert_eq!(names(&lifted), vec![j("a::P$prop$greeting")]);

        let mut lifted = Vec::new();
        lift_constraints(&parsed, &mut lifted).unwrap();
        assert_eq!(
            names(&lifted),
            vec![
                j("a::P$constraint$adult"),
                j("a::P$constraint$named"),
                j("a::P$constraintMsg$named")
            ]
        );
        let adult = &lifted[0];
        assert_eq!(adult.parameters.len(), 1);
        assert_eq!(adult.parameters[0].name, j("this"));
        assert_eq!(type_name(&adult.parameters[0].r#type), j("a::P"));
        assert_eq!(type_name(&adult.return_type), j("meta::pure::metamodel::type::Boolean"));
        assert_eq!(adult.return_multiplicity, pure_one());
        // the lifted body IS the parsed predicate (the same node)
        assert!(same_statement(&adult.body[0], cd.constraints[0].expression().unwrap()));
        assert_eq!(provenance(adult), (SynthHat::CONSTRAINT, j("a::P"), j("adult")));
        let message = &lifted[2];
        assert_eq!(
            type_name(&message.return_type),
            j("meta::pure::metamodel::type::String")
        );
        assert!(same_statement(
            &message.body[0],
            cd.constraints[1].message.as_ref().unwrap()
        ));
        assert_eq!(provenance(message), (SynthHat::CONSTRAINT_MSG, j("a::P"), j("named")));

        let mut lifted = Vec::new();
        lift_defaults(&parsed, &mut lifted).unwrap();
        assert_eq!(names(&lifted), vec![j("a::P$default$nick")]);
        assert!(lifted[0].parameters.is_empty());
        assert_eq!(
            type_name(&lifted[0].return_type),
            j("meta::pure::metamodel::type::String")
        );
        assert_eq!(provenance(&lifted[0]), (SynthHat::DEFAULT, j("a::P"), j("nick")));
    }

    #[test]
    fn a_functions_conditions_lift_by_its_id_and_their_place() {
        let parsed = model(
            "function a::f(x: Integer[0..1]): Integer[1]\n[\n  positive: $x > 0,\n  big(~function: $return > 0 \
             ~message: 'small')\n]\n{\n  1\n}\n",
        );
        let PackageableElement::FunctionDefinition(fd) = &parsed.elements[0] else {
            panic!("the function");
        };
        let owner = JStr(replace(&mangle(&Function::FunctionDefinition(fd.clone())).0, "$", "%"));
        assert!(lite_model::text::contains(&owner.0, "%0_1%"), "{owner}");
        let mut lifted = Vec::new();
        lift_function_constraints(&parsed, &mut lifted).unwrap();
        assert_eq!(
            names(&lifted),
            vec![
                cat!(&owner, "$pre$0"),
                cat!(&owner, "$post$0"),
                cat!(&owner, "$constraintMsg$post0")
            ]
        );
        assert_eq!(lifted[0].parameters.len(), 1);
        assert_eq!(provenance(&lifted[0]), (SynthHat::PRE, j("a::f"), j("positive")));
        // a post-condition reads the result as `$return`
        let post = &lifted[1];
        assert_eq!(post.parameters.len(), 2);
        assert_eq!(post.parameters[1].name, j("return"));
        assert_eq!(type_name(&post.return_type), j("meta::pure::metamodel::type::Boolean"));
        assert_eq!(provenance(post), (SynthHat::POST, j("a::f"), j("big")));
        assert_eq!(provenance(&lifted[2]), (SynthHat::CONSTRAINT_MSG, j("a::f"), j("big")));
        assert_eq!(lifted[2].parameters.len(), 2);

        // a condition bound by reference is an internal error; a synthesized function's conditions are not read
        let by_ref = Arc::new(FunctionDefinition {
            pre_constraints: list(vec![Box::new(p::ConstraintDefinition {
                realization: p::Realization::RealizationRef(Box::new(p::RealizationRef {
                    function_fqn: j("a::g"),
                    source: None,
                })),
                ..(*fd.pre_constraints[0]).clone()
            })]),
            ..(**fd).clone()
        });
        let with = |f: Arc<FunctionDefinition>| ParsedModel {
            elements: list(vec![PackageableElement::FunctionDefinition(f)]),
            ..parsed.clone()
        };
        let e = lift_function_constraints(&with(by_ref.clone()), &mut Vec::new()).unwrap_err();
        assert_eq!(e.java_class(), ILLEGAL_STATE);
        assert_eq!(
            e.message(),
            Some(j("a function's condition is written inline: positive of a::f"))
        );
        let synthesized = Arc::new(
            by_ref.with_synthesized_from(Arc::new(FunctionDefinitionSynthesized::new(
                SynthHat::PROP,
                j("a::X"),
                j("y"),
            ))),
        );
        let mut none = Vec::new();
        lift_function_constraints(&with(synthesized), &mut none).unwrap();
        assert!(none.is_empty());
    }

    const SERVICES: &str = "Class model::Person\n{\n  name: String[1];\n}\nfunction my::q(): Any[*] { 1 }\n\n\
                            ###Service\nService my::Bare\n{\n  pattern: '/a';\n  documentation: '';\n  execution: \
                            Single\n  {\n    query: |model::Person.all();\n    mapping: my::M;\n    runtime: my::R;\n  \
                            }\n}\nService my::Typed\n{\n  pattern: '/b/{n}';\n  documentation: '';\n  execution: \
                            Single\n  {\n    query: {n: String[1] | model::Person.all()->filter(p | $p.name == $n)};\n    \
                            mapping: my::M;\n    runtime: my::R;\n  }\n}\nService my::ByRef\n{\n  pattern: '/c';\n  \
                            documentation: '';\n  execution: Single\n  {\n    query: my::q;\n    mapping: my::M;\n    \
                            runtime: my::R;\n  }\n}\n";

    #[test]
    fn a_service_query_lifts_unless_it_names_a_function() {
        let parsed = model(SERVICES);
        let mut lifted = Vec::new();
        lift_service_queries(&parsed, &mut lifted).unwrap();
        assert_eq!(names(&lifted), vec![j("my::Bare$query"), j("my::Typed$query")]);
        assert!(lifted[0].parameters.is_empty());
        assert_eq!(type_name(&lifted[0].return_type), j("meta::pure::metamodel::type::Any"));
        assert_eq!(lifted[0].return_multiplicity, zero_many());
        assert_eq!(provenance(&lifted[0]), (SynthHat::QUERY, j("my::Bare"), j("query")));
        let typed = &lifted[1];
        assert_eq!(typed.parameters.len(), 1);
        assert_eq!(typed.parameters[0].name, j("n"));
        // the body is the lambda's, unwrapped
        let Some(PackageableElement::ServiceDefinition(sd)) =
            parsed.elements.iter().find(|e| e.qualified_name() == &j("my::Typed"))
        else {
            panic!("the service");
        };
        let p::ValueSpecification::LambdaFunction(lf) = &sd.function_body else {
            panic!("a lambda");
        };
        assert!(same_statement(&typed.body[0], &lf.body[0]));

        // an untyped parameter is the model's error, with no element
        let untyped = Arc::new(ServiceDefinition {
            function_body: p::ValueSpecification::LambdaFunction(Arc::new(p::LambdaFunction {
                parameters: list(vec![Arc::new(p::Variable {
                    r#type: None,
                    ..(*lf.parameters[0]).clone()
                })]),
                ..(**lf).clone()
            })),
            ..(**sd).clone()
        });
        let e = synth_service_query(&untyped).unwrap_err();
        assert!(e.is_model());
        assert_eq!(e.phase(), Some(Phase::Normalize));
        assert_eq!(e.element(), None);
        assert_eq!(
            e.message(),
            Some(j(
                "Service 'my::Typed' query parameter 'n' must be typed (name: Type[mult]) to externalize into \
                 my::Typed$query"
            ))
        );
    }

    const ASSOCIATION: &str = "Class a::P {}\nClass a::F {}\nAssociation a::PF { p: a::P[*]; f: a::F[1]; q() { \
                               $this.p->at(0) }: a::P[1]; }\n";

    fn adopted(text: &str) -> ParsedModel {
        lite_names::knowledge::adopt_association_qualified_properties(&model(text), None).unwrap()
    }

    #[test]
    fn a_qualified_property_must_be_held_by_its_owner_as_adopted() {
        let parsed = adopted(ASSOCIATION);
        assert_eq!(require_qualified_properties_adopted(&parsed, None), Ok(()));
        // not adopted: the pipeline-order bug, loud
        let raw = model(ASSOCIATION);
        let e = require_qualified_properties_adopted(&raw, None).unwrap_err();
        assert_eq!(e.java_class(), ILLEGAL_STATE);
        assert_eq!(
            e.message(),
            Some(j(
                "normalize before the knowledge layer: association 'a::PF' qualified property 'q' is not adopted by \
                 its owner (KnowledgeLayer.adoptAssociationQualifiedProperties)"
            ))
        );
        // an association a tolerant build walled is exempt
        let mut walls = LinkedMap::new();
        walls.put(j("a::PF"), j("walled"));
        assert_eq!(require_qualified_properties_adopted(&raw, Some(&walls)), Ok(()));
        // an owner holding an EQUAL definition that is not the association's (another parse's adoption) is not
        // adopted: identity, never a derived `==`
        let other = adopted(ASSOCIATION);
        let mixed = ParsedModel {
            elements: list(
                parsed
                    .elements
                    .iter()
                    .map(|e| match e {
                        PackageableElement::ClassDefinition(cd) if cd.qualified_name == j("a::F") => {
                            PackageableElement::ClassDefinition(class(&other, "a::F").clone())
                        }
                        _ => e.clone(),
                    })
                    .collect(),
            ),
            ..parsed.clone()
        };
        assert_eq!(
            class(&mixed, "a::F").derived_properties,
            class(&parsed, "a::F").derived_properties
        );
        assert!(require_qualified_properties_adopted(&mixed, None).is_err());
    }

    #[test]
    fn a_reference_body_and_an_empty_body_pass_once_adopted() {
        // a one-element body binds by reference: its source node survives the adoption's copy
        let by_ref = "Class a::P {}\nClass a::F {}\nAssociation a::PF { p: a::P[*]; f: a::F[1]; q() { a::P }: \
                      a::P[1]; }\n";
        let parsed = adopted(by_ref);
        let ad = parsed
            .elements
            .iter()
            .find_map(|e| match e {
                PackageableElement::AssociationDefinition(a) => Some(a.clone()),
                _ => None,
            })
            .expect("the association");
        assert!(matches!(
            ad.derived_properties[0].realization,
            p::Realization::RealizationRef(_)
        ));
        assert_eq!(require_qualified_properties_adopted(&parsed, None), Ok(()));
        assert!(require_qualified_properties_adopted(&model(by_ref), None).is_err());
        // an empty body has no node: compared by value
        let empty_body = "Class a::P {}\nClass a::F {}\nAssociation a::PF { p: a::P[*]; f: a::F[1]; q() { }: \
                          a::P[1]; }\n";
        assert_eq!(require_qualified_properties_adopted(&adopted(empty_body), None), Ok(()));
        assert!(require_qualified_properties_adopted(&model(empty_body), None).is_err());
    }

    fn statement(text: &str) -> p::ValueSpecification {
        let units: Vec<u16> = text.encode_utf16().collect();
        lite_front::spec_parser::parse(&units, Dialect::LegendLite).expect("the expression parses")
    }

    #[test]
    fn a_synthesized_body_resolves_under_its_owners_imports_and_is_rebuilt_only_when_it_changes() {
        let parsed = model("import a::*;\nClass a::P\n{\n  name: String[1];\n}\n");
        assert!(!parsed.element_imports.is_empty());
        let lift = |name: &str, body: &str| {
            Arc::new(
                FunctionDefinition::user_written(
                    j(name),
                    empty(),
                    empty(),
                    empty(),
                    name_ref("meta::pure::metamodel::type::Any"),
                    zero_many(),
                    list(vec![statement(body)]),
                    empty(),
                    empty(),
                )
                .with_synthesized_from(Arc::new(FunctionDefinitionSynthesized::new(
                    SynthHat::QUERY,
                    j("a::P"),
                    j("query"),
                ))),
            )
        };
        let bare = lift("a::P$x", "P.all()");
        let settled = lift("a::P$y", "1");
        let user = Arc::new(FunctionDefinition {
            synthesized_from: None,
            ..(*lift("a::u", "P.all()")).clone()
        });
        let mut elements: Vec<PackageableElement> = parsed.elements.to_vec();
        elements.extend(
            [bare.clone(), settled.clone(), user.clone()]
                .into_iter()
                .map(PackageableElement::FunctionDefinition),
        );
        let out = resolve_synthesized(&parsed, elements, None).unwrap();
        let function = |i: usize| match &out[i] {
            PackageableElement::FunctionDefinition(f) => f.clone(),
            _ => panic!("a function"),
        };
        let resolved = function(1);
        assert!(!same(&resolved, &bare));
        assert!(codec::write(&resolved.body[0], false).contains("a::P"));
        assert_eq!(resolved.synthesized_from, bare.synthesized_from);
        assert!(same(&function(2), &settled));
        // a user function is not this pass's (its body was resolved with its text)
        assert!(same(&function(3), &user));
    }
}
