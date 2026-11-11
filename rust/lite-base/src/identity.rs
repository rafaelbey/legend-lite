// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's reference identity, for the port (docs/rust-port/PHASE3_DESIGN.md §1.4 item 2, as the arena ruling, §1.6,
//! changed it): `==` between two references is [`same`], an `IdentityHashMap` is an [`IdMap`], a
//! `newSetFromMap(new IdentityHashMap<>())` an [`IdSet`], and an `Object`-typed origin an [`Origin`].
//!
//! Identity is an object's address while the object lives. A query's records live in its [`QueryArena`], which
//! outlives every map and origin of the query, so an address is never reused under them; an `Arc` key is kept alive
//! by the map that holds it, as Java's `IdentityHashMap` holds its keys. This is the one module that turns a pointer
//! into an address (`lite-guards` holds every other phase-3 source to that), and an [`Addr`] cannot be printed,
//! ordered or turned into a number, so an address can reach no output. [`IdMap`] and [`IdSet`] have no iteration:
//! the order of an address hash is never seen (docs/architecture/README.md §5, no hash-order dependence).

use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::arena::{Alloc, QueryArena};

/// An object's identity: equal for one object, unequal for two that live at once. No order, no text, no number.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Addr {
    start: usize,
    len: usize,
}

impl fmt::Debug for Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Addr(..)")
    }
}

/// A reference whose referent has an identity: an arena record (`&'q T`), an arena slice or text (`&'q [T]`,
/// `&'q str`: one start and one length are one list, as a Java list is one object), or an `Arc` tier node.
pub trait Identity {
    fn address(&self) -> Addr;
}

impl<T> Identity for &T {
    fn address(&self) -> Addr {
        Addr {
            start: (*self as *const T).addr(),
            len: 0,
        }
    }
}

impl<T> Identity for &[T] {
    fn address(&self) -> Addr {
        Addr {
            start: self.as_ptr().addr(),
            len: self.len(),
        }
    }
}

impl Identity for &str {
    fn address(&self) -> Addr {
        Addr {
            start: self.as_ptr().addr(),
            len: self.len(),
        }
    }
}

impl<T> Identity for Arc<T> {
    fn address(&self) -> Addr {
        Addr {
            start: Arc::as_ptr(self).addr(),
            len: 0,
        }
    }
}

impl<T> Identity for Arc<[T]> {
    fn address(&self) -> Addr {
        Addr {
            start: Arc::as_ptr(self).cast::<T>().addr(),
            len: self.len(),
        }
    }
}

/// Java's `a == b` on two references: the same object, not two equal ones.
pub fn same<K: Identity>(a: &K, b: &K) -> bool {
    a.address() == b.address()
}

/// Java's `IdentityHashMap`: keyed by identity, holding its keys (so an `Arc` key lives while the map does), with
/// lookup and insert and no iteration.
pub struct IdMap<K: Identity, V> {
    entries: HashMap<Addr, (K, V)>,
}

impl<K: Identity, V> IdMap<K, V> {
    pub fn new() -> IdMap<K, V> {
        IdMap {
            entries: HashMap::new(),
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(&key.address()).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.entries.get_mut(&key.address()).map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(&key.address())
    }

    /// Java's `put`: the value `key` held before, if any.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.entries.insert(key.address(), (key, value)).map(|(_, v)| v)
    }

    /// Java's `computeIfAbsent`.
    pub fn get_or_insert_with(&mut self, key: K, make: impl FnOnce() -> V) -> &mut V {
        &mut self.entries.entry(key.address()).or_insert_with(|| (key, make())).1
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.entries.remove(&key.address()).map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<K: Identity, V> Default for IdMap<K, V> {
    fn default() -> IdMap<K, V> {
        IdMap::new()
    }
}

impl<K: Identity, V> fmt::Debug for IdMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IdMap({} entries)", self.entries.len())
    }
}

/// `Collections.newSetFromMap(new IdentityHashMap<>())`: a set by identity, holding its members, with no iteration.
pub struct IdSet<K: Identity> {
    members: IdMap<K, ()>,
}

impl<K: Identity> IdSet<K> {
    pub fn new() -> IdSet<K> {
        IdSet { members: IdMap::new() }
    }

    /// Java's `add`: true when `key` was not a member.
    pub fn insert(&mut self, key: K) -> bool {
        self.members.insert(key, ()).is_none()
    }

    pub fn contains(&self, key: &K) -> bool {
        self.members.contains_key(key)
    }

    pub fn remove(&mut self, key: &K) -> bool {
        self.members.remove(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

impl<K: Identity> Default for IdSet<K> {
    fn default() -> IdSet<K> {
        IdSet::new()
    }
}

impl<K: Identity> fmt::Debug for IdSet<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IdSet({} members)", self.members.len())
    }
}

/// A typed node's origin (`TypedUserCall.origin`, `Evaluation.origin`, `CallInstantiation.frame`): an address token,
/// equal for one source node (or one fresh token) and never dereferenced. Its query keeps what it names alive, so
/// the address is not reused while the query lives (PHASE3_DESIGN.md §1.6 item 5).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Origin<'q> {
    addr: Addr,
    /// The query the token belongs to: it cannot outlive the arena that keeps its node.
    query: PhantomData<&'q ()>,
}

impl<'q> Origin<'q> {
    /// The origin naming the source node `node`; the query's keep pool holds a clone of it.
    pub fn of<T: Send + Sync + 'static>(cx: &'q QueryArena, node: &Arc<T>) -> Origin<'q> {
        cx.keep(Arc::clone(node));
        Origin {
            addr: node.address(),
            query: PhantomData,
        }
    }

    /// A token no other origin equals: Java's `new Object()` (a synthesized node's origin, a call site's frame). A
    /// byte of the query's arena, so no address of a live node or token can be its.
    pub fn fresh(cx: &'q QueryArena) -> Origin<'q> {
        let token: &'q u8 = cx.alloc(0u8);
        Origin {
            addr: token.address(),
            query: PhantomData,
        }
    }
}

impl fmt::Debug for Origin<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Origin(..)")
    }
}

#[cfg(test)]
mod tests;
