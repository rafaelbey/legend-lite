// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The model's lists and maps as the model tier holds them (docs/rust-port/PHASE3_DESIGN.md §6 ruling 1): a list is
//! an `Arc<[T]>`, every empty one the one shared empty list (Java's `List.of()` is one object); a map is an
//! `Arc<[(K, V)]>` in Java's iteration order. The model's maps are `LinkedHashMap`s (or a one-entry `Map.of`), so their
//! order is insertion order, which [`LinkedMap`] builds: `put` replaces a present key's value in place, as
//! `LinkedHashMap.put` does, and appends a new one.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

/// A built list frozen as the model holds it: the one shared empty list when it is empty.
pub fn list<T>(v: Vec<T>) -> Arc<[T]> {
    if v.is_empty() { Arc::default() } else { Arc::from(v) }
}

/// The one shared empty list.
pub fn empty<T>() -> Arc<[T]> {
    Arc::default()
}

/// A `LinkedHashMap` being built: insertion-ordered, a key's slot kept when its value is replaced.
#[derive(Clone, Debug)]
pub struct LinkedMap<K: Eq + Hash + Clone, V> {
    entries: Vec<(K, V)>,
    index: HashMap<K, usize>,
}

impl<K: Eq + Hash + Clone, V> Default for LinkedMap<K, V> {
    fn default() -> Self {
        LinkedMap {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash + Clone, V> LinkedMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    /// A map of `entries` in their order (Java's `new LinkedHashMap<>(m)` of an ordered map; a repeated key keeps its
    /// first slot and its last value).
    pub fn of(entries: impl IntoIterator<Item = (K, V)>) -> Self {
        let mut m = Self::new();
        for (k, v) in entries {
            m.put(k, v);
        }
        m
    }

    /// `put`: the previous value, the key's slot kept.
    pub fn put(&mut self, k: K, v: V) -> Option<V> {
        match self.index.get(&k) {
            Some(&i) => Some(std::mem::replace(&mut self.entries[i].1, v)),
            None => {
                self.index.insert(k.clone(), self.entries.len());
                self.entries.push((k, v));
                None
            }
        }
    }

    /// `putIfAbsent`: whether it was put.
    pub fn put_if_absent(&mut self, k: K, v: V) -> bool {
        if self.index.contains_key(&k) {
            return false;
        }
        self.index.insert(k.clone(), self.entries.len());
        self.entries.push((k, v));
        true
    }

    /// `computeIfAbsent(k, ...)`: the value at `k`, made by `make` when absent.
    pub fn get_or_insert_with(&mut self, k: K, make: impl FnOnce() -> V) -> &mut V {
        let i = match self.index.get(&k) {
            Some(&i) => i,
            None => {
                self.index.insert(k.clone(), self.entries.len());
                self.entries.push((k, make()));
                self.entries.len() - 1
            }
        };
        &mut self.entries[i].1
    }

    pub fn get(&self, k: &K) -> Option<&V> {
        self.index.get(k).map(|&i| &self.entries[i].1)
    }

    pub fn contains_key(&self, k: &K) -> bool {
        self.index.contains_key(k)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries in iteration order.
    pub fn iter(&self) -> impl Iterator<Item = &(K, V)> {
        self.entries.iter()
    }

    /// The map as the model holds it.
    pub fn freeze(self) -> Arc<[(K, V)]> {
        list(self.entries)
    }

    /// The map with each value made the model's (`replaceAll`, then frozen).
    pub fn freeze_with<W>(self, f: impl FnMut(V) -> W) -> Arc<[(K, W)]> {
        let mut f = f;
        list(self.entries.into_iter().map(|(k, v)| (k, f(v))).collect())
    }
}

/// `map.get(key)` on a frozen map: the first entry's value at `key` (the keys of a frozen map are distinct).
pub fn get<'a, K: PartialEq, V>(map: &'a [(K, V)], key: &K) -> Option<&'a V> {
    map.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}
