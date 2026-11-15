// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ValueSpecification.equals` as Java's records define it (2026-11-14, the normalizer slice's fill, e1): the derived
//! `PartialEq` of the generated records compares every component, while Java's `equals` on the spec records leaves
//! out the source position everywhere and, per record, the written details of older JSON and the parser's spelling
//! marks (`AppliedFunction`'s `propertyCall`/`grouped`/`infix`/`island`/`fControl`/`ownerClass`, `AppliedProperty`'s
//! `ownerClass`, `CString.multiLine`, `CDate.written`, `EnumValue`'s `enumerationPos`/`node`, `Generic`'s
//! `typeVariableValues`, `RelationShape`'s `spelledName`/`typeSpan`, `ColSpec`'s stereotypes and tagged values,
//! `PathLiteral`'s `literalLength`/`lineStarts`; `CTime.written` counts only when there is no value). The
//! normalizer's XStore direction check (`MappingNormalizer.canonicalizeEqualOperands` and the `.equals` after it)
//! compares two lines' conditions, parsed at two positions, by Java's `equals`.
//!
//! [`ValueSpecification::equality_key`] is a text of exactly the components Java's `equals` reads, record by record
//! (every arm spelled, no default): two specifications have one key exactly when Java's `equals` holds, and the keys
//! ordered as text are a total order whose ties are Java's equality. A record with Java's default (all-component)
//! equality and no custom-equality record below it is keyed by its lossless codec text (`Multiplicity`, the date and
//! time literals, `PathLiteral.Segment`, `TypeAnnotation.MultiplicityRef` -- its position included, as its default
//! `equals` has it -- and the GraphQL document): the codec writes a `double` as `Double.toString`, so its text
//! equality is a record's `Double.compare`, and a `BigDecimal` as `toString`, which is one-to-one with
//! `BigDecimal.equals`.

use crate::codec::{Codec, Writer};
use crate::jtypes::JStr;
use crate::records::{
    AppliedFunction, ColSpec, ColumnInstance, GraphFetchLiteralNode, GraphFetchLiteralSubTypeNode, LambdaFunction,
    TypeAnnotation, TypeExpression, TypeExpressionTypedParameter, ValueSpecification, Variable,
};

use ValueSpecification as V;

impl ValueSpecification {
    /// `equals(other)`: Java's record equality (module note).
    pub fn java_equals(&self, other: &ValueSpecification) -> bool {
        self.equality_key() == other.equality_key()
    }

    /// The components Java's `equals` reads, as one text (module note): equal keys exactly when [`java_equals`]
    /// holds; their text order a total order consistent with it.
    ///
    /// [`java_equals`]: ValueSpecification::java_equals
    pub fn equality_key(&self) -> String {
        let mut w = Writer::new();
        vs(&mut w, self);
        w.finish()
    }
}

impl TypeExpression {
    /// `TypeExpression.equals`'s components as one text, as [`ValueSpecification::equality_key`] keys a
    /// specification: equal keys exactly when Java's `equals` holds (the normalizer's second batch, item 4 (b),
    /// 2026-11-15: a class mapping's local property is typed by one, compared at US:797).
    pub fn equality_key(&self) -> String {
        let mut w = Writer::new();
        type_expression(&mut w, self);
        w.finish()
    }
}

/// A node: `["Tag"` then each component after a comma, then `]` -- a JSON array, so the text parses back to its
/// components (the key is one-to-one).
fn node(w: &mut Writer, tag: &str, components: impl FnOnce(&mut Writer)) {
    w.raw("[\"");
    w.raw(tag);
    w.raw("\"");
    components(w);
    w.raw("]");
}

fn string(w: &mut Writer, s: &JStr) {
    w.raw(",");
    w.string(s);
}

fn opt_string(w: &mut Writer, s: Option<&JStr>) {
    w.raw(",");
    match s {
        None => w.raw("null"),
        Some(s) => w.string(s),
    }
}

fn boolean(w: &mut Writer, b: bool) {
    w.raw(",");
    w.boolean(b);
}

/// A component keyed by its codec text (a record of default equality; module note).
fn leaf<T: Codec + ?Sized>(w: &mut Writer, x: &T) {
    w.raw(",");
    x.write(w, false);
}

/// A component that is itself keyed.
fn sub(w: &mut Writer, f: impl FnOnce(&mut Writer)) {
    w.raw(",");
    f(w);
}

fn list<T>(w: &mut Writer, xs: impl IntoIterator<Item = T>, mut f: impl FnMut(&mut Writer, T)) {
    w.raw(",[");
    let mut first = true;
    for x in xs {
        if !first {
            w.raw(",");
        }
        first = false;
        f(w, x);
    }
    w.raw("]");
}

fn strings<'a>(w: &mut Writer, xs: impl IntoIterator<Item = &'a JStr>) {
    list(w, xs, |w, s| w.string(s));
}

fn vs(w: &mut Writer, v: &ValueSpecification) {
    match v {
        V::AppliedFunction(af) => applied(w, af),
        V::AppliedProperty(ap) => node(w, "AppliedProperty", |w| {
            sub(w, |w| vs(w, &ap.receiver));
            string(w, &ap.property);
        }),
        V::CBoolean(x) => node(w, "CBoolean", |w| boolean(w, x.value)),
        V::CByteArray(x) => node(w, "CByteArray", |w| string(w, &x.value)),
        V::CDate(x) => node(w, "CDate", |w| leaf(w, &x.value)),
        V::CDecimal(x) => node(w, "CDecimal", |w| {
            w.raw(",");
            w.decimal(&x.value);
        }),
        V::CFloat(x) => node(w, "CFloat", |w| {
            w.raw(",");
            w.double(x.value);
            w.raw(",");
            match &x.exact {
                None => w.raw("null"),
                Some(d) => w.decimal(d),
            }
        }),
        V::CInteger(x) => node(w, "CInteger", |w| {
            w.raw(",");
            w.value(&x.value);
        }),
        V::CLatestDate(_) => node(w, "CLatestDate", |_| {}),
        V::GraphFetchLiteral(g) => node(w, "GraphFetchLiteral", |w| {
            string(w, &g.class_name);
            list(w, g.sub_trees.iter(), |w, n| graph_node(w, n));
            list(w, g.sub_type_trees.iter(), |w, n| graph_sub_type_node(w, n));
        }),
        V::QuotedTreeCall(q) => node(w, "QuotedTreeCall", |w| {
            sub(w, |w| applied(w, &q.original));
            sub(w, |w| vs(w, &q.tree));
        }),
        V::QuotedGrammarCall(q) => node(w, "QuotedGrammarCall", |w| {
            sub(w, |w| applied(w, &q.original));
            list(w, q.functions.iter(), |w, f| lambda(w, f));
        }),
        V::PathLiteral(pl) => node(w, "PathLiteral", |w| {
            string(w, &pl.start_type);
            list(w, pl.segments.iter(), |w, s| s.write(w, false));
            sub(w, |w| lambda(w, &pl.desugared));
            opt_string(w, pl.alias.as_ref());
            boolean(w, pl.has_dated_segment);
        }),
        V::SqlIsland(x) => node(w, "SqlIsland", |w| {
            string(w, &x.sql);
            string(w, &x.language);
            opt_string(w, x.profile.as_ref());
        }),
        V::GqlIsland(x) => node(w, "GqlIsland", |w| leaf(w, &*x.document)),
        V::TdsLiteral(x) => node(w, "TdsLiteral", |w| {
            string(w, &x.tds_string);
            sub(w, |w| applied(w, &x.desugared));
        }),
        V::CString(x) => node(w, "CString", |w| string(w, &x.value)),
        // `Objects.equals(value, other.value) && (value != null || Objects.equals(written, other.written))`
        V::CTime(x) => node(w, "CTime", |w| match &x.value {
            Some(t) => leaf(w, t),
            None => {
                w.raw(",null");
                opt_string(w, x.written.as_ref());
            }
        }),
        V::ColumnInstance(ColumnInstance::ColSpec(c)) => col_spec(w, c),
        V::ColumnInstance(ColumnInstance::ColSpecArray(a)) => node(w, "ColSpecArray", |w| {
            list(w, a.col_specs.iter(), |w, c| col_spec(w, c));
        }),
        V::EnumValue(e) => node(w, "EnumValue", |w| {
            string(w, &e.full_path);
            string(w, &e.value);
        }),
        V::LambdaFunction(l) => lambda(w, l),
        // the default record equality: every component
        V::NewInstance(ni) => node(w, "NewInstance", |w| {
            string(w, &ni.class_name);
            list(w, ni.type_arguments.iter(), type_expression);
            strings(w, ni.type_multiplicity_arguments.iter());
            list(w, ni.properties.iter(), |w, kb| {
                node(w, "KeyBinding", |w| {
                    string(w, &kb.key);
                    sub(w, |w| {
                        node(w, "KeyExpression", |w| {
                            sub(w, |w| vs(w, &kb.expression.value));
                            boolean(w, kb.expression.is_add);
                            boolean(w, kb.expression.is_local);
                        })
                    });
                })
            });
        }),
        V::NewInstanceCast(nc) => node(w, "NewInstanceCast", |w| {
            string(w, &nc.class_name);
            list(w, nc.type_arguments.iter(), type_expression);
            sub(w, |w| vs(w, &nc.src));
            opt_string(w, nc.target_set_id.as_ref());
        }),
        V::PackageableElementPtr(p) => node(w, "PackageableElementPtr", |w| string(w, &p.full_path)),
        V::PureCollection(pc) => node(w, "PureCollection", |w| list(w, pc.values.iter(), vs)),
        V::TypeAnnotation(ta) => type_annotation(w, ta),
        V::Variable(x) => variable(w, x),
    }
}

/// `AppliedFunction.equals`: function, parameters, referents.
fn applied(w: &mut Writer, af: &AppliedFunction) {
    node(w, "AppliedFunction", |w| {
        string(w, &af.function);
        list(w, af.parameters.iter(), vs);
        strings(w, af.referents.iter());
    });
}

/// `LambdaFunction.equals`: parameters, body.
fn lambda(w: &mut Writer, l: &LambdaFunction) {
    node(w, "LambdaFunction", |w| {
        list(w, l.parameters.iter(), |w, p| variable(w, p));
        list(w, l.body.iter(), vs);
    });
}

/// `Variable.equals`: name, type, multiplicity.
fn variable(w: &mut Writer, v: &Variable) {
    node(w, "Variable", |w| {
        string(w, &v.name);
        sub(w, |w| match &v.r#type {
            None => w.raw("null"),
            Some(t) => type_expression(w, t),
        });
        w.raw(",");
        match &v.multiplicity {
            None => w.raw("null"),
            Some(m) => m.write(w, false),
        }
    });
}

/// `ColSpec.equals`: every component but the position, the stereotypes and the tagged values.
fn col_spec(w: &mut Writer, c: &ColSpec) {
    node(w, "ColSpec", |w| {
        string(w, &c.name);
        sub(w, |w| match &c.function1 {
            None => w.raw("null"),
            Some(f) => lambda(w, f),
        });
        sub(w, |w| match &c.function2 {
            None => w.raw("null"),
            Some(f) => lambda(w, f),
        });
        opt_string(w, c.alias.as_ref());
        list(w, c.args.iter(), vs);
        boolean(w, c.qualified);
        sub(w, |w| match &c.col_type {
            None => w.raw("null"),
            Some(t) => type_expression(w, t),
        });
        w.raw(",");
        match &c.col_type_mult {
            None => w.raw("null"),
            Some(m) => m.write(w, false),
        }
    });
}

/// `GraphFetchLiteral.Node.equals`: property, parameters, alias, subType, subTrees.
fn graph_node(w: &mut Writer, n: &GraphFetchLiteralNode) {
    node(w, "Node", |w| {
        string(w, &n.property);
        list(w, n.parameters.iter(), vs);
        opt_string(w, n.alias.as_ref());
        opt_string(w, n.sub_type.as_ref());
        list(w, n.sub_trees.iter(), |w, s| graph_node(w, s));
    });
}

/// `GraphFetchLiteral.SubTypeNode.equals`: subTypeClass, subTrees.
fn graph_sub_type_node(w: &mut Writer, n: &GraphFetchLiteralSubTypeNode) {
    node(w, "SubTypeNode", |w| {
        string(w, &n.sub_type_class);
        list(w, n.sub_trees.iter(), |w, s| graph_node(w, s));
    });
}

/// `TypeExpression`'s equality: `NameRef` by name, `Generic` by name, arguments and multiplicity arguments, the rest
/// by every component (a column without its position).
fn type_expression(w: &mut Writer, t: &TypeExpression) {
    match t {
        TypeExpression::TypeExpressionNameRef(n) => node(w, "NameRef", |w| string(w, &n.name)),
        TypeExpression::TypeExpressionGeneric(g) => node(w, "Generic", |w| {
            string(w, &g.name);
            list(w, g.arguments.iter(), type_expression);
            strings(w, g.multiplicity_arguments.iter());
        }),
        TypeExpression::TypeExpressionFunctionType(f) => node(w, "FunctionType", |w| {
            list(w, f.parameters.iter(), |w, p| typed_parameter(w, p));
            sub(w, |w| typed_parameter(w, &f.result));
        }),
        TypeExpression::TypeExpressionRelationType(r) => node(w, "RelationType", |w| {
            list(w, r.columns.iter(), |w, c| {
                node(w, "Column", |w| {
                    string(w, &c.name);
                    sub(w, |w| type_expression(w, &c.r#type));
                    leaf(w, &c.multiplicity);
                })
            });
        }),
        TypeExpression::TypeExpressionSchemaAlgebra(s) => node(w, "SchemaAlgebra", |w| {
            sub(w, |w| type_expression(w, &s.left));
            leaf(w, &s.op);
            sub(w, |w| type_expression(w, &s.right));
        }),
    }
}

fn typed_parameter(w: &mut Writer, p: &TypeExpressionTypedParameter) {
    node(w, "TypedParameter", |w| {
        sub(w, |w| type_expression(w, &p.r#type));
        leaf(w, &p.multiplicity);
    });
}

/// `TypeAnnotation`'s equality: `Named` by its type, `RelationShape` by its columns (a column without its position),
/// `MultiplicityRef` by its default record equality (its position included), `Wildcard` all one.
fn type_annotation(w: &mut Writer, t: &TypeAnnotation) {
    match t {
        TypeAnnotation::TypeAnnotationNamed(n) => node(w, "Named", |w| sub(w, |w| type_expression(w, &n.r#type))),
        TypeAnnotation::TypeAnnotationRelationShape(r) => node(w, "RelationShape", |w| {
            list(w, r.columns.iter(), |w, c| {
                node(w, "Column", |w| {
                    opt_string(w, c.name.as_ref());
                    sub(w, |w| type_annotation(w, &c.r#type));
                    leaf(w, &c.multiplicity);
                })
            });
        }),
        TypeAnnotation::TypeAnnotationMultiplicityRef(m) => node(w, "MultiplicityRef", |w| leaf(w, &**m)),
        TypeAnnotation::TypeAnnotationWildcard(_) => node(w, "Wildcard", |_| {}),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::records::{AppliedProperty, CString, SourceInfo};

    use super::*;

    fn at(line: i32) -> Option<Box<SourceInfo>> {
        Some(Box::new(SourceInfo {
            source_id: JStr::from(""),
            start_line: line,
            start_column: 1,
            end_line: line,
            end_column: 9,
        }))
    }

    fn var(name: &str) -> ValueSpecification {
        V::Variable(Arc::new(Variable {
            name: JStr::from(name),
            r#type: None,
            multiplicity: None,
            pos: None,
        }))
    }

    fn prop(receiver: ValueSpecification, p: &str, line: i32, owner: Option<&str>) -> ValueSpecification {
        V::AppliedProperty(Arc::new(AppliedProperty {
            receiver,
            property: JStr::from(p),
            pos: at(line),
            owner_class: owner.map(JStr::from),
        }))
    }

    fn string_at(s: &str, line: i32, multi_line: bool) -> ValueSpecification {
        V::CString(Arc::new(CString {
            value: JStr::from(s),
            pos: at(line),
            multi_line,
        }))
    }

    #[test]
    fn positions_and_written_details_are_not_compared() {
        let a = prop(var("this"), "id", 1, None);
        let b = prop(var("this"), "id", 7, Some("a::Firm"));
        assert_ne!(a, b, "the derived equality compares them");
        assert!(a.java_equals(&b));
        assert!(string_at("x", 1, false).java_equals(&string_at("x", 2, true)));
    }

    #[test]
    fn a_different_component_is_a_different_key() {
        assert!(!prop(var("this"), "id", 1, None).java_equals(&prop(var("that"), "id", 1, None)));
        assert!(!prop(var("this"), "id", 1, None).java_equals(&prop(var("this"), "ID", 1, None)));
        assert!(!string_at("x", 1, false).java_equals(&var("x")));
        // a string whose text spells another key's punctuation stays one component
        assert!(!string_at("a\",\"b", 1, false).java_equals(&string_at("a", 1, false)));
    }
}
