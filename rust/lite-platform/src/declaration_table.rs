// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `DeclarationTable`: every function declaration the platform knows, one per `FunctionId`. Its sources are the
//! standard library declared upstream, the catalog, and the platform's own modules; the table does not care which,
//! only that one id is one declaration.
//!
//! The same id met twice is the same entity: a catalog native and upstream's declaration of that id are one
//! declaration, and the one with a body is kept. Two different bodies under one id cannot both be kept: the first is,
//! and the id is reported in [`DeclarationTable::duplicates`].
//!
//! Ported from core's `com.legend.platform.DeclarationTable` (2026-11-12, docs/rust-port/PHASE3.md slice 1b). Java's
//! by-id map is a `LinkedHashMap` (a replaced declaration keeps its slot): its order is the table's id order. Its by-FQN
//! map is asked by name only, so it is a hash map here.

use std::collections::HashMap;

use lite_model::linked::LinkedMap;
use lite_model::{Function, FunctionId, JStr};

/// Every function declaration, one per id.
#[derive(Clone, Debug)]
pub struct DeclarationTable {
    by_id: Vec<(FunctionId, Function)>,
    index: HashMap<FunctionId, usize>,
    by_fqn: HashMap<JStr, Vec<Function>>,
    duplicates: Vec<JStr>,
}

impl DeclarationTable {
    /// `DeclarationTable.of(declarations)`: the table over `declarations`, merged by id.
    pub fn of(declarations: impl IntoIterator<Item = Function>) -> DeclarationTable {
        let mut by_id: LinkedMap<FunctionId, Function> = LinkedMap::new();
        let mut duplicates = Vec::new();
        for f in declarations {
            let id = FunctionId::of(&f);
            match by_id.get(&id) {
                None => {
                    by_id.put(id, f);
                }
                Some(prior) => {
                    let bodied = matches!(f, Function::FunctionDefinition(_));
                    // `!f.equals(prior)`: Java's record equality, the protocol records' source positions left out
                    // (`Function::java_equals`; the derived `!=` read them, so a graph's copy of a boot function parsed
                    // elsewhere was a duplicate -- found by the element differential, 2026-11-15)
                    if bodied && matches!(prior, Function::FunctionDefinition(_)) && !f.java_equals(prior) {
                        // each id once, in first-met order (2026-11-12, the owner's ruling: a third differing body
                        // reported the id again, in Java and here)
                        if !duplicates.contains(&id.qualified) {
                            duplicates.push(id.qualified);
                        }
                    } else if bodied {
                        // the bodied declaration is upstream's reference
                        by_id.put(id, f);
                    }
                }
            }
        }
        let by_id: Vec<(FunctionId, Function)> = by_id.freeze().to_vec();
        let index = by_id.iter().enumerate().map(|(i, (id, _))| (id.clone(), i)).collect();
        let mut by_fqn: HashMap<JStr, Vec<Function>> = HashMap::new();
        for (_, f) in &by_id {
            by_fqn.entry(f.qualified_name().clone()).or_default().push(f.clone());
        }
        DeclarationTable {
            by_id,
            index,
            by_fqn,
            duplicates,
        }
    }

    /// `get(id)`: the declaration with exactly this id, if any.
    pub fn get(&self, id: &FunctionId) -> Option<&Function> {
        self.index.get(id).map(|&i| &self.by_id[i].1)
    }

    /// `at(fqn)`: every declaration at `fqn` (its overloads), in insertion order.
    pub fn at(&self, fqn: &JStr) -> &[Function] {
        self.by_fqn.get(fqn).map_or(&[], Vec::as_slice)
    }

    /// `duplicates()`: the ids declared by two different bodies (the first kept), each once, in the order first met.
    pub fn duplicates(&self) -> &[JStr] {
        &self.duplicates
    }

    /// `ids()`: every id, in insertion order.
    pub fn ids(&self) -> impl Iterator<Item = &FunctionId> {
        self.by_id.iter().map(|(id, _)| id)
    }

    /// Every id with its declaration, in insertion order.
    pub fn entries(&self) -> &[(FunctionId, Function)] {
        &self.by_id
    }

    /// `size()`.
    pub fn size(&self) -> usize {
        self.by_id.len()
    }
}

/// DeclarationTableTest: the table's identity and merge rules.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{UPPER_ID, catalog, declare, id, native, on_stack};

    /// A catalog native and upstream's bodied declaration of the same id are one declaration; the bodied one
    /// (upstream's reference) is kept.
    #[test]
    fn one_id_is_one_declaration_and_the_bodied_one_is_kept() {
        on_stack(|| {
            let mut fs = vec![native(&catalog(UPPER_ID))];
            fs.extend(declare(
                "function meta::pure::functions::string::toUpper(source:String[1]):String[1] { $source }",
            ));
            let t = DeclarationTable::of(fs);
            assert_eq!(t.size(), 1);
            assert!(matches!(t.get(&id(UPPER_ID)), Some(Function::FunctionDefinition(_))));
        });
    }

    #[test]
    fn overloads_are_distinct_declarations_at_one_fqn() {
        on_stack(|| {
            let t = DeclarationTable::of(declare(
                "function my::pkg::twice(x:Integer[1]):Integer[1] { $x }\n\
                 function my::pkg::twice(x:String[1]):String[1] { $x }",
            ));
            assert_eq!(t.size(), 2);
            assert_eq!(t.at(&JStr::from("my::pkg::twice")).len(), 2);
            assert_eq!(t.at(&JStr::from("my::pkg::thrice")).len(), 0);
        });
    }

    #[test]
    fn the_same_body_twice_is_one_declaration() {
        on_stack(|| {
            let mut fs = declare("function my::pkg::one(x:Integer[1]):Integer[1] { $x }");
            fs.extend(declare("function my::pkg::one(x:Integer[1]):Integer[1] { $x }"));
            assert_eq!(DeclarationTable::of(fs).size(), 1);
        });
    }

    /// Java's `equals` leaves the protocol records' source positions out: one function parsed at two places is one
    /// declaration, never a duplicate (2026-11-15, the element slice: the derived `!=` read the positions, found by
    /// the element differential on a graph's copy of a boot function).
    #[test]
    fn the_same_body_at_two_places_is_one_declaration() {
        on_stack(|| {
            let mut fs = declare("function my::pkg::one(x:Integer[1]):Integer[1] { $x }");
            fs.extend(declare("\n\n   function my::pkg::one(x:Integer[1]):Integer[1]\n{\n  $x\n}"));
            assert_ne!(fs[0], fs[1], "the positions differ");
            assert!(fs[0].java_equals(&fs[1]));
            let t = DeclarationTable::of(fs);
            assert_eq!(t.size(), 1);
            assert!(t.duplicates().is_empty());
        });
    }

    #[test]
    fn two_different_bodies_under_one_id_are_reported_and_the_first_kept() {
        on_stack(|| {
            let mut fs = declare("function my::pkg::one(x:Integer[1]):Integer[1] { $x }");
            fs.extend(declare("function my::pkg::one(x:Integer[1]):Integer[1] { 1 }"));
            let t = DeclarationTable::of(fs.clone());
            assert_eq!(t.duplicates(), &[JStr::from("my::pkg::one_Integer_1__Integer_1_")]);
            assert_eq!(t.get(&id("my::pkg::one_Integer_1__Integer_1_")), Some(&fs[0]));
            assert_eq!(t.size(), 1);
        });
    }

    #[test]
    fn an_id_with_three_different_bodies_is_reported_once() {
        // 2026-11-12, the owner's ruling: each id once, however many bodies differ from the first
        on_stack(|| {
            let mut fs = declare("function my::pkg::one(x:Integer[1]):Integer[1] { $x }");
            fs.extend(declare("function my::pkg::one(x:Integer[1]):Integer[1] { 1 }"));
            fs.extend(declare("function my::pkg::one(x:Integer[1]):Integer[1] { 2 }"));
            let t = DeclarationTable::of(fs.clone());
            assert_eq!(t.duplicates(), &[JStr::from("my::pkg::one_Integer_1__Integer_1_")]);
            assert_eq!(t.get(&id("my::pkg::one_Integer_1__Integer_1_")), Some(&fs[0]));
        });
    }

    /// The catalog declares each id once: the id spells types by their short names, so two catalog functions that
    /// differ only by a package or a type argument would collide.
    #[test]
    fn the_catalog_declares_each_id_once() {
        on_stack(|| {
            let all = lite_builtin::pure::catalog().all();
            let t = DeclarationTable::of(all.iter().map(native));
            assert_eq!(t.size(), all.len(), "two catalog declarations share an id");
        });
    }
}
