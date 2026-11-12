// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The protocol records' convenience constructors (core/'s `protocol.spec` records), as the parser calls them: each
//! fills the components Java's shorter constructor fills (`AppliedFunction(function, parameters)` has no referents,
//! no span, every flag false; `EnumValue`'s four-argument form sets `node` when it has no enumeration span; ...).
//! A `ValueSpecification` node is an `Arc`, its lists `Arc<[T]>` (records.rs); the helpers take a list as a built `Vec`
//! or, its length known where it is written, an array, and freeze it ([`IntoList`]).

use std::sync::Arc;

/// A node's list frozen into its `Arc<[T]>` (2026-11-11, docs/rust-port/PHASE3.md step 1, the flip's cost won back): an
/// empty list is the one shared empty list, `Arc::<[T]>::default()`, which allocates nothing, as Java's `List.of()` is
/// one object; an array goes straight in, its length known, with no `Vec` first (one allocation); a `Vec` built by the
/// parser is moved into one (its own buffer then freed).
pub trait IntoList<T> {
    fn into_list(self) -> Arc<[T]>;
}

impl<T> IntoList<T> for Vec<T> {
    fn into_list(self) -> Arc<[T]> {
        if self.is_empty() {
            Arc::default()
        } else {
            Arc::from(self)
        }
    }
}

impl<T, const N: usize> IntoList<T> for [T; N] {
    fn into_list(self) -> Arc<[T]> {
        if N == 0 { Arc::default() } else { Arc::from(self) }
    }
}

/// A list already frozen (by [`list_of`]): itself.
impl<T> IntoList<T> for Arc<[T]> {
    fn into_list(self) -> Arc<[T]> {
        self
    }
}

/// An iterator's items as a node's list, its length known before it is read (a `Vec`'s, a slice's, an array's, `once`,
/// and their maps and chains): one allocation, none for an empty one (the shared empty list).
pub fn list_of<T>(items: impl Iterator<Item = T>) -> Arc<[T]> {
    if items.size_hint().1 == Some(0) {
        Arc::default()
    } else {
        items.collect()
    }
}

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
    parameters: impl IntoList<ValueSpecification>,
    pos: Span,
    property_call: bool,
    grouped: bool,
    infix: bool,
    island: bool,
) -> AppliedFunction {
    AppliedFunction {
        function,
        parameters: parameters.into_list(),
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
pub fn applied(function: JStr, parameters: impl IntoList<ValueSpecification>, pos: Span) -> AppliedFunction {
    applied_full(function, parameters, pos, false, false, false, false)
}

/// `new AppliedFunction(function, parameters, List.of(), pos, false, false, true)`: an operator's call.
pub fn infix(function: JStr, parameters: impl IntoList<ValueSpecification>, pos: Span) -> AppliedFunction {
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
pub fn collection(values: impl IntoList<ValueSpecification>, pos: Span) -> ValueSpecification {
    ValueSpecification::PureCollection(Arc::new(PureCollection {
        values: values.into_list(),
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
pub fn lambda(
    parameters: impl IntoIterator<Item = Variable>,
    body: impl IntoList<ValueSpecification>,
    pos: Span,
) -> LambdaFunction {
    LambdaFunction {
        parameters: list_of(parameters.into_iter().map(Arc::new)),
        body: body.into_list(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_list_is_the_one_shared_list() {
        // 2026-11-11: every empty list, however it is built, holds the one shared empty list (its count says it is
        // held more than once), so none allocates; a full one is its own
        let empties: [Arc<[ValueSpecification]>; 4] = [
            Vec::new().into_list(),
            [].into_list(),
            list_of(std::iter::empty()),
            applied(JStr::from("f"), Vec::new(), None).parameters,
        ];
        for e in &empties {
            assert!(e.is_empty());
            assert!(Arc::strong_count(e) >= empties.len(), "{}", Arc::strong_count(e));
        }
        let full = [cstring(JStr::from("a"), None)].into_list();
        assert_eq!((full.len(), Arc::strong_count(&full)), (1, 1));
        let lam = lambda([variable(JStr::from("x"))], vec![ptr(JStr::from("a::B"), None)], None);
        assert_eq!((lam.parameters.len(), lam.body.len()), (1, 1));
    }
}
