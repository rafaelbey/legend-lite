// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The resolver's walkers over the protocol layer (2026-11-12): type expressions, value specifications, stereotypes
//! and tagged values. `None` is Java's `==` input; a rebuilt record is built with the constructor Java's code calls.

use std::sync::Arc;

use lite_model::JStr;
use lite_model::protocol as p;
use lite_model::protocol::ValueSpecification as VS;
use lite_model::{ImportScope, StereotypeApplication, TaggedValue, cat};

use super::{Rs, Scope, qualify_id_by_base, resolve_call_candidates, resolve_name, resolve_profile};
use crate::error::Thrown;

// =====================================================================
// Lists: the input when no element changed, else a copy
// =====================================================================

/// `resolveList(in, fn)` over an `Arc<[T]>`.
pub(super) fn arc_list<T: Clone>(input: &Arc<[T]>, mut f: impl FnMut(&T) -> Rs<T>) -> Rs<Arc<[T]>> {
    Ok(walk(input, &mut f)?.map(Arc::from))
}

/// `resolveList(in, fn)` over a `Vec<T>` (a protocol record's list).
pub(super) fn vec_list<T: Clone>(input: &[T], mut f: impl FnMut(&T) -> Rs<T>) -> Rs<Vec<T>> {
    walk(input, &mut f)
}

fn walk<T: Clone>(input: &[T], f: &mut impl FnMut(&T) -> Rs<T>) -> Rs<Vec<T>> {
    let mut out: Option<Vec<T>> = None;
    for (i, t) in input.iter().enumerate() {
        match f(t)? {
            Some(r) => {
                out.get_or_insert_with(|| input[..i].to_vec()).push(r);
            }
            None => {
                if let Some(v) = out.as_mut() {
                    v.push(t.clone());
                }
            }
        }
    }
    Ok(out)
}

/// A name resolved: `None` when it is the same text.
pub(super) fn name(n: &JStr, scope: &Scope<'_>) -> Rs<JStr> {
    let r = resolve_name(n, scope)?;
    Ok(if &r == n { None } else { Some(r) })
}

/// An optional name resolved (`x == null ? null : resolveName(x)`).
pub(super) fn opt_name(n: &Option<JStr>, scope: &Scope<'_>) -> Rs<Option<JStr>> {
    match n {
        None => Ok(None),
        Some(n) => Ok(name(n, scope)?.map(Some)),
    }
}

/// `resolveFqnList(fqns)`.
pub(super) fn fqn_list(fqns: &Arc<[JStr]>, scope: &Scope<'_>) -> Rs<Arc<[JStr]>> {
    arc_list(fqns, |n| name(n, scope))
}

/// The value or its replacement.
pub(super) fn or<T: Clone>(r: &Option<T>, orig: &T) -> T {
    r.clone().unwrap_or_else(|| orig.clone())
}

// =====================================================================
// Type expressions
// =====================================================================

/// `resolveType(t)`.
pub(super) fn resolve_type(t: &p::TypeExpression, scope: &Scope<'_>) -> Rs<p::TypeExpression> {
    use p::TypeExpression as T;
    Ok(match t {
        T::TypeExpressionNameRef(nr) => name(&nr.name, scope)?.map(|r| {
            T::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
                name: r,
                pos: nr.pos.clone(),
            }))
        }),
        T::TypeExpressionGeneric(g) => {
            let r = name(&g.name, scope)?;
            let args = type_vec(&g.arguments, scope)?;
            // the type-variable values resolve as any value specification (since 2026-11-13: the names fixes, item 4)
            let values = vec_list(&g.type_variable_values, |x| resolve_vs(x, scope))?;
            if r.is_none() && args.is_none() && values.is_none() {
                None
            } else {
                // the rebuild keeps the multiplicity arguments and the position
                Some(T::TypeExpressionGeneric(Box::new(p::TypeExpressionGeneric {
                    name: or(&r, &g.name),
                    arguments: or(&args, &g.arguments),
                    multiplicity_arguments: g.multiplicity_arguments.clone(),
                    type_variable_values: or(&values, &g.type_variable_values),
                    pos: g.pos.clone(),
                })))
            }
        }
        T::TypeExpressionFunctionType(ft) => {
            let params = vec_list(&ft.parameters, |tp| Ok(typed_parameter(tp, scope)?.map(Box::new)))?;
            let result = typed_parameter(&ft.result, scope)?;
            if params.is_none() && result.is_none() {
                None
            } else {
                Some(T::TypeExpressionFunctionType(Box::new(p::TypeExpressionFunctionType {
                    parameters: or(&params, &ft.parameters),
                    result: result.map(Box::new).unwrap_or_else(|| ft.result.clone()),
                })))
            }
        }
        T::TypeExpressionRelationType(rt) => vec_list(&rt.columns, |c| Ok(column(c, scope)?.map(Box::new)))?
            .map(|cols| T::TypeExpressionRelationType(Box::new(p::TypeExpressionRelationType { columns: cols }))),
        T::TypeExpressionSchemaAlgebra(sa) => {
            let l = resolve_type(&sa.left, scope)?;
            let r = resolve_type(&sa.right, scope)?;
            if l.is_none() && r.is_none() {
                None
            } else {
                Some(T::TypeExpressionSchemaAlgebra(Box::new(
                    p::TypeExpressionSchemaAlgebra {
                        left: or(&l, &sa.left),
                        op: sa.op,
                        right: or(&r, &sa.right),
                    },
                )))
            }
        }
    })
}

fn typed_parameter(tp: &p::TypeExpressionTypedParameter, scope: &Scope<'_>) -> Rs<p::TypeExpressionTypedParameter> {
    Ok(
        resolve_type(&tp.r#type, scope)?.map(|t| p::TypeExpressionTypedParameter {
            r#type: t,
            multiplicity: tp.multiplicity.clone(),
        }),
    )
}

fn column(c: &p::TypeExpressionColumn, scope: &Scope<'_>) -> Rs<p::TypeExpressionColumn> {
    Ok(resolve_type(&c.r#type, scope)?.map(|t| p::TypeExpressionColumn {
        name: c.name.clone(),
        r#type: t,
        multiplicity: c.multiplicity.clone(),
        pos: c.pos.clone(),
    }))
}

/// `resolveTypeList(types)` over an `Arc<[T]>`.
pub(super) fn type_list(types: &Arc<[p::TypeExpression]>, scope: &Scope<'_>) -> Rs<Arc<[p::TypeExpression]>> {
    arc_list(types, |t| resolve_type(t, scope))
}

fn type_vec(types: &[p::TypeExpression], scope: &Scope<'_>) -> Rs<Vec<p::TypeExpression>> {
    vec_list(types, |t| resolve_type(t, scope))
}

/// `resolveParameter(p)` (the protocol's `ParameterDefinition`): its type; the rebuild keeps the position.
pub(super) fn parameter(pd: &p::ParameterDefinition, scope: &Scope<'_>) -> Rs<p::ParameterDefinition> {
    Ok(resolve_type(&pd.r#type, scope)?.map(|t| p::ParameterDefinition {
        name: pd.name.clone(),
        r#type: t,
        multiplicity: pd.multiplicity.clone(),
        pos: pd.pos.clone(),
    }))
}

/// `resolveRealization(r)`: a reference's function FQN (the rebuilt `Ref` keeps its source), an inline body's names.
pub(super) fn realization(r: &p::Realization, scope: &Scope<'_>) -> Rs<p::Realization> {
    Ok(match r {
        p::Realization::RealizationRef(rf) => name(&rf.function_fqn, scope)?.map(|fqn| {
            p::Realization::RealizationRef(Box::new(p::RealizationRef {
                function_fqn: fqn,
                source: rf.source.clone(),
            }))
        }),
        p::Realization::RealizationInline(inl) => vec_list(&inl.body, |x| resolve_vs(x, scope))?
            .map(|body| p::Realization::RealizationInline(Box::new(p::RealizationInline { body }))),
    })
}

/// `resolveConstraint(c)`: its realization and `~message`; the rebuild keeps the external id, the owner and the
/// position.
pub(super) fn constraint(c: &p::ConstraintDefinition, scope: &Scope<'_>) -> Rs<p::ConstraintDefinition> {
    let realization = realization(&c.realization, scope)?;
    let message = match &c.message {
        None => None,
        Some(m) => resolve_vs(m, scope)?,
    };
    if realization.is_none() && message.is_none() {
        return Ok(None);
    }
    Ok(Some(p::ConstraintDefinition {
        name: c.name.clone(),
        realization: or(&realization, &c.realization),
        message: match message {
            Some(m) => Some(m),
            None => c.message.clone(),
        },
        enforcement_level: c.enforcement_level.clone(),
        external_id: c.external_id.clone(),
        owner: c.owner.clone(),
        pos: c.pos.clone(),
    }))
}

/// `resolveDerivedProperty(dp)`: parameters, realization, type, and the annotations' profiles; the rebuild keeps the
/// annotations and the position. The annotations are compared by value, as Java's `equals`.
pub(super) fn derived_property(
    dp: &p::DerivedPropertyDefinition,
    scope: &Scope<'_>,
) -> Rs<p::DerivedPropertyDefinition> {
    let params = vec_list(&dp.parameters, |x| Ok(parameter(x, scope)?.map(Box::new)))?;
    let realization = realization(&dp.realization, scope)?;
    let ty = resolve_type(&dp.r#type, scope)?;
    let mut stereotypes: Vec<Box<p::PStereotype>> = Vec::with_capacity(dp.stereotypes.len());
    for st in &dp.stereotypes {
        stereotypes.push(Box::new(st.with_profile(&resolve_profile(&st.profile, scope)?)));
    }
    let mut tagged: Vec<Box<p::PTaggedValue>> = Vec::with_capacity(dp.tagged_values.len());
    for tv in &dp.tagged_values {
        tagged.push(Box::new(tv.with_profile(&resolve_profile(&tv.tag.profile, scope)?)));
    }
    if params.is_none()
        && realization.is_none()
        && ty.is_none()
        && stereotypes == dp.stereotypes
        && tagged == dp.tagged_values
    {
        return Ok(None);
    }
    Ok(Some(p::DerivedPropertyDefinition {
        name: dp.name.clone(),
        parameters: or(&params, &dp.parameters),
        realization: or(&realization, &dp.realization),
        r#type: or(&ty, &dp.r#type),
        multiplicity: dp.multiplicity.clone(),
        stereotypes,
        tagged_values: tagged,
        pos: dp.pos.clone(),
    }))
}

// =====================================================================
// Stereotypes and tagged values
// =====================================================================

/// `resolveStereotypes(apps)`.
pub(super) fn stereotypes(
    apps: &Arc<[Arc<StereotypeApplication>]>,
    scope: &Scope<'_>,
) -> Rs<Arc<[Arc<StereotypeApplication>]>> {
    arc_list(apps, |app| {
        let resolved = resolve_profile(&app.profile_name, scope)?;
        Ok(if resolved == app.profile_name {
            None
        } else {
            Some(Arc::new(StereotypeApplication {
                profile_name: resolved,
                stereotype_name: app.stereotype_name.clone(),
            }))
        })
    })
}

/// `resolveTaggedValues(tvs)`.
pub(super) fn tagged_values(tvs: &Arc<[Arc<TaggedValue>]>, scope: &Scope<'_>) -> Rs<Arc<[Arc<TaggedValue>]>> {
    arc_list(tvs, |tv| {
        let resolved = resolve_profile(&tv.profile_name, scope)?;
        Ok(if resolved == tv.profile_name {
            None
        } else {
            Some(Arc::new(TaggedValue {
                profile_name: resolved,
                tag_name: tv.tag_name.clone(),
                value: tv.value.clone(),
            }))
        })
    })
}

// =====================================================================
// Value specifications
// =====================================================================

/// `resolveVsList(list)`.
pub(super) fn vs_list(list: &Arc<[VS]>, scope: &Scope<'_>) -> Rs<Arc<[VS]>> {
    arc_list(list, |x| resolve_vs(x, scope))
}

/// `resolveVs(vs, scope)`: every value-specification variant, by name.
pub fn resolve_vs(vs: &VS, scope: &Scope<'_>) -> Rs<VS> {
    Ok(match vs {
        // a path literal KEEPS its node (its navigation lambda resolves inside it): always rebuilt
        VS::PathLiteral(pl) => {
            let d = lambda(&pl.desugared, scope)?.unwrap_or_else(|| pl.desugared.clone());
            Some(VS::PathLiteral(Arc::new(pl.with_desugared(d))))
        }
        VS::CByteArray(_) | VS::SqlIsland(_) | VS::GqlIsland(_) => None,
        // a TDS literal dissolves into its desugared tds(...) call
        VS::TdsLiteral(tl) => {
            let d = VS::AppliedFunction(tl.desugared.clone());
            Some(resolve_vs(&d, scope)?.unwrap_or(d))
        }
        // a graph-fetch tree KEEPS its node: its call arguments resolve as expressions, its class names in place
        VS::GraphFetchLiteral(gf) => Some(VS::GraphFetchLiteral(Arc::new(graph_fetch(gf, scope)?))),
        // the tree resolves in the SECTIONLESS scope (the core imports, no section imports) over the same universe: the
        // engine compiles the quoted string as a section of its own with no imports (LegendCompile.doCompileVS). Not
        // walked until 2026-11-13: the names fixes, item 4
        VS::QuotedTreeCall(q) => {
            let original = applied_function_arc(&q.original, scope)?;
            let sectionless = Scope::prelude_of(ImportScope::empty(), scope.known);
            let tree = resolve_vs(&q.tree, &sectionless)?.unwrap_or_else(|| q.tree.clone());
            Some(VS::QuotedTreeCall(Arc::new(p::QuotedTreeCall {
                original,
                tree,
                pos: q.pos.clone(),
            })))
        }
        // the grammar carrier's lambdas resolve in the enclosing scope
        VS::QuotedGrammarCall(q) => {
            let original = applied_function_arc(&q.original, scope)?;
            let mut functions: Vec<Arc<p::LambdaFunction>> = Vec::with_capacity(q.functions.len());
            for f in q.functions.iter() {
                functions.push(lambda(f, scope)?.unwrap_or_else(|| f.clone()));
            }
            Some(VS::QuotedGrammarCall(Arc::new(p::QuotedGrammarCall {
                original,
                functions: functions.into(),
                pos: q.pos.clone(),
            })))
        }
        VS::PackageableElementPtr(ptr) => {
            // a UNIT reference Measure~unit: the measure resolves through the tiers, the unit part rides along
            let tilde = ptr.full_path.0.iter().position(|&u| u == u16::from(b'~'));
            let r = match tilde {
                Some(t) if t > 0 => {
                    let base = resolve_name(&JStr::from(&ptr.full_path.0[..t]), scope)?;
                    cat!(base, &ptr.full_path.0[t..])
                }
                // a function referenced BY ITS SIGNATURE ID resolves like any element name; an id nobody declares
                // still names its base function, qualified under the first tier that knows it
                _ => qualify_id_by_base(resolve_name(&ptr.full_path, scope)?, scope),
            };
            if r == ptr.full_path {
                None
            } else {
                Some(VS::PackageableElementPtr(Arc::new(p::PackageableElementPtr {
                    full_path: r,
                    pos: ptr.pos.clone(),
                })))
            }
        }
        VS::EnumValue(ev) => name(&ev.full_path, scope)?.map(|r| {
            // the positions and the wire spelling ride the rebuild
            VS::EnumValue(Arc::new(p::EnumValue {
                full_path: r,
                value: ev.value.clone(),
                enumeration_pos: ev.enumeration_pos.clone(),
                pos: ev.pos.clone(),
                node: ev.node,
            }))
        }),
        VS::AppliedFunction(af) => applied_function(af, scope)?.map(|a| VS::AppliedFunction(Arc::new(a))),
        VS::AppliedProperty(ap) => resolve_vs(&ap.receiver, scope)?.map(|receiver| {
            VS::AppliedProperty(Arc::new(p::AppliedProperty {
                receiver,
                property: ap.property.clone(),
                pos: ap.pos.clone(),
                owner_class: ap.owner_class.clone(),
            }))
        }),
        VS::LambdaFunction(lf) => lambda(lf, scope)?.map(VS::LambdaFunction),
        VS::Variable(v) => variable(v, scope)?.map(VS::Variable),
        VS::NewInstance(ni) => {
            let class_name = name(&ni.class_name, scope)?;
            let type_args = type_list(&ni.type_arguments, scope)?;
            let props = key_bindings(&ni.properties, scope)?;
            if class_name.is_none() && type_args.is_none() && props.is_none() {
                None
            } else {
                Some(VS::NewInstance(Arc::new(p::NewInstance {
                    class_name: or(&class_name, &ni.class_name),
                    type_arguments: or(&type_args, &ni.type_arguments),
                    type_multiplicity_arguments: ni.type_multiplicity_arguments.clone(),
                    properties: or(&props, &ni.properties),
                })))
            }
        }
        VS::NewInstanceCast(nic) => {
            let class_name = name(&nic.class_name, scope)?;
            let type_args = type_list(&nic.type_arguments, scope)?;
            let src = resolve_vs(&nic.src, scope)?;
            if class_name.is_none() && type_args.is_none() && src.is_none() {
                None
            } else {
                Some(VS::NewInstanceCast(Arc::new(p::NewInstanceCast {
                    class_name: or(&class_name, &nic.class_name),
                    type_arguments: or(&type_args, &nic.type_arguments),
                    src: or(&src, &nic.src),
                    target_set_id: nic.target_set_id.clone(),
                })))
            }
        }
        VS::PureCollection(coll) => vs_list(&coll.values, scope)?.map(|values| {
            VS::PureCollection(Arc::new(p::PureCollection {
                values,
                pos: coll.pos.clone(),
            }))
        }),
        VS::ColumnInstance(p::ColumnInstance::ColSpec(cs)) => {
            col_spec(cs, scope)?.map(|c| VS::ColumnInstance(p::ColumnInstance::ColSpec(c)))
        }
        VS::ColumnInstance(p::ColumnInstance::ColSpecArray(csa)) => arc_list(&csa.col_specs, |c| col_spec(c, scope))?
            .map(|out| {
                VS::ColumnInstance(p::ColumnInstance::ColSpecArray(Arc::new(p::ColSpecArray {
                    col_specs: out,
                    pos: csa.pos.clone(),
                })))
            }),
        VS::TypeAnnotation(ta) => type_annotation(ta, scope)?.map(VS::TypeAnnotation),
        VS::CBoolean(_)
        | VS::CDate(_)
        | VS::CDecimal(_)
        | VS::CFloat(_)
        | VS::CInteger(_)
        | VS::CLatestDate(_)
        | VS::CString(_)
        | VS::CTime(_) => None,
    })
}

/// An `AppliedFunction` node resolved and handed back as a node (the quoted carriers' `(AppliedFunction)
/// resolveVs(original)`).
fn applied_function_arc(af: &Arc<p::AppliedFunction>, scope: &Scope<'_>) -> Result<Arc<p::AppliedFunction>, Thrown> {
    Ok(applied_function(af, scope)?.map_or_else(|| af.clone(), Arc::new))
}

/// The call arm: IDEMPOTENT on a call carrying its record (only its parameters resolve again); else the names decided
/// (`decide`), then the parameters; the rebuild keeps the position and every written form.
fn applied_function(af: &p::AppliedFunction, scope: &Scope<'_>) -> Rs<p::AppliedFunction> {
    if !af.referents.is_empty() {
        return Ok(vs_list(&af.parameters, scope)?.map(|again| af.with_parameters(again)));
    }
    let named = super::decide(
        &af.function,
        resolve_call_candidates(&af.function, scope),
        scope.prelude,
        af.parameters.len(),
    );
    let params = vs_list(&af.parameters, scope)?;
    if named.function == af.function && params.is_none() && named.referents.is_empty() {
        return Ok(None);
    }
    Ok(Some(p::AppliedFunction {
        function: named.function,
        parameters: or(&params, &af.parameters),
        referents: named.referents.into(),
        pos: af.pos.clone(),
        property_call: af.property_call,
        grouped: af.grouped,
        infix: af.infix,
        island: af.island,
        f_control: af.f_control.clone(),
        owner_class: af.owner_class.clone(),
    }))
}

/// `resolveLambda(lf)`: its parameters' types and its body; the rebuild keeps the position.
pub(super) fn lambda(lf: &p::LambdaFunction, scope: &Scope<'_>) -> Rs<Arc<p::LambdaFunction>> {
    let params = arc_list(&lf.parameters, |v| variable(v, scope))?;
    let body = vs_list(&lf.body, scope)?;
    if params.is_none() && body.is_none() {
        return Ok(None);
    }
    Ok(Some(Arc::new(p::LambdaFunction {
        parameters: or(&params, &lf.parameters),
        body: or(&body, &lf.body),
        pos: lf.pos.clone(),
    })))
}

/// `resolveVariable(v)`: its type; the rebuild keeps the position.
fn variable(v: &p::Variable, scope: &Scope<'_>) -> Rs<Arc<p::Variable>> {
    let Some(t) = &v.r#type else { return Ok(None) };
    Ok(resolve_type(t, scope)?.map(|t| {
        Arc::new(p::Variable {
            name: v.name.clone(),
            r#type: Some(t),
            multiplicity: v.multiplicity.clone(),
            pos: v.pos.clone(),
        })
    }))
}

/// `resolveKeyBindings(props)`: each binding's value; a binding whose value changed is rebuilt with `withValue`.
fn key_bindings(
    props: &Arc<[Box<p::NewInstanceKeyBinding>]>,
    scope: &Scope<'_>,
) -> Rs<Arc<[Box<p::NewInstanceKeyBinding>]>> {
    if props.is_empty() {
        return Ok(None);
    }
    arc_list(props, |e| {
        Ok(resolve_vs(&e.expression.value, scope)?.map(|r| {
            Box::new(p::NewInstanceKeyBinding {
                key: e.key.clone(),
                expression: Box::new(e.expression.with_value(r)),
            })
        }))
    })
}

/// `resolveColSpec(cs)`: its lambdas and its qualifier arguments; the rebuild keeps the position, the column's declared
/// type and multiplicity and its annotations.
fn col_spec(cs: &p::ColSpec, scope: &Scope<'_>) -> Rs<Arc<p::ColSpec>> {
    let r1 = match &cs.function1 {
        None => None,
        Some(f) => lambda(f, scope)?,
    };
    let r2 = match &cs.function2 {
        None => None,
        Some(f) => lambda(f, scope)?,
    };
    let ra = vs_list(&cs.args, scope)?;
    if r1.is_none() && r2.is_none() && ra.is_none() {
        return Ok(None);
    }
    Ok(Some(Arc::new(p::ColSpec {
        name: cs.name.clone(),
        function1: r1.or_else(|| cs.function1.clone()),
        function2: r2.or_else(|| cs.function2.clone()),
        alias: cs.alias.clone(),
        args: or(&ra, &cs.args),
        qualified: cs.qualified,
        pos: cs.pos.clone(),
        col_type: cs.col_type.clone(),
        col_type_mult: cs.col_type_mult.clone(),
        stereotypes: cs.stereotypes.clone(),
        tagged_values: cs.tagged_values.clone(),
    })))
}

/// `resolveTypeAnnotation(ta)`.
fn type_annotation(ta: &p::TypeAnnotation, scope: &Scope<'_>) -> Rs<p::TypeAnnotation> {
    Ok(match ta {
        p::TypeAnnotation::TypeAnnotationNamed(named) => resolve_type(&named.r#type, scope)?.map(|t| {
            p::TypeAnnotation::TypeAnnotationNamed(Arc::new(p::TypeAnnotationNamed {
                r#type: t,
                pos: named.pos.clone(),
            }))
        }),
        p::TypeAnnotation::TypeAnnotationWildcard(_) | p::TypeAnnotation::TypeAnnotationMultiplicityRef(_) => None,
        p::TypeAnnotation::TypeAnnotationRelationShape(shape) => arc_list(&shape.columns, |c| {
            Ok(type_annotation(&c.r#type, scope)?.map(|inner| {
                Box::new(p::TypeAnnotationRelationShapeColumn {
                    name: c.name.clone(),
                    r#type: inner,
                    multiplicity: c.multiplicity.clone(),
                    pos: c.pos.clone(),
                })
            }))
        })?
        .map(|out| {
            p::TypeAnnotation::TypeAnnotationRelationShape(Arc::new(p::TypeAnnotationRelationShape {
                columns: out,
                spelled_name: shape.spelled_name.clone(),
                type_span: shape.type_span.clone(),
                pos: shape.pos.clone(),
            }))
        }),
    })
}

/// `resolveGraphFetch(gf)`: the qualifier call arguments resolve as expressions, then the class names in place; always
/// a new tree.
fn graph_fetch(gf: &p::GraphFetchLiteral, scope: &Scope<'_>) -> Result<p::GraphFetchLiteral, Thrown> {
    let args = gf.arguments();
    let resolved = vec_list(&args, |x| resolve_vs(x, scope))?.unwrap_or(args);
    let with_args = gf
        .with_arguments(&resolved)
        .map_err(|m| Thrown::illegal_argument(JStr::from(m)))?;
    with_args.with_class_names(&mut |n: &JStr| resolve_name(n, scope))
}
