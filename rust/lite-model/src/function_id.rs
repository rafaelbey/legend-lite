// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! A function's identity (`FunctionId`): its engine signature id, generated from the declaration by
//! `SignatureMangle.mangle` and compared whole, never parsed back apart; and `SignatureMangle.resolve`, a reference's
//! declarations by spelling each one under a prefix of the reference.

use std::cmp::Ordering;
use std::sync::Arc;

use lite_front::jtypes::JStr;
use lite_front::records as p;

use crate::records::{Function, FunctionId};
use crate::text::last_index_of;

impl FunctionId {
    /// `new FunctionId(qualified)`.
    pub fn new(qualified: JStr) -> FunctionId {
        FunctionId { qualified }
    }

    /// `FunctionId.of(declaration)`.
    pub fn of(declaration: &Function) -> FunctionId {
        FunctionId {
            qualified: mangle(declaration),
        }
    }

    /// `FunctionId.ofAll(declarations...)`.
    pub fn of_all(declarations: &[Function]) -> Arc<[FunctionId]> {
        crate::linked::list(declarations.iter().map(FunctionId::of).collect())
    }

    /// `FunctionId.all(groups...)`: several identity groups as one list.
    pub fn all(groups: &[&Arc<[FunctionId]>]) -> Arc<[FunctionId]> {
        if groups.len() == 1 {
            return groups[0].clone();
        }
        crate::linked::list(groups.iter().flat_map(|g| g.iter().cloned()).collect())
    }
}

/// `FunctionId implements Comparable`: by the qualified id, as `String.compareTo` compares (UTF-16 units).
impl PartialOrd for FunctionId {
    fn partial_cmp(&self, other: &FunctionId) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FunctionId {
    fn cmp(&self, other: &FunctionId) -> Ordering {
        self.qualified.0.cmp(&other.qualified.0)
    }
}

impl std::fmt::Display for FunctionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.qualified)
    }
}

/// `SignatureMangle.mangle(def)`: the qualified name, then a `_Type_mult_` segment per parameter and one for the
/// return type (an underscore first when there are no parameters).
pub fn mangle(def: &Function) -> JStr {
    let mut id: Vec<u16> = def.qualified_name().0.clone();
    if def.parameters().is_empty() {
        id.push(u16::from(b'_'));
    }
    for p in def.parameters().iter() {
        segment(&mut id, &p.r#type, &p.multiplicity);
    }
    segment(&mut id, def.return_type(), def.return_multiplicity());
    JStr(id)
}

/// `SignatureMangle.tail(def)`: the id without the qualified name.
pub fn tail(def: &Function) -> JStr {
    let m = mangle(def);
    JStr(m.0[def.qualified_name().len()..].to_vec())
}

fn segment(id: &mut Vec<u16>, ty: &p::TypeExpression, mult: &p::Multiplicity) {
    id.push(u16::from(b'_'));
    id.extend_from_slice(&type_id(ty));
    id.push(u16::from(b'_'));
    id.extend(mult_id(mult).encode_utf16_units());
    id.push(u16::from(b'_'));
}

/// The raw simple name: a type parameter its own name, a function or relation type its metaclass.
fn type_id(t: &p::TypeExpression) -> Vec<u16> {
    let q: Vec<u16> = match t {
        p::TypeExpression::TypeExpressionNameRef(n) => n.name.0.clone(),
        p::TypeExpression::TypeExpressionGeneric(g) => g.name.0.clone(),
        p::TypeExpression::TypeExpressionFunctionType(_) => "Function".encode_utf16().collect(),
        p::TypeExpression::TypeExpressionRelationType(_) | p::TypeExpression::TypeExpressionSchemaAlgebra(_) => {
            "Relation".encode_utf16().collect()
        }
    };
    let sep: Vec<u16> = "::".encode_utf16().collect();
    match last_index_of(&q, &sep) {
        Some(cut) => q[cut + 2..].to_vec(),
        None => q,
    }
}

/// A multiplicity's segment: `1`, `MANY`, `$lo_MANY$`, `$lo_hi$`, or the parameter's own name.
enum MultId<'a> {
    Text(String),
    Name(&'a JStr),
}

impl MultId<'_> {
    fn encode_utf16_units(&self) -> Vec<u16> {
        match self {
            MultId::Text(s) => s.encode_utf16().collect(),
            MultId::Name(n) => n.0.clone(),
        }
    }
}

fn mult_id(m: &p::Multiplicity) -> MultId<'_> {
    match m {
        p::Multiplicity::MultiplicityParameter(p) => MultId::Name(&p.name),
        p::Multiplicity::MultiplicityConcrete(c) => MultId::Text(match c.upper_bound {
            None if c.lower_bound == 0 => "MANY".to_owned(),
            None => format!("${}_MANY$", c.lower_bound),
            Some(upper) if c.lower_bound == upper => upper.to_string(),
            Some(upper) => format!("${}_{}$", c.lower_bound, upper),
        }),
    }
}

/// `SignatureMangle.Resolution`: the declarations whose id is exactly the reference, whether some declaration exists
/// under a prefix of it, and those under the longest such base.
#[derive(Clone, Debug)]
pub struct Resolution<F> {
    pub exact: Vec<F>,
    pub base_exists: bool,
    pub base: Vec<F>,
}

/// `SignatureMangle.resolve(ref, lookup, def)`: every `_` in the reference's last segment a candidate cut, longest
/// first; the first cut with an exact match answers.
pub fn resolve<F: Clone>(
    reference: &JStr,
    lookup: impl Fn(&[u16]) -> Vec<F>,
    def: impl Fn(&F) -> Option<Function>,
) -> Resolution<F> {
    let r = &reference.0;
    let sep: Vec<u16> = "::".encode_utf16().collect();
    // Java's Math.max(ref.lastIndexOf("::") + 2, 0): 1 when the reference has no package
    let from = last_index_of(r, &sep).map_or(1, |i| i + 2);
    let mut base_exists = false;
    let mut base: Vec<F> = Vec::new();
    let mut i = r.len();
    while i > from + 1 {
        i -= 1;
        if r[i] != u16::from(b'_') {
            continue;
        }
        let candidates = lookup(&r[..i]);
        if candidates.is_empty() {
            continue;
        }
        base_exists = true;
        if base.is_empty() {
            base = candidates.clone();
        }
        let mut exact = Vec::new();
        for c in &candidates {
            if let Some(d) = def(c) {
                let mut spelled = r[..i].to_vec();
                spelled.extend_from_slice(&tail(&d).0);
                if spelled == *r {
                    exact.push(c.clone());
                }
            }
        }
        if !exact.is_empty() {
            return Resolution {
                exact,
                base_exists: true,
                base,
            };
        }
    }
    Resolution {
        exact: Vec::new(),
        base_exists,
        base,
    }
}
