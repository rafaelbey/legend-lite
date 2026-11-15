// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ModelJoinNesting` (ModelJoinNesting.java at ad86fa895): a ModelJoin association's nested hops -- the ends'
//! composite pipelines and the nested slot reads. RE-EXPRESSED (census §4): MJN:57's `Set<String[]> hops` (arrays
//! hash by identity: it dedups nothing; the real dedup is :66-67) is a list of pairs. The raw NPE (:118) and the
//! `Map.of` duplicate-key IAE (:59, :117) are ASK 11's; since ad86fa895 the end-variable clash is an internal error
//! with its own text (:58).

use std::collections::HashSet;
use std::sync::Arc;

use lite_builtin::pure::lite;
use lite_front::records as p;
use lite_model::linked::{LinkedMap, empty, list};
use lite_model::text::j;
use lite_model::{
    AssociationDefinition, AssociationDefinitionAssociationEndDefinition, AssociationMapping,
    AssociationMappingModelJoin, ClassMappingRelationFunction, JStr, cat,
};
use lite_names::{ModelIndex, calls};

use crate::error::{Thrown, require};
use crate::mapping_normalizer::name_ref_or_null;
use crate::mapping_validation;
use crate::relation_reads;
use crate::resolved_mapping::ResolvedMapping;
use crate::xstore_pure_ends::{root_end_id, variable, xstore_end_of};

use p::ValueSpecification as V;

/// The nested column reads a ModelJoin's composition records: end variable -> property -> nested property -> slot
/// column (Java's `Map<String, Map<String, Map<String, String>>>`).
pub type NestedCols = LinkedMap<JStr, LinkedMap<JStr, LinkedMap<JStr, JStr>>>;

/// `ModelJoinNesting.Composed` (:38): the ends' composite pipelines and the nested reads.
#[derive(Clone, Debug)]
pub struct Composed {
    pub pipe_a: Option<p::ValueSpecification>,
    pub pipe_b: Option<p::ValueSpecification>,
    pub nested_cols: NestedCols,
}

/// `ModelJoinNesting.compose` (:42): the nested hops of the lambda composed onto the ends' pipelines. (`ad2` is read
/// by nothing here, as in Java.)
#[allow(clippy::too_many_arguments)]
pub fn compose(
    md: &ResolvedMapping<'_>,
    model: &ModelIndex,
    mj: &AssociationMappingModelJoin,
    _ad2: &AssociationDefinition, // Java never reads `ad2` either (ModelJoinNesting.java:43)
    class_a: &JStr,
    class_b: &JStr,
    a_var: &JStr,
    b_var: &JStr,
    rf_by_var: &LinkedMap<JStr, Arc<ClassMappingRelationFunction>>,
    pipe_a0: Option<&p::ValueSpecification>,
    pipe_b0: Option<&p::ValueSpecification>,
) -> Result<Composed, Thrown> {
    // NESTED HOPS ($employees.address.city): the mid property is ANOTHER ModelJoin association -- the END's pipeline
    // composes with the nested target joined in (the engine golden's side subselect: person ⟕ address on
    // Person_Address's condition), and the read rewrites to the nested target's column on the composite row.
    // THE INVARIANT (ruling 29, the normalizer slice's step 1): the two end variables are two names -- compose's one
    // caller, synthesizeModelJoinMapping, refuses a lambda whose parameters share a name before it composes
    if a_var == b_var {
        return Err(Thrown::illegal_state(cat!(
            "ModelJoinNesting.compose: the two end variables are one name '",
            a_var,
            "'; synthesizeModelJoinMapping refuses that before it composes (an internal error)"
        )));
    }
    let mut pipe_a = pipe_a0.cloned();
    let mut pipe_b = pipe_b0.cloned();
    let mut nested_cols = NestedCols::new();
    let mut hops: Vec<(JStr, JStr)> = Vec::new();
    let end_vars: HashSet<JStr> = HashSet::from([a_var.clone(), b_var.clone()]);
    collect_nested_hops(last_statement(&mj.lambda)?, &end_vars, &mut hops);
    for (var, prop) in hops {
        let rf0 = require(rf_by_var.get(&var), None)?;
        let plain_col = rf0.columns.iter().any(|c| c.property == prop && c.column.is_some());
        if plain_col || nested_cols.get(&var).is_some_and(|m| m.contains_key(&prop)) {
            continue;
        }
        let end_cls = if var == *a_var { class_a } else { class_b };
        let nad = model
            .find_association_of(end_cls, &prop)?
            .ok_or_else(|| Thrown::never_fired("ModelJoinNesting#1"))?;
        let nested_cls = if nad.property1.property_name == prop {
            target_class_fqn(&nad.property1)?
        } else {
            target_class_fqn(&nad.property2)?
        };
        // the nested association's ModelJoin: this mapping's, else the first include's (depth-first, include order)
        // that has one, the last of a mapping (until 2026-11-15 this mapping's own alone: the normalizer's second
        // batch, item 1, MJN:85-93)
        let mut nmj: Option<Arc<AssociationMappingModelJoin>> = None;
        for m in md.closure() {
            for am2 in m.association_mappings.iter() {
                if let AssociationMapping::AssociationMappingModelJoin(cand) = am2
                    && model
                        .find_association(&cand.association_name)
                        .is_some_and(|x| x.qualified_name == nad.qualified_name)
                {
                    nmj = Some(cand.clone());
                }
            }
            if nmj.is_some() {
                break;
            }
        }
        let Some(nmj) = nmj else {
            return Err(Thrown::not_implemented(cat!(
                "ModelJoin for '",
                &mj.association_name,
                "': nested hop '$",
                &var,
                ".",
                &prop,
                "' navigates association '",
                &nad.qualified_name,
                "' which has no ModelJoin in this mapping — only ModelJoin-backed nested hops compose yet"
            )));
        };
        let n_end = xstore_end_of(md, &nested_cls, root_end_id(md, &nested_cls).as_ref(), model)?;
        let n_rf = n_end.cols_view.clone();
        // the nested end is the stock NAVIGATE step ("both XStore and ModelJoin are just navigate()") -- the
        // ColSpec-join spelling every @Join property emission uses, so demand / materialization / prefixing are the
        // ONE slot machinery
        if nmj.lambda.parameters.len() != 2 {
            return Err(Thrown::not_implemented(cat!(
                "nested ModelJoin '",
                &nmj.association_name,
                "' needs a 2-param lambda"
            )));
        }
        let np0 = &nmj.lambda.parameters[0];
        let np1 = &nmj.lambda.parameters[1];
        let nt0 = np0.r#type.as_ref().and_then(name_ref_or_null);
        let nt1 = np1.r#type.as_ref().and_then(name_ref_or_null);
        let n_pair = pair_end_vars(
            &nmj.association_name,
            nad,
            end_cls,
            &nested_cls,
            np0,
            np1,
            nt0,
            nt1,
            model,
        )?;
        // two refusals where Java's maps threw until 2026-11-14 (ruling 29), in the order those maps were built: the
        // nested lambda's two parameters one name, as the top-level ModelJoin refuses it; the nested end mapped by a
        // Pure set (no column view), in the top-level ModelJoin's words for a Pure-set end
        if n_pair.0 == n_pair.1 {
            return Err(Thrown::not_implemented(cat!(
                "nested ModelJoin '",
                &nmj.association_name,
                "' needs two distinct lambda parameter names; got [",
                &np0.name,
                ", ",
                &np1.name,
                "]"
            )));
        }
        if n_end.pure {
            return Err(Thrown::not_implemented(cat!(
                "nested ModelJoin '",
                &nmj.association_name,
                "' has a Pure-set end — the property-space route covers XStore only so far (mapping=",
                md.qualified_name(),
                ")"
            )));
        }
        let j0 = variable("_mj0");
        let j1 = variable("_mj1");
        let n_cond = relation_reads::rewrite(
            last_statement(&nmj.lambda)?,
            &LinkedMap::of([(n_pair.0.clone(), j0.clone()), (n_pair.1.clone(), j1.clone())]),
            &LinkedMap::of([
                (n_pair.0.clone(), rf0.clone()),
                (n_pair.1.clone(), require(n_rf.clone(), None)?),
            ]),
            &nmj.association_name,
            md,
            Some(&NestedCols::new()),
            model,
        )?;
        // `List.of(pipe, new ColSpec(prop, {| nEnd.pipeline()}, null), {_mj0, _mj1 | nCond})`, its arguments first
        let slot = V::ColumnInstance(p::ColumnInstance::ColSpec(Arc::new(p::ColSpec {
            name: prop.clone(),
            function1: Some(Arc::new(p::LambdaFunction {
                parameters: empty(),
                body: list(vec![require(n_end.pipeline.clone(), None)?]),
                pos: None,
            })),
            function2: None,
            alias: None,
            args: empty(),
            qualified: false,
            pos: None,
            col_type: None,
            col_type_mult: None,
            stereotypes: empty(),
            tagged_values: empty(),
        })));
        let on = V::LambdaFunction(Arc::new(p::LambdaFunction {
            parameters: list(vec![j0, j1]),
            body: list(vec![n_cond]),
            pos: None,
        }));
        let pipe = if var == *a_var { pipe_a.clone() } else { pipe_b.clone() };
        let composite = V::AppliedFunction(Arc::new(calls::exact(
            &j(lite::JOIN_SLOT),
            vec![require(pipe, None)?, slot, on],
        )));
        if var == *a_var {
            pipe_a = Some(composite);
        } else {
            pipe_b = Some(composite);
        }
        let mut leaf_cols: LinkedMap<JStr, JStr> = LinkedMap::new();
        for nc in require(n_rf.as_ref(), None)?.columns.iter() {
            if let Some(column) = &nc.column {
                leaf_cols.put(nc.property.clone(), column.clone());
            }
        }
        nested_cols
            .get_or_insert_with(var.clone(), LinkedMap::new)
            .put(prop.clone(), leaf_cols);
    }
    Ok(Composed {
        pipe_a,
        pipe_b,
        nested_cols,
    })
}

/// `ModelJoinNesting.pairEndVars` (:169): which lambda parameter is end A's and which end B's (Java's `String[2]`):
/// legend-engine's pairing ([`mapping_validation::engine_pairing`], its first direction) since 2026-11-15 (the
/// normalizer's second batch, item 4 (e)); held -- the exact-type-then-name rule kept -- for a temporal end, ends that are
/// not the association's own classes, and parameter types not spelled as their full paths.
#[allow(clippy::too_many_arguments)]
pub fn pair_end_vars(
    assoc_name: &JStr,
    ad2: &AssociationDefinition,
    class_a: &JStr,
    class_b: &JStr,
    p0: &p::Variable,
    p1: &p::Variable,
    t0: Option<&JStr>,
    t1: Option<&JStr>,
    model: &ModelIndex,
) -> Result<(JStr, JStr), Thrown> {
    let c1 = target_class_fqn(&ad2.property1)?;
    let c2 = target_class_fqn(&ad2.property2)?;
    let direct = class_a == &c1 && class_b == &c2;
    let swapped = class_a == &c2 && class_b == &c1;
    if (direct || swapped)
        && !mapping_validation::temporal_end(&c1, model)?
        && !mapping_validation::temporal_end(&c2, model)?
    {
        // (the source's: property2's class, the target's: property1's); none: held, the rule below answers
        if let Some((src, tgt)) = mapping_validation::engine_pairing(&ad2.property2, &ad2.property1, p0, p1, model)? {
            return Ok(if direct { (tgt, src) } else { (src, tgt) });
        }
    }
    // exact type FQN when the ends differ, else the engine's property-name rule -- never suffix matching
    let distinct = class_a != class_b;
    if distinct && t0 == Some(class_a) && t1 == Some(class_b) {
        return Ok((p0.name.clone(), p1.name.clone()));
    }
    if distinct && t1 == Some(class_a) && t0 == Some(class_b) {
        return Ok((p1.name.clone(), p0.name.clone()));
    }
    if p0.name == ad2.property1.property_name && p1.name == ad2.property2.property_name {
        return Ok((p0.name.clone(), p1.name.clone()));
    }
    if p1.name == ad2.property1.property_name && p0.name == ad2.property2.property_name {
        return Ok((p1.name.clone(), p0.name.clone()));
    }
    let null = j("null");
    Err(Thrown::not_implemented(cat!(
        "ModelJoin for '",
        assoc_name,
        "': lambda params [",
        &p0.name,
        ": ",
        t0.unwrap_or(&null),
        ", ",
        &p1.name,
        ": ",
        t1.unwrap_or(&null),
        "] pair with the association ends neither by exact type nor by the engine's property-name rule"
    )))
}

/// `ModelJoinNesting.collectNestedHops` (:196): `(end variable, property)` for each nested hop the expression reads,
/// in reading order (module note).
pub fn collect_nested_hops(v: &p::ValueSpecification, end_vars: &HashSet<JStr>, out: &mut Vec<(JStr, JStr)>) {
    if let V::AppliedProperty(ap) = v
        && let V::AppliedProperty(mid) = &ap.receiver
        && let V::Variable(var) = &mid.receiver
        && end_vars.contains(&var.name)
    {
        out.push((var.name.clone(), mid.property.clone()));
        return;
    }
    match v {
        V::AppliedFunction(af) => af.parameters.iter().for_each(|x| collect_nested_hops(x, end_vars, out)),
        V::AppliedProperty(ap2) => collect_nested_hops(&ap2.receiver, end_vars, out),
        V::PureCollection(pc) => pc.values.iter().for_each(|x| collect_nested_hops(x, end_vars, out)),
        V::LambdaFunction(lf) => lf.body.iter().for_each(|x| collect_nested_hops(x, end_vars, out)),
        _ => v.children().iter().for_each(|x| collect_nested_hops(x, end_vars, out)),
    }
}

/// `lambda.body().get(lambda.body().size() - 1)`: the lambda's last statement; an empty body (no parsed lambda has
/// one) is the `ArrayIndexOutOfBoundsException` of `List.copyOf`'s empty list.
pub(crate) fn last_statement(lambda: &p::LambdaFunction) -> Result<&p::ValueSpecification, Thrown> {
    relation_reads::statement(&lambda.body, lambda.body.len() as i64 - 1)
}

/// `end.targetClassFqn()` (lite-model's): its one throw, the IllegalStateException, converted by hand (lite-model's
/// `Thrown` names its Java class by a `String`).
pub(crate) fn target_class_fqn(end: &AssociationDefinitionAssociationEndDefinition) -> Result<JStr, Thrown> {
    match end.target_class_fqn() {
        Ok(fqn) => Ok(fqn.clone()),
        Err(lite_model::Thrown::Java(e)) => Err(Thrown::illegal_state(e.message.unwrap_or_default())),
        Err(other) => unreachable!("targetClassFqn throws only its IllegalStateException, not {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xstore_pure_ends::fixture::{model_join, with_mapping};
    use crate::xstore_pure_ends::property;

    const MODEL: &str = r#"
Class t::A { id: Integer[1]; bId: Integer[1]; }
Class t::B { id: Integer[1]; cName: String[1]; }
Class t::C { name: String[1]; }
Class t::S { name: String[1]; }
Association t::A_B { b: t::B[0..1]; as: t::A[*]; }
Association t::B_C { c: t::C[0..1]; bs: t::B[*]; }
###Mapping
Mapping t::M (
  t::C: Pure { ~src t::S name: $src.name }
  t::A_B: ModelJoin { {x: t::A[1], y: t::B[1]|($x.bId == $y.id) && ($y.c.name == 'n')} }
  BC
)
"#;

    fn view(class: &str, cols: &[(&str, &str)]) -> Arc<ClassMappingRelationFunction> {
        Arc::new(ClassMappingRelationFunction {
            class_name: j(class),
            set_id: None,
            extends_set_id: None,
            root: false,
            func_ref: None,
            inline_source: None,
            columns: list(
                cols.iter()
                    .map(|(p, c)| Arc::new(lite_model::ClassMappingRelationFunctionCol::new(j(p), j(c), false)))
                    .collect(),
            ),
            primary_key: empty(),
        })
    }

    /// `t::A_B`'s composition (end A `t::B` as `y`, end B `t::A` as `x`), `bc` its mapping's B_C association mapping.
    fn composed(bc: &str, a_var: &str) -> Result<Composed, Thrown> {
        with_mapping(&MODEL.replace("BC", bc), "t::M", |md, idx| {
            let mj = model_join(md, "t::A_B");
            let ad = idx.find_association(&j("t::A_B")).unwrap();
            let rfs = LinkedMap::of([
                (j("y"), view("t::B", &[("id", "ID"), ("cName", "C_NAME")])),
                (j("x"), view("t::A", &[("id", "ID"), ("bId", "B_ID")])),
            ]);
            let pipe = V::Variable(variable("pipe"));
            compose(
                md,
                idx,
                &mj,
                ad,
                &j("t::B"),
                &j("t::A"),
                &j(a_var),
                &j("x"),
                &rfs,
                Some(&pipe),
                Some(&pipe),
            )
        })
    }

    #[test]
    fn the_end_variables_of_one_name_are_the_internal_error() {
        let e = composed("", "x").unwrap_err();
        assert_eq!(e.java_class(), crate::error::ILLEGAL_STATE);
        assert_eq!(
            e.message(),
            Some(j(
                "ModelJoinNesting.compose: the two end variables are one name 'x'; synthesizeModelJoinMapping \
                    refuses that before it composes (an internal error)"
            ))
        );
    }

    #[test]
    fn a_nested_hop_needs_a_model_join_of_its_association() {
        let e = composed("", "y").unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "ModelJoin for 't::A_B': nested hop '$y.c' navigates association 't::B_C' which has no ModelJoin in \
                    this mapping — only ModelJoin-backed nested hops compose yet"
            ))
        );
    }

    #[test]
    fn a_nested_model_joins_parameters_of_one_name_and_its_pure_end_are_refused_in_that_order() {
        // RefusalsNotCrashesTest's texts (ruling 29)
        let e = composed(
            "t::B_C: ModelJoin { {u: t::B[1], u: t::C[1]|$u.cName == $u.name} }",
            "y",
        )
        .unwrap_err();
        assert!(e.is_not_implemented());
        assert_eq!(
            e.message(),
            Some(j(
                "nested ModelJoin 't::B_C' needs two distinct lambda parameter names; got [u, u]"
            ))
        );
        let e = composed(
            "t::B_C: ModelJoin { {u: t::B[1], v: t::C[1]|$u.cName == $v.name} }",
            "y",
        )
        .unwrap_err();
        assert_eq!(
            e.message(),
            Some(j(
                "nested ModelJoin 't::B_C' has a Pure-set end — the property-space route covers XStore only so far \
                    (mapping=t::M)"
            ))
        );
    }

    fn param(name: &str, class: Option<&str>) -> p::Variable {
        p::Variable {
            name: j(name),
            r#type: class.map(|c| {
                p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef { name: j(c), pos: None }))
            }),
            multiplicity: None,
            pos: None,
        }
    }

    #[test]
    fn the_end_variables_pair_by_type_else_by_the_property_names() {
        with_mapping(&MODEL.replace("BC", ""), "t::M", |_, idx| {
            let ad = idx.find_association(&j("t::A_B")).unwrap();
            let (a, b) = (j("t::B"), j("t::A"));
            let (x, y) = (param("x", Some("t::A")), param("y", Some("t::B")));
            let pair = |p0: &p::Variable, p1: &p::Variable, ca: &JStr, cb: &JStr| {
                let t = |v: &p::Variable| v.r#type.clone();
                let (t0, t1) = (t(p0), t(p1));
                pair_end_vars(
                    &j("t::A_B"),
                    ad,
                    ca,
                    cb,
                    p0,
                    p1,
                    t0.as_ref().and_then(name_ref_or_null),
                    t1.as_ref().and_then(name_ref_or_null),
                    idx,
                )
            };
            assert_eq!(pair(&x, &y, &a, &b).unwrap(), (j("y"), j("x")));
            assert_eq!(pair(&y, &x, &a, &b).unwrap(), (j("y"), j("x")));
            // one class at both ends: the engine's property-name rule
            let (pb, pas) = (param("b", None), param("as", None));
            assert_eq!(pair(&pas, &pb, &a, &a).unwrap(), (j("b"), j("as")));
            // the engine's refusal (since 2026-11-15, the normalizer's second batch, item 4 (e): it was lite's
            // NotImplementedException, "ModelJoin for 't::A_B': lambda params [m: null, n: t::A] pair with the
            // association ends neither by exact type nor by the engine's property-name rule")
            let e = pair(&param("m", None), &param("n", Some("t::A")), &a, &b).unwrap_err();
            assert!(e.is_model());
            assert_eq!(
                e.message(),
                Some(j(
                    "Cannot pair ModelJoin lambda parameters 'm' and 'n' with association properties 'as' and 'b'. \
                     Either type the parameters explicitly (e.g. {m:A[1], n:B[1] | ...}) or rename the parameters to \
                     match the association property names ('as' and 'b')."
                ))
            );
        });
    }

    #[test]
    fn nested_hops_are_collected_in_reading_order_repeats_kept() {
        let var = |n: &str| V::Variable(variable(n));
        let hop = |v: &str, mid: &str, leaf: &str| property(property(var(v), j(mid)), j(leaf));
        let both = V::PureCollection(Arc::new(p::PureCollection {
            values: list(vec![
                hop("x", "b", "id"),
                hop("y", "c", "name"),
                hop("x", "b", "id"),
                hop("z", "q", "r"),
            ]),
            pos: None,
        }));
        let mut out = Vec::new();
        collect_nested_hops(&both, &HashSet::from([j("x"), j("y")]), &mut out);
        assert_eq!(out, vec![(j("x"), j("b")), (j("y"), j("c")), (j("x"), j("b"))]);
    }
}
