// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The protocol records' convenience constructors (core/'s `protocol.spec` records), as the parser calls them: each
//! fills the components Java's shorter constructor fills (`AppliedFunction(function, parameters)` has no referents,
//! no span, every flag false; `EnumValue`'s four-argument form sets `node` when it has no enumeration span; ...).
//! A `ValueSpecification` node is an `Arc`, its lists `Arc<[T]>` (records.rs); the helpers take a built `Vec` and
//! freeze it.

use std::sync::Arc;

use crate::jtypes::JStr;
use crate::records::{
    AppliedFunction, AppliedProperty, CString, LambdaFunction, Multiplicity, MultiplicityConcrete,
    PackageableElementPtr, PureCollection, SourceInfo, TypeExpression, TypeExpressionNameRef, ValueSpecification,
    Variable,
};

pub type Span = Option<Box<SourceInfo>>;

/// `new AppliedFunction(function, parameters, List.of(), pos, propertyCall, grouped, infix, island)`.
pub fn applied_full(
    function: JStr,
    parameters: Vec<ValueSpecification>,
    pos: Span,
    property_call: bool,
    grouped: bool,
    infix: bool,
    island: bool,
) -> AppliedFunction {
    AppliedFunction {
        function,
        parameters: parameters.into(),
        referents: Arc::default(),
        pos,
        property_call,
        grouped,
        infix,
        island,
        f_control: None,
        owner_class: None,
    }
}

/// `new AppliedFunction(function, parameters, List.of(), pos)`: every flag false.
pub fn applied(function: JStr, parameters: Vec<ValueSpecification>, pos: Span) -> AppliedFunction {
    applied_full(function, parameters, pos, false, false, false, false)
}

/// `new AppliedFunction(function, parameters, List.of(), pos, false, false, true)`: an operator's call.
pub fn infix(function: JStr, parameters: Vec<ValueSpecification>, pos: Span) -> AppliedFunction {
    applied_full(function, parameters, pos, false, false, true, false)
}

pub fn vs_fn(f: AppliedFunction) -> ValueSpecification {
    ValueSpecification::AppliedFunction(Arc::new(f))
}

/// `new CString(value, pos)`: not multi-line.
pub fn cstring(value: JStr, pos: Span) -> ValueSpecification {
    ValueSpecification::CString(Arc::new(CString {
        value,
        pos,
        multi_line: false,
    }))
}

/// `new PureCollection(values, pos)`.
pub fn collection(values: Vec<ValueSpecification>, pos: Span) -> ValueSpecification {
    ValueSpecification::PureCollection(Arc::new(PureCollection {
        values: values.into(),
        pos,
    }))
}

/// `new PackageableElementPtr(fullPath, pos)`.
pub fn ptr(full_path: JStr, pos: Span) -> ValueSpecification {
    ValueSpecification::PackageableElementPtr(Arc::new(PackageableElementPtr { full_path, pos }))
}

/// `new AppliedProperty(receiver, property, pos)`: no owner class.
pub fn property(receiver: ValueSpecification, name: JStr, pos: Span) -> ValueSpecification {
    ValueSpecification::AppliedProperty(Arc::new(AppliedProperty {
        receiver,
        property: name,
        pos,
        owner_class: None,
    }))
}

/// `new Variable(name)`: no type, multiplicity or span.
pub fn variable(name: JStr) -> Variable {
    Variable {
        name,
        r#type: None,
        multiplicity: None,
        pos: None,
    }
}

pub fn vs_var(v: Variable) -> ValueSpecification {
    ValueSpecification::Variable(Arc::new(v))
}

/// `new LambdaFunction(parameters, body, pos)`.
pub fn lambda(parameters: Vec<Variable>, body: Vec<ValueSpecification>, pos: Span) -> LambdaFunction {
    LambdaFunction {
        parameters: parameters.into_iter().map(Arc::new).collect(),
        body: body.into(),
        pos,
    }
}

/// `new TypeExpression.NameRef(name, pos)`.
pub fn name_ref(name: JStr, pos: Span) -> TypeExpression {
    TypeExpression::TypeExpressionNameRef(Box::new(TypeExpressionNameRef { name, pos }))
}

/// `new Multiplicity.Concrete(lower, upper)`.
pub fn concrete(lower: i32, upper: Option<i32>) -> Multiplicity {
    Multiplicity::MultiplicityConcrete(Box::new(MultiplicityConcrete {
        lower_bound: lower,
        upper_bound: upper,
    }))
}

/// A span's box.
pub fn boxed(s: SourceInfo) -> Span {
    Some(Box::new(s))
}
