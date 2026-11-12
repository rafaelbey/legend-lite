// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Subtyping over class declarations: ONE walk, the hierarchy's closure from a class ([`reach`]), and Java's two
//! relations as its two call shapes (2026-11-12, the names slice, docs/rust-port/PHASE3.md; first made in slice 1b
//! under the owner's ruling 22 of docs/rust-port/PHASE3_SHIM_DESIGN.md §10, held to the live Java, not frozen).
//!
//! Java keeps two subtype relations, and they answer differently:
//! - **F's, the typer's** (`PureModelContext.isSubtype`, [`is_subtype`]): the compiled classes' supertypes, their
//!   heads' FQNs (`TypeClassifier.headFqn`: a head that is no name fails that class, and none of its supertypes is
//!   walked); `Nil` the bottom type (reached, never walked past; a reach holding it answers true); a primitive's lattice
//!   walked like any class's; the first failure met is the question's when the answer would otherwise be false.
//! - **E's, the normalizer's** (`KnowledgeLayer.isSubtype`, lite-names' kernel, which calls [`reach`] with
//!   [`Heads::Raw`]): the declarations' raw heads (`TypeExpression.rawClassName`: a head that is no name contributes
//!   nothing, never a failure), a primitive no class (the caller's lookup answers none), no bottom.
//!
//! Each relation's class lookup is the caller's: `KnowledgeLayer.classDef` (the catalog's native classes first, then
//! the graph's) for both, `hierarchyClass` (no primitive) for E's. The walk is breadth first; a class it meets that has
//! no declaration contributes nothing; a cycle contributes nothing new.

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

/// How a walked class's supertypes are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heads {
    /// F's: `headFqn`, a head that is no name fails the class (none of its supertypes walked) and `Nil` stops the walk.
    Compiled,
    /// E's: `rawClassName`, a head that is no name contributes nothing.
    Raw,
}

/// A class's reach: the classes the walk met (itself first among them), and the first failure it met.
#[derive(Clone, Debug, Default)]
pub struct Reach {
    pub classes: HashSet<JStr>,
    pub failure: Option<&'static str>,
}

/// The hierarchy's closure from `cls` over the declarations `find` answers, supertypes read by `heads`.
pub fn reach<'a>(cls: &JStr, heads: Heads, find: impl Fn(&JStr) -> Option<&'a ClassDefinition>) -> Reach {
    let nil = JStr::from(NIL);
    let mut out = Reach::default();
    let mut work: VecDeque<JStr> = VecDeque::from([cls.clone()]);
    while let Some(cur) = work.pop_front() {
        if !out.classes.insert(cur.clone()) || (heads == Heads::Compiled && cur == nil) {
            continue;
        }
        let Some(def) = find(&cur) else { continue };
        match heads {
            Heads::Raw => work.extend(def.super_classes.iter().filter_map(|s| s.raw_class_name().cloned())),
            Heads::Compiled => {
                // Java compiles the class first: a head that is no name fails it, and none of its supertypes is walked
                let mut supers = Vec::with_capacity(def.super_classes.len());
                let mut failed = None;
                for s in def.super_classes.iter() {
                    match head_fqn(s) {
                        Ok(h) => supers.push(h.clone()),
                        Err(e) => {
                            failed = Some(e);
                            break;
                        }
                    }
                }
                match failed {
                    Some(e) => {
                        out.failure.get_or_insert(e);
                    }
                    None => work.extend(supers),
                }
            }
        }
    }
    out
}

/// `PureModelContext.isSubtype(child, parent)` (F's relation) over the declarations `find` answers: `Err` with the
/// first failure the walk met when the answer would otherwise be false (Java throws it then).
pub fn is_subtype<'a>(
    child: &JStr,
    parent: &JStr,
    find: impl Fn(&JStr) -> Option<&'a ClassDefinition>,
) -> Result<bool, &'static str> {
    if child == parent {
        return Ok(true);
    }
    let r = reach(child, Heads::Compiled, find);
    if r.classes.contains(parent) || r.classes.contains(&JStr::from(NIL)) {
        return Ok(true);
    }
    match r.failure {
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
        // E's shape: the same closure, no bottom
        let raw = reach(&JStr::from("a::C"), Heads::Raw, find);
        assert!(raw.classes.contains(&JStr::from("a::A")) && raw.failure.is_none());
        assert!(
            !reach(&JStr::from(NIL), Heads::Raw, find)
                .classes
                .contains(&JStr::from("a::A"))
        );
    }
}
