// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `SymbolTable` (2026-11-12): fully-qualified names interned to dense ids, `0` first, stable for the table's life.

use std::collections::HashMap;

use lite_model::JStr;

/// `SymbolTable.UNRESOLVED`: the id of a name never interned.
pub const UNRESOLVED: i32 = -1;

/// `SymbolTable`: FQN to id and back.
#[derive(Clone, Debug, Default)]
pub struct SymbolTable {
    id_by_fqn: HashMap<JStr, i32>,
    fqn_by_id: Vec<JStr>,
}

impl SymbolTable {
    pub fn new() -> SymbolTable {
        SymbolTable::default()
    }

    /// `intern(fqn)`: the name's id, a fresh one the first time.
    pub fn intern(&mut self, fqn: &JStr) -> i32 {
        if let Some(&id) = self.id_by_fqn.get(fqn) {
            return id;
        }
        let id = self.fqn_by_id.len() as i32;
        self.id_by_fqn.insert(fqn.clone(), id);
        self.fqn_by_id.push(fqn.clone());
        id
    }

    /// `resolveId(fqn)`: the name's id, or [`UNRESOLVED`]; never interns.
    pub fn resolve_id(&self, fqn: &JStr) -> i32 {
        self.id_by_fqn.get(fqn).copied().unwrap_or(UNRESOLVED)
    }

    /// `nameOf(id)`: the name at `id`; `None` out of bounds (Java's `IndexOutOfBoundsException`).
    pub fn name_of(&self, id: i32) -> Option<&JStr> {
        usize::try_from(id).ok().and_then(|i| self.fqn_by_id.get(i))
    }

    /// `size()`.
    pub fn size(&self) -> usize {
        self.fqn_by_id.len()
    }

    /// Whether `fqn` was interned (Java's `allFqns().contains`; the set's iteration order is a hash's, so the port
    /// offers membership alone).
    pub fn contains(&self, fqn: &JStr) -> bool {
        self.id_by_fqn.contains_key(fqn)
    }
}
