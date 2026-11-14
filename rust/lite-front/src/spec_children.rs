// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ValueSpecification`'s structural recursion (2026-11-14, phase 3's normalizer slice, step 3: the model methods,
//! docs/rust-port/EF_CENSUS.md §8): core's `protocol/spec/ValueSpecification` default methods `children()`,
//! `withChildren(cs)` and `mapChildren(f)`, every arm in Java's order, each rebuild through the constructor Java's arm
//! calls -- so the components Java's rebuild drops are dropped here the same way:
//!
//! - `AppliedProperty` loses its position (`new AppliedProperty(r, p, null, ownerClass)`; the owner class kept);
//! - `LambdaFunction` loses its position (the two-argument constructor);
//! - `ColSpecArray` loses its position (the one-argument constructor);
//! - `ColSpec` loses its stereotypes and tagged values (the nine-argument constructor; position and type kept);
//! - every other rebuild keeps every component (`AppliedFunction` through `withParameters`).
//!
//! Java's `mapChildren` answers the very object (`this`) when `f` answered every child's very object (`r == c`), and a
//! rebuild otherwise -- which differs in what it writes, by the drops above. Identity is not this crate's to read (it
//! reaches no lite-base: rust/lite-guards/crate-layers.txt), so "the very object" is said structurally: `f` answers
//! `None` for "this child, unchanged" and `Some(r)` for a different object (even an equal one, as Java's `r != c` is),
//! and [`ValueSpecification::map_children`] answers `None` for `this` and `Some` for the rebuild. A walker recurses by
//! answering `map_children`'s answer from its own default arm, as Java's walkers do; its caller keeps the node when the
//! answer is `None`. The one place this is not Java's `==`: a closure that hands back the identical object it was given
//! must answer `None`, never `Some` of a clone of it.
//!
//! A child of a typed slot (a lambda, a call, a column spec) is cast as Java casts it; another variant there is Java's
//! `ClassCastException`, which nothing in Java catches, so here it panics, naming the slot. Likewise an index past the
//! children Java reads (`IndexOutOfBoundsException`) and the two refusals of the records' constructors Java's arms call
//! (`GraphFetchLiteral.withArguments`' and `QuotedGrammarCall`'s `IllegalArgumentException`): a wrong count is the
//! caller's bug, and `map_children` never makes one.

use std::convert::Infallible;
use std::sync::Arc;

use crate::records::{
    AppliedFunction, AppliedProperty, ColSpec, ColSpecArray, ColumnInstance, KeyExpression, LambdaFunction,
    NewInstance, NewInstanceCast, NewInstanceKeyBinding, PureCollection, QuotedGrammarCall, QuotedTreeCall, TdsLiteral,
    ValueSpecification,
};

use ValueSpecification as V;

impl ValueSpecification {
    /// `children()`: the direct children, in rebuild order -- the very objects, each `Arc` shared. A lambda's are its
    /// body alone, never its parameters (walkers enter through bodies).
    pub fn children(&self) -> Vec<ValueSpecification> {
        match self {
            V::PathLiteral(pl) => vec![V::LambdaFunction(pl.desugared.clone())],
            V::GraphFetchLiteral(gf) => gf.arguments(),
            V::QuotedTreeCall(q) => vec![V::AppliedFunction(q.original.clone()), q.tree.clone()],
            V::QuotedGrammarCall(q) => {
                let mut cs = Vec::with_capacity(q.functions.len() + 1);
                cs.push(V::AppliedFunction(q.original.clone()));
                cs.extend(q.functions.iter().map(|f| V::LambdaFunction(f.clone())));
                cs
            }
            V::CBoolean(_)
            | V::CByteArray(_)
            | V::CDate(_)
            | V::CDecimal(_)
            | V::CFloat(_)
            | V::CInteger(_)
            | V::CLatestDate(_)
            | V::CString(_)
            | V::CTime(_)
            | V::EnumValue(_)
            | V::PackageableElementPtr(_)
            | V::Variable(_)
            | V::TypeAnnotation(_)
            | V::SqlIsland(_)
            | V::GqlIsland(_) => Vec::new(),
            V::TdsLiteral(tl) => vec![V::AppliedFunction(tl.desugared.clone())],
            V::AppliedFunction(af) => af.parameters.to_vec(),
            V::AppliedProperty(ap) => vec![ap.receiver.clone()],
            V::LambdaFunction(lf) => lf.body.to_vec(),
            V::NewInstance(ni) => ni.properties.iter().map(|b| b.expression.value.clone()).collect(),
            V::NewInstanceCast(nc) => vec![nc.src.clone()],
            V::PureCollection(pc) => pc.values.to_vec(),
            V::ColumnInstance(ColumnInstance::ColSpecArray(ca)) => ca
                .col_specs
                .iter()
                .map(|c| V::ColumnInstance(ColumnInstance::ColSpec(c.clone())))
                .collect(),
            V::ColumnInstance(ColumnInstance::ColSpec(cs)) => {
                let mut out = Vec::with_capacity(cs.args.len() + 2);
                if let Some(f1) = &cs.function1 {
                    out.push(V::LambdaFunction(f1.clone()));
                }
                if let Some(f2) = &cs.function2 {
                    out.push(V::LambdaFunction(f2.clone()));
                }
                out.extend(cs.args.iter().cloned());
                out
            }
        }
    }

    /// `withChildren(cs)`: this node with its direct children replaced (`cs` in [`children`](Self::children)' order),
    /// rebuilt as Java's arm rebuilds it; a leaf answers itself (its very `Arc`).
    pub fn with_children(&self, cs: Vec<ValueSpecification>) -> ValueSpecification {
        match self {
            V::CByteArray(_) => self.clone(),
            V::PathLiteral(pl) => V::PathLiteral(Arc::new(pl.with_desugared(lambda(&cs, 0)))),
            V::GraphFetchLiteral(gf) => match gf.with_arguments(&cs) {
                Ok(rebuilt) => V::GraphFetchLiteral(Arc::new(rebuilt)),
                Err(message) => panic!("java.lang.IllegalArgumentException: {message}"),
            },
            V::QuotedTreeCall(q) => V::QuotedTreeCall(Arc::new(QuotedTreeCall {
                original: applied(&cs, 0),
                tree: child(&cs, 1).clone(),
                pos: q.pos.clone(),
            })),
            V::QuotedGrammarCall(q) => {
                let original = applied(&cs, 0);
                let functions: Vec<Arc<LambdaFunction>> = (1..cs.len()).map(|i| lambda(&cs, i)).collect();
                if functions.is_empty() {
                    panic!(
                        "java.lang.IllegalArgumentException: a folded grammar payload carries at least one function"
                    );
                }
                V::QuotedGrammarCall(Arc::new(QuotedGrammarCall {
                    original,
                    functions: functions.into(),
                    pos: q.pos.clone(),
                }))
            }
            V::CBoolean(_)
            | V::CDate(_)
            | V::CDecimal(_)
            | V::CFloat(_)
            | V::CInteger(_)
            | V::CLatestDate(_)
            | V::CString(_)
            | V::CTime(_)
            | V::EnumValue(_)
            | V::PackageableElementPtr(_)
            | V::Variable(_)
            | V::TypeAnnotation(_)
            | V::SqlIsland(_)
            | V::GqlIsland(_) => self.clone(),
            V::TdsLiteral(tl) => V::TdsLiteral(Arc::new(TdsLiteral {
                tds_string: tl.tds_string.clone(),
                desugared: applied(&cs, 0),
                pos: tl.pos.clone(),
            })),
            V::AppliedFunction(af) => V::AppliedFunction(Arc::new(af.with_parameters(cs.into()))),
            // the owner class (older JSON's written detail) rides through; the position is W1.2's known defect
            V::AppliedProperty(ap) => V::AppliedProperty(Arc::new(AppliedProperty {
                receiver: child(&cs, 0).clone(),
                property: ap.property.clone(),
                pos: None,
                owner_class: ap.owner_class.clone(),
            })),
            V::LambdaFunction(lf) => V::LambdaFunction(Arc::new(LambdaFunction {
                parameters: lf.parameters.clone(),
                body: cs.into(),
                pos: None,
            })),
            V::NewInstance(ni) => {
                let properties: Vec<Box<NewInstanceKeyBinding>> = ni
                    .properties
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        Box::new(NewInstanceKeyBinding {
                            key: e.key.clone(),
                            expression: Box::new(KeyExpression {
                                value: child(&cs, i).clone(),
                                is_add: e.expression.is_add,
                                is_local: e.expression.is_local,
                            }),
                        })
                    })
                    .collect();
                V::NewInstance(Arc::new(NewInstance {
                    class_name: ni.class_name.clone(),
                    type_arguments: ni.type_arguments.clone(),
                    type_multiplicity_arguments: ni.type_multiplicity_arguments.clone(),
                    properties: properties.into(),
                }))
            }
            V::NewInstanceCast(nc) => V::NewInstanceCast(Arc::new(NewInstanceCast {
                class_name: nc.class_name.clone(),
                type_arguments: nc.type_arguments.clone(),
                src: child(&cs, 0).clone(),
                target_set_id: nc.target_set_id.clone(),
            })),
            V::PureCollection(old) => V::PureCollection(Arc::new(PureCollection {
                values: cs.into(),
                pos: old.pos.clone(),
            })),
            V::ColumnInstance(ColumnInstance::ColSpecArray(_)) => {
                let col_specs: Vec<Arc<ColSpec>> = (0..cs.len()).map(|i| col_spec(&cs, i)).collect();
                V::ColumnInstance(ColumnInstance::ColSpecArray(Arc::new(ColSpecArray {
                    col_specs: col_specs.into(),
                    pos: None,
                })))
            }
            V::ColumnInstance(ColumnInstance::ColSpec(old)) => {
                let mut i = 0;
                let function1 = old.function1.as_ref().map(|_| {
                    i += 1;
                    lambda(&cs, i - 1)
                });
                let function2 = old.function2.as_ref().map(|_| {
                    i += 1;
                    lambda(&cs, i - 1)
                });
                // each slot read stood below cs.len(), so i <= cs.len(): Java's subList(i, size)
                V::ColumnInstance(ColumnInstance::ColSpec(Arc::new(ColSpec {
                    name: old.name.clone(),
                    function1,
                    function2,
                    alias: old.alias.clone(),
                    args: cs[i..].to_vec().into(),
                    qualified: old.qualified,
                    pos: old.pos.clone(),
                    col_type: old.col_type.clone(),
                    col_type_mult: old.col_type_mult.clone(),
                    stereotypes: Arc::default(),
                    tagged_values: Arc::default(),
                })))
            }
        }
    }

    /// `mapChildren(f)`, fallible: `f` over every direct child in order (the first `Err` returned at once, as Java's
    /// throw leaves the loop), `f`'s `None` meaning "this child, unchanged". `Ok(None)` when every child is unchanged
    /// (Java's `this`), a leaf included; `Ok(Some(rebuilt))` through [`with_children`](Self::with_children) otherwise,
    /// the unchanged children their very `Arc`s.
    pub fn try_map_children<E>(
        &self,
        mut f: impl FnMut(&ValueSpecification) -> Result<Option<ValueSpecification>, E>,
    ) -> Result<Option<ValueSpecification>, E> {
        let cs = self.children();
        if cs.is_empty() {
            return Ok(None);
        }
        let mut rw = Vec::with_capacity(cs.len());
        let mut same = true;
        for c in cs {
            match f(&c)? {
                None => rw.push(c),
                Some(r) => {
                    same = false;
                    rw.push(r);
                }
            }
        }
        Ok(if same { None } else { Some(self.with_children(rw)) })
    }

    /// `mapChildren(f)` for an `f` that cannot fail: [`try_map_children`](Self::try_map_children)'s answer.
    pub fn map_children(
        &self,
        mut f: impl FnMut(&ValueSpecification) -> Option<ValueSpecification>,
    ) -> Option<ValueSpecification> {
        let Ok(answer) = self.try_map_children(|c| Ok::<_, Infallible>(f(c)));
        answer
    }
}

/// `cs.get(i)`.
fn child(cs: &[ValueSpecification], i: usize) -> &ValueSpecification {
    cs.get(i).unwrap_or_else(|| {
        panic!(
            "java.lang.IndexOutOfBoundsException: Index {i} out of bounds for length {}",
            cs.len()
        )
    })
}

/// `(LambdaFunction) cs.get(i)`.
fn lambda(cs: &[ValueSpecification], i: usize) -> Arc<LambdaFunction> {
    match child(cs, i) {
        V::LambdaFunction(l) => l.clone(),
        _ => panic!("java.lang.ClassCastException: withChildren's child {i} is not a LambdaFunction"),
    }
}

/// `(AppliedFunction) cs.get(i)`.
fn applied(cs: &[ValueSpecification], i: usize) -> Arc<AppliedFunction> {
    match child(cs, i) {
        V::AppliedFunction(a) => a.clone(),
        _ => panic!("java.lang.ClassCastException: withChildren's child {i} is not an AppliedFunction"),
    }
}

/// `(ColSpec) cs.get(i)`.
fn col_spec(cs: &[ValueSpecification], i: usize) -> Arc<ColSpec> {
    match child(cs, i) {
        V::ColumnInstance(ColumnInstance::ColSpec(c)) => c.clone(),
        _ => panic!("java.lang.ClassCastException: withChildren's child {i} is not a ColSpec"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec;
    use crate::doors::Dialect;
    use crate::jtypes::JStr;
    use crate::records::{PStereotype, Variable};

    fn parse(text: &str) -> ValueSpecification {
        let units: Vec<u16> = text.encode_utf16().collect();
        crate::spec_parser::parse(&units, Dialect::LegendLite).expect("the expression parses")
    }

    fn json(v: &ValueSpecification) -> String {
        codec::write(v, false)
    }

    fn var(name: &str) -> ValueSpecification {
        V::Variable(Arc::new(Variable {
            name: JStr::from(name),
            r#type: None,
            multiplicity: None,
            pos: None,
        }))
    }

    /// `$x` renamed to `$y` everywhere, its position kept (no shadow stop: a test walker), every other node through
    /// `map_children`.
    fn rename(v: &ValueSpecification) -> Option<ValueSpecification> {
        if let V::Variable(x) = v
            && x.name == "x"
        {
            return Some(V::Variable(Arc::new(Variable {
                name: JStr::from("y"),
                ..(**x).clone()
            })));
        }
        v.map_children(rename)
    }

    fn walked(v: &ValueSpecification) -> ValueSpecification {
        rename(v).unwrap_or_else(|| v.clone())
    }

    #[test]
    fn a_call_rebuilds_through_with_parameters_and_keeps_every_component() {
        let v = parse("$x + 1");
        let V::AppliedFunction(before) = &v else {
            panic!("a call")
        };
        assert!(before.pos.is_some());
        let after = walked(&v);
        let V::AppliedFunction(a) = &after else {
            panic!("a call")
        };
        assert_eq!(a.pos, before.pos, "the position rides through withParameters");
        assert_eq!(a.function, before.function);
        assert_eq!(a.infix, before.infix);
        assert_eq!(json(&after), json(&v).replace("\"name\":\"x\"", "\"name\":\"y\""));
    }

    #[test]
    fn an_unchanged_node_answers_none_and_keeps_its_position() {
        let v = parse("$z.a + 1");
        assert_eq!(rename(&v), None);
        assert_eq!(json(&walked(&v)), json(&v));
        let leaf = parse("1");
        assert!(leaf.children().is_empty());
        assert_eq!(
            leaf.map_children(|_| Some(var("never"))),
            None,
            "a leaf has no child to change"
        );
    }

    #[test]
    fn a_property_rebuild_drops_its_position_and_keeps_its_owner() {
        let v = parse("$x.name");
        let V::AppliedProperty(before) = &v else {
            panic!("a property")
        };
        assert!(before.pos.is_some());
        let after = walked(&v);
        let V::AppliedProperty(a) = &after else {
            panic!("a property")
        };
        assert_eq!(a.pos, None, "Java rebuilds with a null position");
        assert_eq!(a.property, before.property);
        assert_eq!(a.owner_class, before.owner_class);
        // an untouched property keeps its position
        let kept = walked(&parse("$z.name"));
        let V::AppliedProperty(k) = &kept else {
            panic!("a property")
        };
        assert!(k.pos.is_some());
    }

    #[test]
    fn a_lambda_rebuild_drops_its_position_and_keeps_its_parameters() {
        let v = parse("{p | $x + $p}");
        let V::LambdaFunction(before) = &v else {
            panic!("a lambda")
        };
        assert!(before.pos.is_some());
        assert_eq!(v.children().len(), 1, "the body alone, never the parameters");
        let after = walked(&v);
        let V::LambdaFunction(l) = &after else {
            panic!("a lambda")
        };
        assert_eq!(l.pos, None);
        assert_eq!(l.parameters, before.parameters);
        assert!(json(&after).contains("\"name\":\"y\""));
    }

    #[test]
    fn a_collection_keeps_its_position() {
        let v = parse("[$x, 2]");
        let V::PureCollection(before) = &v else {
            panic!("a collection")
        };
        let after = walked(&v);
        let V::PureCollection(c) = &after else {
            panic!("a collection")
        };
        assert_eq!(c.pos, before.pos);
        assert_eq!(c.values.len(), 2);
        // the unchanged child is the very object
        assert_eq!(json(&c.values[1]), json(&before.values[1]));
    }

    #[test]
    fn a_new_instance_rebuilds_its_bindings_in_order() {
        // the parser writes `^P(...)` as new(P, the instance)
        let V::AppliedFunction(call) = parse("^a::P(name = $x, age = 3)") else {
            panic!("a call")
        };
        let v = call.parameters[1].clone();
        let V::NewInstance(before) = &v else {
            panic!("a new instance: {}", json(&v))
        };
        assert_eq!(v.children().len(), 2);
        let after = walked(&v);
        let V::NewInstance(n) = &after else {
            panic!("a new instance")
        };
        assert_eq!(n.properties.len(), 2);
        assert_eq!(n.properties[0].key, before.properties[0].key);
        assert_eq!(n.properties[1].key, before.properties[1].key);
        assert_eq!(
            n.properties[0].expression.is_add,
            before.properties[0].expression.is_add
        );
        assert_eq!(json(&after), json(&v).replace("\"name\":\"x\"", "\"name\":\"y\""));
    }

    #[test]
    fn a_column_spec_drops_its_annotations_and_keeps_its_position() {
        let v = parse("~a:x | $x.b");
        let V::ColumnInstance(ColumnInstance::ColSpec(before)) = &v else {
            panic!("a col spec: {}", json(&v))
        };
        let annotated = V::ColumnInstance(ColumnInstance::ColSpec(Arc::new(ColSpec {
            stereotypes: Arc::from(vec![Box::new(PStereotype {
                profile: JStr::from("a::Pr"),
                value: JStr::from("s"),
                profile_source_information: None,
                source_information: None,
            })]),
            ..(**before).clone()
        })));
        assert_eq!(annotated.children().len(), 1, "function1, then the args");
        // unchanged: the annotations ride (the very object)
        assert_eq!(annotated.map_children(|_| None), None);
        let V::ColumnInstance(ColumnInstance::ColSpec(c)) = walked(&annotated) else {
            panic!("a col spec")
        };
        assert!(
            c.stereotypes.is_empty(),
            "the nine-argument constructor drops the annotations"
        );
        assert!(c.tagged_values.is_empty());
        assert_eq!(c.pos, before.pos);
        assert_eq!(c.name, before.name);
        assert!(c.function1.is_some() && c.function2.is_none());
        assert_eq!(
            c.function1.as_ref().unwrap().pos,
            None,
            "the rebuilt lambda lost its position"
        );
    }

    #[test]
    fn a_column_spec_array_drops_its_position() {
        let v = parse("~[a:x | $x.b, c]");
        let V::ColumnInstance(ColumnInstance::ColSpecArray(before)) = &v else {
            panic!("an array: {}", json(&v))
        };
        assert!(before.pos.is_some());
        assert_eq!(v.children().len(), 2);
        let V::ColumnInstance(ColumnInstance::ColSpecArray(a)) = walked(&v) else {
            panic!("an array")
        };
        assert_eq!(a.pos, None);
        assert_eq!(a.col_specs.len(), 2);
        assert_eq!(
            a.col_specs[1], before.col_specs[1],
            "the unchanged spec is the very one"
        );
    }

    #[test]
    fn a_cast_keeps_its_class_and_set() {
        let src = parse("$x");
        let v = V::NewInstanceCast(Arc::new(NewInstanceCast {
            class_name: JStr::from("a::P"),
            type_arguments: Arc::default(),
            src,
            target_set_id: Some(JStr::from("s1")),
        }));
        let after = walked(&v);
        assert_eq!(json(&after), json(&v).replace("\"name\":\"x\"", "\"name\":\"y\""));
    }

    /// Every string literal's text upper-cased, its other components kept.
    fn shout(v: &ValueSpecification) -> Option<ValueSpecification> {
        if let V::CString(s) = v {
            let text = String::from_utf16_lossy(&s.value.0).to_uppercase();
            return Some(V::CString(Arc::new(crate::records::CString {
                value: JStr::from(text.as_str()),
                ..(**s).clone()
            })));
        }
        v.map_children(shout)
    }

    #[test]
    fn a_tds_literal_keeps_its_text_and_position() {
        let v = parse("#TDS{a\n1}#");
        let V::TdsLiteral(before) = &v else {
            panic!("a TDS literal: {}", json(&v))
        };
        assert_eq!(v.children().len(), 1, "its desugared call");
        let after = shout(&v).expect("changed");
        let V::TdsLiteral(t) = &after else {
            panic!("a TDS literal")
        };
        assert_eq!(t.tds_string, before.tds_string);
        assert_eq!(t.pos, before.pos);
        assert!(json(&after).contains("\"value\":\"A\\n1\""), "{}", json(&after));
    }

    #[test]
    fn a_path_literal_keeps_its_wire_pieces() {
        let v = parse("#/a::P/name#");
        let V::PathLiteral(before) = &v else {
            panic!("a path literal: {}", json(&v))
        };
        assert_eq!(v.children().len(), 1, "its desugared lambda");
        // a rebuilt desugared lambda: the literal rebuilt around it, every wire piece kept
        let fresh = V::LambdaFunction(Arc::new((*before.desugared).clone()));
        let after = v.map_children(|_| Some(fresh.clone())).expect("changed");
        assert_eq!(json(&after), json(&v));
    }

    #[test]
    fn a_wrong_kind_in_a_typed_slot_is_java_s_class_cast() {
        let v = parse("{p | $p}");
        let tds = V::TdsLiteral(Arc::new(TdsLiteral {
            tds_string: JStr::from("a\n1"),
            desugared: match parse("f(1)") {
                V::AppliedFunction(a) => a,
                _ => panic!("a call"),
            },
            pos: None,
        }));
        let caught = std::panic::catch_unwind(|| tds.with_children(vec![v.clone()]));
        assert!(caught.is_err(), "a lambda where the call is cast");
    }

    #[test]
    fn the_first_refusal_is_answered_and_the_rest_unvisited() {
        let v = parse("f($x, $z, $x)");
        let mut seen = Vec::new();
        let answer: Result<Option<ValueSpecification>, String> = v.try_map_children(|c| {
            seen.push(json(c));
            match c {
                V::Variable(x) if x.name == "z" => Err("no z".to_owned()),
                _ => Ok(None),
            }
        });
        assert_eq!(answer, Err("no z".to_owned()));
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn every_child_is_visited_after_the_first_change() {
        let v = parse("f($x, $z, $x)");
        let mut visits = 0;
        let after = v.map_children(|c| {
            visits += 1;
            rename(c)
        });
        assert_eq!(visits, 3);
        let after = after.expect("changed");
        assert_eq!(json(&after).matches("\"name\":\"y\"").count(), 2);
        assert_eq!(json(&after).matches("\"name\":\"z\"").count(), 1);
    }
}
