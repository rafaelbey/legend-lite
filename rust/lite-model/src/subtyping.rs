// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Subtyping over class declarations (`ModelContext.isSubtype` as `PureModelContext` answers it; 2026-11-12, the
//! owner's ruling 22 of docs/rust-port/PHASE3_SHIM_DESIGN.md §10: held to the live Java in slice 1b, with the model, not
//! frozen). `child <: parent` when they are one name, or when the child's reach -- the child and, walked breadth first,
//! every class a reached class names as a supertype (its head's FQN, `TypeClassifier.headFqn`), `Nil` not walked past --
//! holds the parent or `Nil`, the bottom type. A class the walk meets that has no declaration contributes nothing.
//!
//! Java walks the compiled classes' `superClassFqns`, which are the declarations' heads after name resolution; this walks
//! the declarations a caller hands, so their supertypes must be spelled as FQNs (the platform's boot layer is: the
//! catalog's, the prelude's and the system metamodel's classes). A head that is no name (a function, relation or
//! schema-algebra type) is Java's `ModelException`, the first one met answered as the walk's failure, as Java answers a
//! class that failed to compile on the way.

use std::collections::{HashSet, VecDeque};

use lite_front::jtypes::JStr;
use lite_front::records as p;

use crate::records::ClassDefinition;

/// `meta::pure::metamodel::type::Nil`, the bottom type.
pub const NIL: &str = "meta::pure::metamodel::type::Nil";

/// `TypeClassifier.headFqn`: a supertype reference's head FQN; a structural form refused with Java's message.
pub fn head_fqn(t: &p::TypeExpression) -> Result<&JStr, &'static str> {
    match t {
        p::TypeExpression::TypeExpressionNameRef(n) => Ok(&n.name),
        p::TypeExpression::TypeExpressionGeneric(g) => Ok(&g.name),
        p::TypeExpression::TypeExpressionFunctionType(_) => {
            Err("a function type cannot head a supertype/generic reference")
        }
        p::TypeExpression::TypeExpressionRelationType(_) => {
            Err("a relation type cannot head a supertype/generic reference")
        }
        p::TypeExpression::TypeExpressionSchemaAlgebra(_) => {
            Err("a schema-algebra expression cannot head a supertype/generic reference")
        }
    }
}

/// `PureModelContext.isSubtype(child, parent)` over the declarations `find` answers: `Err` with the first failure the
/// walk met when the answer would otherwise be false (Java throws it then).
pub fn is_subtype<'a>(
    child: &JStr,
    parent: &JStr,
    find: impl Fn(&JStr) -> Option<&'a ClassDefinition>,
) -> Result<bool, &'static str> {
    if child == parent {
        return Ok(true);
    }
    let nil = JStr::from(NIL);
    let mut reach: HashSet<JStr> = HashSet::new();
    let mut failure: Option<&'static str> = None;
    let mut work: VecDeque<JStr> = VecDeque::from([child.clone()]);
    while let Some(cur) = work.pop_front() {
        if !reach.insert(cur.clone()) || cur == nil {
            continue;
        }
        if let Some(def) = find(&cur) {
            // Java compiles the class first: a head that is no name fails it, and none of its supertypes is walked
            let mut heads = Vec::with_capacity(def.super_classes.len());
            let mut failed = None;
            for s in def.super_classes.iter() {
                match head_fqn(s) {
                    Ok(h) => heads.push(h.clone()),
                    Err(e) => {
                        failed = Some(e);
                        break;
                    }
                }
            }
            match failed {
                Some(e) => {
                    failure.get_or_insert(e);
                }
                None => work.extend(heads),
            }
        }
    }
    if reach.contains(parent) || reach.contains(&nil) {
        return Ok(true);
    }
    match failure {
        Some(e) => Err(e),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn class(fqn: &str, supers: &[&str]) -> Arc<ClassDefinition> {
        let units: Vec<u16> = if supers.is_empty() {
            format!("Class {fqn} {{}}")
        } else {
            format!("Class {fqn} extends {} {{}}", supers.join(", "))
        }
        .encode_utf16()
        .collect();
        let parsed = lite_front::doors::pmcd(&units, lite_front::doors::Dialect::LegendLite, &[])
            .parsed
            .unwrap();
        let lite_front::records::Element::PClass(c) = &parsed.model.elements[0] else {
            panic!("a class")
        };
        Arc::new(crate::from_protocol::to_class_definition(c))
    }

    #[test]
    fn the_walk_is_the_declared_closure_with_nil_at_the_bottom() {
        let defs: HashMap<JStr, Arc<ClassDefinition>> = [
            class("a::A", &[]),
            class("a::B", &["a::A"]),
            class("a::C", &["a::B", "a::D"]),
            // a cycle the frontend accepts contributes nothing new
            class("a::X", &["a::Y"]),
            class("a::Y", &["a::X"]),
        ]
        .into_iter()
        .map(|c| (c.qualified_name.clone(), c))
        .collect();
        let find = |f: &JStr| defs.get(f).map(|c| &**c);
        let is = |c: &str, p: &str| is_subtype(&JStr::from(c), &JStr::from(p), find).unwrap();
        assert!(is("a::C", "a::A"));
        assert!(is("a::C", "a::D"), "an undeclared supertype is reached by its name");
        assert!(!is("a::A", "a::C"));
        assert!(is("a::Q", "a::Q"));
        assert!(!is("a::X", "a::A"));
        assert!(is(NIL, "a::A"), "Nil is below every type");
        assert!(!is("a::A", NIL));
    }
}
