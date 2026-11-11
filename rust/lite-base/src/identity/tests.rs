// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

// docs/rust-port/PHASE3_DESIGN.md §1.5 item 3: equal but not same; same after a copy; a node rebuilt with equal
// children not same.

use super::{IdMap, IdSet, Origin, same};
use crate::arena::{Alloc, ModelArena, QueryArena};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Leaf<'q> {
    name: &'q str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Filter<'q> {
    source: &'q Leaf<'q>,
    predicate: &'q Leaf<'q>,
    columns: &'q [&'q Leaf<'q>],
}

#[test]
fn equal_records_are_not_the_same_record() {
    let cx = QueryArena::new();
    let a = cx.alloc(Leaf { name: "x" });
    let b = cx.alloc(Leaf { name: "x" });
    assert_eq!(a, b);
    assert!(!same(&a, &b));
    assert!(same(&a, &a));
}

#[test]
fn a_copy_is_the_same_record() {
    let cx = QueryArena::new();
    let a = cx.alloc(Leaf { name: "x" });
    let alias = a;
    let held = [alias];
    assert!(same(&a, &alias) && same(&a, &held[0]));
    let shared = Arc::new(String::from("a source node"));
    let clone = Arc::clone(&shared);
    assert!(same(&shared, &clone));
    assert!(!same(&shared, &Arc::new(String::from("a source node"))));
}

#[test]
fn a_node_rebuilt_with_equal_children_is_not_the_same_node() {
    let cx = QueryArena::new();
    let source = cx.alloc(Leaf { name: "t" });
    let predicate = cx.alloc(Leaf { name: "p" });
    let columns = cx.alloc_slice(&[source, predicate]);
    let filter = cx.alloc(Filter {
        source,
        predicate,
        columns,
    });
    // Java's withChildren: a new object holding the very same children
    let rebuilt = cx.alloc(Filter {
        source: filter.source,
        predicate: filter.predicate,
        columns: filter.columns,
    });
    assert_eq!(filter, rebuilt);
    assert!(!same(&filter, &rebuilt));
    assert!(same(&filter.source, &rebuilt.source) && same(&filter.columns, &rebuilt.columns));
    // and a list copied with the same elements is another list, as a Java list copied is another object
    let copied = cx.alloc_slice(columns);
    assert_eq!(columns, copied);
    assert!(!same(&columns, &copied));
    // a prefix of a list is not the list
    assert!(!same(&columns, &&columns[..1]));
}

#[test]
fn two_zero_sized_records_are_two_objects_and_every_empty_list_is_one() {
    #[derive(Clone, Copy)]
    struct Nothing;
    let cx = QueryArena::new();
    let a = cx.alloc(Nothing);
    let b = cx.alloc(Nothing);
    assert!(!same(&a, &b));
    let none: &[Leaf] = cx.alloc_slice(&[]);
    let also: &[Leaf] = cx.alloc_from_iter(Vec::new());
    assert!(same(&none, &also));
}

#[test]
fn texts_have_identity() {
    let cx = QueryArena::new();
    let a = cx.alloc_str("token");
    let b = cx.alloc_str("token");
    assert_eq!(a, b);
    assert!(!same(&a, &b) && same(&a, &a));
}

#[test]
fn an_id_map_tells_equal_keys_apart_and_finds_a_copy() {
    let cx = QueryArena::new();
    let a = cx.alloc(Leaf { name: "x" });
    let b = cx.alloc(Leaf { name: "x" });
    let mut zones: IdMap<&Leaf, &str> = IdMap::new();
    assert_eq!(zones.insert(a, "user"), None);
    assert_eq!(zones.insert(b, "system"), None);
    assert_eq!(zones.len(), 2);
    let copy = a;
    assert_eq!(zones.get(&copy), Some(&"user"));
    assert_eq!(zones.insert(copy, "both"), Some("user"));
    assert_eq!(zones.get(&b), Some(&"system"));
    *zones.get_or_insert_with(cx.alloc(Leaf { name: "x" }), || "fresh") = "set";
    assert_eq!(zones.len(), 3);
    assert_eq!(zones.remove(&b), Some("system"));
    assert!(!zones.contains_key(&b));
    assert_eq!(format!("{zones:?}"), "IdMap(2 entries)");
}

#[test]
fn an_id_map_keeps_its_arc_keys_alive() {
    let node = Arc::new(vec![1, 2, 3]);
    let mut seen: IdSet<Arc<Vec<i32>>> = IdSet::new();
    assert!(seen.insert(Arc::clone(&node)));
    assert!(!seen.insert(Arc::clone(&node)));
    assert!(seen.insert(Arc::new(vec![1, 2, 3])));
    assert_eq!(Arc::strong_count(&node), 2);
    let address_holder = Arc::clone(&node);
    drop(node);
    // the set's clone keeps the allocation, so no new node can be made at its address while the set lives
    assert!(seen.contains(&address_holder));
    assert_eq!(seen.len(), 2);
    let lists: Arc<[u8]> = Arc::from(vec![1u8, 2]);
    let mut by_list: IdMap<Arc<[u8]>, u8> = IdMap::new();
    by_list.insert(Arc::clone(&lists), 7);
    assert_eq!(by_list.get(&lists), Some(&7));
    assert_eq!(by_list.get(&Arc::from(vec![1u8, 2])), None);
}

#[test]
fn origins_name_a_source_node_or_a_fresh_token() {
    let cx = QueryArena::new();
    let call = Arc::new(String::from("an AppliedFunction"));
    let twin = Arc::new(String::from("an AppliedFunction"));
    let first = Origin::of(&cx, &call);
    let again = Origin::of(&cx, &Arc::clone(&call));
    assert_eq!(first, again);
    assert_ne!(first, Origin::of(&cx, &twin));
    let synthesized = Origin::fresh(&cx);
    assert_ne!(synthesized, Origin::fresh(&cx));
    assert_ne!(synthesized, first);
    // the query keeps every node its origins name
    assert_eq!(Arc::strong_count(&call), 3);
    drop(call);
    assert_eq!(first, again);
    assert_eq!(format!("{first:?}"), "Origin(..)");
}

#[test]
fn identity_and_tokens_cross_threads() {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Origin<'static>>();
    assert_send_sync::<IdMap<&'static Leaf<'static>, u8>>();
    let model = ModelArena::new();
    let leaf = model.alloc(Leaf { name: "shared" });
    let alias = std::thread::scope(|s| s.spawn(|| leaf).join().unwrap());
    assert!(same(&leaf, &alias));
}
