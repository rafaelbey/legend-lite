// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! lite-element's and lite-types' values as the generated mirrors ([`super::oracle_records`]) hold them, so that the
//! mirrors' writers write ElementAnswers' text: `classView`, `functionView` and the records they hold, by hand (small,
//! and type-checked against both sides). A Java enum's constant is found by its name (`value_of`), a name the two sides
//! share.

use std::sync::Arc;

use lite_element::{Property, TypedClass, TypedConstraint, TypedEnum, TypedFunction, TypedParameter};
use lite_front::codec;
use lite_front::sha256::sha256_utf16le;
use lite_model::JStr;
use lite_types::{Column, Multiplicity, Param, Type};

use super::oracle_records as o;

fn text(s: &str) -> JStr {
    JStr::from(s)
}

fn texts(xs: &[JStr]) -> Vec<JStr> {
    xs.to_vec()
}

/// A `Type` as `Type`'s mirror.
pub fn ty(t: Type<'_>) -> o::Type {
    match t {
        Type::Primitive(p) => o::Type::TypePrimitive(
            o::TypePrimitive::value_of(p.name()).unwrap_or_else(|| panic!("no Java primitive {}", p.name())),
        ),
        Type::PrecisionDecimal(d) => o::Type::TypePrecisionDecimal(Box::new(o::TypePrecisionDecimal {
            precision: d.precision(),
            scale: d.scale(),
        })),
        Type::ClassType(fqn) => o::Type::TypeClassType(Box::new(o::TypeClassType { fqn: text(fqn) })),
        Type::EnumType(fqn) => o::Type::TypeEnumType(Box::new(o::TypeEnumType { fqn: text(fqn) })),
        Type::TypeVar(name) => o::Type::TypeTypeVar(Box::new(o::TypeTypeVar { name: text(name) })),
        Type::GenericType(g) => o::Type::TypeGenericType(Box::new(o::TypeGenericType {
            raw_fqn: text(g.raw_fqn),
            arguments: g.arguments.iter().map(|a| ty(*a)).collect(),
            mult_arguments: g.mult_arguments.iter().map(|m| multiplicity(*m)).collect(),
        })),
        Type::FunctionType(f) => o::Type::TypeFunctionType(Box::new(o::TypeFunctionType {
            params: f.params.iter().map(|p| Box::new(param(p))).collect(),
            result: Box::new(param(&f.result)),
        })),
        Type::RelationType(r) => o::Type::TypeRelationType(Box::new(relation_type(r))),
        Type::SchemaAlgebra(s) => o::Type::TypeSchemaAlgebra(Box::new(o::TypeSchemaAlgebra {
            left: ty(s.left),
            op: o::TypeOp::value_of(s.op.name()).unwrap_or_else(|| panic!("no Java op {}", s.op.name())),
            right: ty(s.right),
        })),
    }
}

fn param(p: &Param<'_>) -> o::TypeParam {
    o::TypeParam {
        r#type: ty(p.r#type),
        multiplicity: multiplicity(p.multiplicity),
    }
}

fn column(c: &Column<'_>) -> o::TypeColumn {
    o::TypeColumn {
        name: text(c.name),
        r#type: ty(c.r#type),
        multiplicity: multiplicity(c.multiplicity),
    }
}

/// A `Type.RelationType` as its mirror (findTable's answer, written untagged).
pub fn relation_type(r: &lite_types::RelationType<'_>) -> o::TypeRelationType {
    o::TypeRelationType {
        columns: r.columns().iter().map(|c| Box::new(column(c))).collect(),
        dynamic_columns: r.dynamic_columns().iter().map(|c| Box::new(column(c))).collect(),
    }
}

/// A `Multiplicity` as its mirror.
pub fn multiplicity(m: Multiplicity<'_>) -> o::Multiplicity {
    match m {
        Multiplicity::Bounded(b) => o::Multiplicity::MultiplicityBounded(Box::new(o::MultiplicityBounded {
            lower: b.lower(),
            upper: b.upper(),
        })),
        Multiplicity::Var(v) => o::Multiplicity::MultiplicityVar(Box::new(o::MultiplicityVar { name: text(v.name()) })),
    }
}

fn parameter(p: &TypedParameter<'_>) -> o::TypedParameter {
    o::TypedParameter {
        name: p.name.clone(),
        r#type: ty(p.r#type),
        multiplicity: multiplicity(p.multiplicity),
    }
}

/// A `Property` as its mirror (findProperty's answer, a class's properties).
pub fn property(p: &Property<'_>) -> o::Property {
    match p {
        Property::Stored(s) => o::Property::PropertyStored(Box::new(o::PropertyStored {
            name: s.name.clone(),
            r#type: ty(s.r#type),
            multiplicity: multiplicity(s.multiplicity),
            equality_key: s.equality_key,
            has_default: s.has_default,
            default_value: s.default_value.clone(),
        })),
        Property::Derived(d) => o::Property::PropertyDerived(Box::new(o::PropertyDerived {
            name: d.name.clone(),
            r#type: ty(d.r#type),
            multiplicity: multiplicity(d.multiplicity),
            parameters: d.parameters.iter().map(|p| Box::new(parameter(p))).collect(),
            body_function_fqn: d.body_function_fqn.clone(),
        })),
    }
}

fn constraint(c: &TypedConstraint) -> o::ConstraintView {
    o::ConstraintView {
        name: c.name.clone(),
        level: o::TypedConstraintEnforcementLevel::value_of(c.level.name())
            .unwrap_or_else(|| panic!("no Java level {}", c.level.name())),
        predicate_fqn: c.predicate_fqn.clone(),
        message_fqn: c.message_fqn.clone(),
    }
}

/// `ElementAnswers.classView(tc)` (ElementAnswers.java:897): the typed class, whole but for its constraints' external id.
pub fn class_view(tc: &TypedClass<'_>) -> o::ClassView {
    o::ClassView {
        qualified_name: tc.qualified_name.clone(),
        type_parameters: texts(&tc.type_parameters),
        super_class_fqns: texts(&tc.super_class_fqns),
        super_types: tc.super_types.iter().map(|t| ty(*t)).collect(),
        properties: tc.properties.iter().map(property).collect(),
        constraints: tc.constraints.iter().map(|c| Box::new(constraint(c))).collect(),
        is_native: tc.is_native,
        multiplicity_parameters: texts(&tc.multiplicity_parameters),
    }
}

/// A `TypedEnum` as its mirror.
pub fn typed_enum(te: &TypedEnum) -> o::TypedEnum {
    o::TypedEnum {
        qualified_name: te.qualified_name.clone(),
        values: texts(&te.values),
    }
}

/// `ElementAnswers.functionView(tf)` (ElementAnswers.java:920): the signature, the body's presence, the native flag, the id, and the
/// definition's record-codec text's SHA-256 (written as a `Function`: tagged).
pub fn function_view(tf: &TypedFunction<'_>) -> o::FunctionView {
    let definition: Vec<u16> = codec::write(&tf.definition, false).encode_utf16().collect();
    o::FunctionView {
        qualified_name: tf.qualified_name.clone(),
        type_parameters: texts(&tf.type_parameters),
        multiplicity_parameters: texts(&tf.multiplicity_parameters),
        parameters: tf.parameters.iter().map(|p| Box::new(parameter(p))).collect(),
        return_type: ty(tf.return_type),
        return_multiplicity: multiplicity(tf.return_multiplicity),
        body_present: tf.body.is_some(),
        is_native: tf.is_native,
        id: Arc::new(tf.id.clone()),
        definition: text(&sha256_utf16le(&definition)),
    }
}
