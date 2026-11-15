// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! MappingNormalizer family 19 (EF_CENSUS §1.1): instance construction (`new(@C, ^C(...))`), the `trustOne` and
//! `to(get, @Prim)` wrappings of a to-one declared property, and `'true'`/`'false'` CASE literals as Boolean literals.
//! Lite-own emissions with no register row (ASK 12): port as Java has them.

use std::sync::Arc;

use lite_builtin::native_fn::RowGetter;
use lite_builtin::natives::IF__BOOLEAN_1__FUNCTION_1__FUNCTION_1;
use lite_builtin::pure::lite;
use lite_front::records as p;
use lite_front::records::ValueSpecification as VS;
use lite_model::JStr;
use lite_model::linked::empty;
use lite_model::text;
use lite_names::{ModelIndex, calls, resolved_names};
use lite_platform::CoreFn;

use super::Fields;
use crate::error::{Thrown, require};

/// `MappingNormalizer.buildNewInstance` (:2729): `new(@C, ^C(fields))`; loud (a `NullPointerException`, Java's
/// `requireNonNull`) with no target class.
pub fn build_new_instance(class_fqn: Option<&JStr>, fields: &Fields) -> Result<p::ValueSpecification, Thrown> {
    let fqn_nn = require(class_fqn, Some("instance construction without a target class"))?;
    let bindings: Vec<Box<p::NewInstanceKeyBinding>> = fields
        .iter()
        .map(|(key, value)| {
            Box::new(p::NewInstanceKeyBinding {
                key: key.clone(),
                expression: Box::new(value.clone()),
            })
        })
        .collect();
    Ok(platform(
        "new",
        vec![
            VS::PackageableElementPtr(Arc::new(p::PackageableElementPtr {
                full_path: fqn_nn.clone(),
                pos: None,
            })),
            // `new NewInstance(fqn, List.of(), bindings)`: the common form, no multiplicity arguments
            VS::NewInstance(Arc::new(p::NewInstance {
                class_name: fqn_nn.clone(),
                type_arguments: empty(),
                type_multiplicity_arguments: empty(),
                properties: bindings.into(),
            })),
        ],
    ))
}

/// `MappingNormalizer.simpleTypeName` (:2762): the name after the last `::`.
pub fn simple_type_name(name: &JStr) -> JStr {
    let sep: Vec<u16> = "::".encode_utf16().collect();
    match text::last_index_of(&name.0, &sep) {
        None => name.clone(),
        Some(idx) => JStr::from(&name.0[idx + 2..]),
    }
}

/// `MappingNormalizer.PRIMITIVE_TYPE_NAMES` (:2767), a membership set.
const PRIMITIVE_TYPE_NAMES: [&str; 9] = [
    "Integer",
    "String",
    "Float",
    "Boolean",
    "Decimal",
    "Number",
    "StrictDate",
    "DateTime",
    "Date",
];

fn is_primitive_type_name(name: &JStr) -> bool {
    PRIMITIVE_TYPE_NAMES.iter().any(|t| name.eq_str(t))
}

/// `MappingNormalizer.buildNewInstanceToOne` (:2771): [`build_new_instance`] with each to-one declared property's
/// value wrapped (`trustOne`, a primitive row getter's `to(get, @Prim)`, booleanized CASE literals).
pub fn build_new_instance_to_one(
    class_fqn: Option<&JStr>,
    fields: &Fields,
    model: &ModelIndex,
) -> Result<p::ValueSpecification, Thrown> {
    let knowledge = model.knowledge();
    let cd = knowledge.hierarchy_class(class_fqn);
    let mut wrapped = Fields::new();
    for (name, key) in fields.iter() {
        let prop = match cd {
            None => None,
            Some(cd) => knowledge.property_def(Some(cd), name)?,
        };
        let to_one_declared = prop.as_ref().is_some_and(|pd| {
            matches!(&pd.multiplicity, p::Multiplicity::MultiplicityConcrete(c)
                if c.lower_bound == 1 && c.upper_bound == Some(1))
        });
        let mut v = key.value.clone();
        let pt_name: Option<JStr> = match prop.as_ref().map(|pd| &pd.r#type) {
            Some(p::TypeExpression::TypeExpressionNameRef(ptn)) => Some(ptn.name.clone()),
            _ => None,
        };
        let primitive_name: Option<JStr> = match &pt_name {
            None => None,
            Some(pt) if is_primitive_type_name(pt) => Some(pt.clone()),
            Some(pt)
                if text::starts_with(&pt.0, "meta::pure::metamodel::type::")
                    && is_primitive_type_name(&simple_type_name(pt)) =>
            {
                Some(simple_type_name(pt))
            }
            Some(_) => None,
        };
        let is_get = matches!(&v, VS::AppliedFunction(gf) if gf.function == RowGetter::GET.property());
        if let (true, Some(prim)) = (is_get, &primitive_name) {
            v = platform(
                "to",
                vec![
                    v,
                    VS::TypeAnnotation(p::TypeAnnotation::TypeAnnotationNamed(Arc::new(
                        p::TypeAnnotationNamed {
                            r#type: p::TypeExpression::TypeExpressionNameRef(Box::new(p::TypeExpressionNameRef {
                                name: prim.clone(),
                                pos: None,
                            })),
                            pos: None,
                        },
                    ))),
                ],
            );
        }
        if primitive_name.as_ref().is_some_and(|n| n.eq_str("Boolean")) {
            v = booleanize_case_literals(&v);
        }
        let exempt = match &v {
            VS::AppliedFunction(af) => {
                resolved_names::form(af) == Some(CoreFn::NAVIGATE)
                    || af.function == lite::LEGACY_NAVIGATE
                    || af.function == lite::OTHERWISE
                    // `AppliedFunction.isNew(af)`
                    || af.function == "new"
            }
            _ => false,
        };
        let value = if to_one_declared && !exempt {
            VS::AppliedFunction(Arc::new(calls::exact(&JStr::from(lite::TRUST_ONE), vec![v])))
        } else {
            v
        };
        wrapped.put(
            name.clone(),
            p::KeyExpression {
                value,
                is_add: key.is_add,
                is_local: key.is_local,
            },
        );
    }
    build_new_instance(class_fqn, &wrapped)
}

/// `MappingNormalizer.booleanizeCaseLiterals` (:2826): `'true'`/`'false'` string literals (in a CASE's branches) as
/// Boolean literals.
pub fn booleanize_case_literals(v: &p::ValueSpecification) -> p::ValueSpecification {
    if let VS::CString(cs) = v {
        if cs.value == "true" {
            return VS::CBoolean(Arc::new(p::CBoolean { value: true, pos: None }));
        }
        if cs.value == "false" {
            return VS::CBoolean(Arc::new(p::CBoolean {
                value: false,
                pos: None,
            }));
        }
        return v.clone();
    }
    if let VS::AppliedFunction(af) = v
        && resolved_names::names(af, &IF__BOOLEAN_1__FUNCTION_1__FUNCTION_1.def().qualified_name.lossy())
        && af.parameters.len() == 3
    {
        let mut ps: Vec<VS> = af.parameters.to_vec();
        for slot in ps.iter_mut().take(3).skip(1) {
            if let VS::LambdaFunction(lf) = &*slot
                && !lf.body.is_empty()
            {
                let mut b: Vec<VS> = lf.body.to_vec();
                let last = b.len() - 1;
                b[last] = booleanize_case_literals(&b[last]);
                // Java's two-argument `new LambdaFunction(params, body)`: the branch's position is not kept
                let rebuilt = VS::LambdaFunction(Arc::new(p::LambdaFunction {
                    parameters: lf.parameters.clone(),
                    body: b.into(),
                    pos: None,
                }));
                *slot = rebuilt;
            }
        }
        return VS::AppliedFunction(Arc::new(af.with_parameters(ps.into())));
    }
    v.clone()
}

/// `Calls.platform(name, args)` as a value.
fn platform(name: &str, args: Vec<VS>) -> VS {
    VS::AppliedFunction(Arc::new(calls::platform(&JStr::from(name), args)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{index, model};
    use lite_model::text::j;

    const MODEL: &str = r#"
###Pure
Class a::P
{
  name: String[1];
  nick: String[0..1];
  flag: Boolean[1];
  n: meta::pure::metamodel::type::Integer[1];
}
"#;

    fn key(v: VS) -> p::KeyExpression {
        p::KeyExpression {
            value: v,
            is_add: false,
            is_local: false,
        }
    }

    fn cstring(s: &str) -> VS {
        VS::CString(Arc::new(p::CString {
            value: j(s),
            pos: None,
            multi_line: false,
        }))
    }

    fn bindings(v: &VS) -> Vec<(JStr, VS)> {
        let VS::AppliedFunction(af) = v else {
            panic!("a call: {v:?}")
        };
        assert_eq!(af.function, j("new"));
        let VS::NewInstance(ni) = &af.parameters[1] else {
            panic!("^C(...)")
        };
        ni.properties
            .iter()
            .map(|b| (b.key.clone(), b.expression.value.clone()))
            .collect()
    }

    fn function(v: &VS) -> JStr {
        let VS::AppliedFunction(af) = v else {
            panic!("a call: {v:?}")
        };
        af.function.clone()
    }

    #[test]
    fn simple_type_name_is_the_text_after_the_last_separator() {
        assert_eq!(
            simple_type_name(&j("meta::pure::metamodel::type::Integer")),
            j("Integer")
        );
        assert_eq!(simple_type_name(&j("Integer")), j("Integer"));
        assert_eq!(simple_type_name(&j("a::")), j(""));
    }

    #[test]
    fn no_target_class_is_javas_null_pointer() {
        let e = build_new_instance(None, &Fields::new()).unwrap_err();
        assert_eq!(
            e,
            Thrown::null_pointer(Some("instance construction without a target class"))
        );
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let mut fields = Fields::new();
        fields.put(j("name"), key(cstring("x")));
        assert_eq!(build_new_instance_to_one(None, &fields, &idx).unwrap_err(), e);
    }

    #[test]
    fn a_to_one_declared_binding_is_trusted_and_a_boolean_case_booleanized() {
        let parsed = model(MODEL);
        let idx = index(&parsed);
        let mut fields = Fields::new();
        fields.put(j("name"), key(cstring("x")));
        fields.put(j("nick"), key(cstring("y")));
        let case = VS::AppliedFunction(Arc::new(calls::platform(
            &j("if"),
            vec![
                VS::CBoolean(Arc::new(p::CBoolean { value: true, pos: None })),
                VS::LambdaFunction(Arc::new(p::LambdaFunction {
                    parameters: empty(),
                    body: vec![cstring("true")].into(),
                    pos: None,
                })),
                VS::LambdaFunction(Arc::new(p::LambdaFunction {
                    parameters: empty(),
                    body: vec![cstring("no")].into(),
                    pos: None,
                })),
            ],
        )));
        fields.put(j("flag"), key(case));
        let get = VS::AppliedFunction(Arc::new(calls::platform(
            &JStr::from(RowGetter::GET.property()),
            vec![cstring("row"), cstring("N")],
        )));
        fields.put(j("n"), key(get));
        fields.put(j("undeclared"), key(cstring("z")));
        let built = build_new_instance_to_one(Some(&j("a::P")), &fields, &idx).unwrap();
        let bs = bindings(&built);
        assert_eq!(
            bs.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>(),
            vec![j("name"), j("nick"), j("flag"), j("n"), j("undeclared")]
        );
        // [1]: trustOne; [0..1] and undeclared: as written
        assert_eq!(function(&bs[0].1), j(lite::TRUST_ONE));
        assert_eq!(bs[1].1, cstring("y"));
        assert_eq!(bs[4].1, cstring("z"));
        // the Boolean CASE: trustOne(if(true, {|true}, {|'no'})), the literal booleanized, the other string kept
        let VS::AppliedFunction(trust) = &bs[2].1 else { panic!() };
        let VS::AppliedFunction(iff) = &trust.parameters[0] else {
            panic!()
        };
        let VS::LambdaFunction(then) = &iff.parameters[1] else {
            panic!()
        };
        assert_eq!(
            then.body[0],
            VS::CBoolean(Arc::new(p::CBoolean { value: true, pos: None }))
        );
        let VS::LambdaFunction(other) = &iff.parameters[2] else {
            panic!()
        };
        assert_eq!(other.body[0], cstring("no"));
        // the platform-FQN primitive: trustOne(to(get(..), @Integer))
        let VS::AppliedFunction(trust_n) = &bs[3].1 else {
            panic!()
        };
        assert_eq!(function(&trust_n.parameters[0]), j("to"));
        // an exempt value is not trusted
        let mut nav = Fields::new();
        nav.put(
            j("name"),
            key(VS::AppliedFunction(Arc::new(calls::exact(
                &j(lite::OTHERWISE),
                vec![cstring("a")],
            )))),
        );
        let built = build_new_instance_to_one(Some(&j("a::P")), &nav, &idx).unwrap();
        assert_eq!(function(&bindings(&built)[0].1), j(lite::OTHERWISE));
    }

    #[test]
    fn booleanize_keeps_any_other_string() {
        assert_eq!(
            booleanize_case_literals(&cstring("false")),
            VS::CBoolean(Arc::new(p::CBoolean {
                value: false,
                pos: None
            }))
        );
        assert_eq!(booleanize_case_literals(&cstring("TRUE")), cstring("TRUE"));
    }
}
