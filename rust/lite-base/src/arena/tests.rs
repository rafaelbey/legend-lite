// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

use super::{Alloc, CHUNK_ALIGN, FIRST_CHUNK, MAX_CHUNK, ModelArena, QueryArena};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

// ---- what the type system holds: a query's arena is Send and not Sync, the model's Send and Sync

const fn assert_send<T: Send>() {}
const fn assert_sync<T: Sync>() {}
const _: () = assert_send::<QueryArena>();
const _: () = assert_send::<ModelArena>();
const _: () = assert_sync::<ModelArena>();

/// `QueryArena: !Sync`, at compile time: were it Sync, `NotSync::<_>::check` would have two candidates and the
/// constant below would not compile (the `static_assertions` crate's trick).
trait NotSync<A> {
    fn check() {}
}
impl<T: ?Sized> NotSync<()> for T {}
impl<T: ?Sized + Sync> NotSync<u8> for T {}
const _: fn() = || <QueryArena as NotSync<_>>::check();

fn aligned<T>(r: &T) -> bool {
    (r as *const T).is_aligned()
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(align(64))]
struct Wide(u8);

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(align(4096))]
struct Page(u16);

#[test]
fn every_value_is_aligned_and_reads_back() {
    let cx = QueryArena::new();
    let mut held = Vec::new();
    for i in 0..2000u32 {
        let b = cx.alloc(i as u8);
        let q = cx.alloc(u64::from(i) * 7);
        let w = cx.alloc(u128::from(i) << 70);
        let s = cx.alloc((i as u16, i, i as u8));
        let x = cx.alloc(Wide(i as u8));
        assert!(aligned(b) && aligned(q) && aligned(w) && aligned(s) && aligned(x));
        held.push((b, q, w, s, x));
    }
    let p = cx.alloc(Page(7));
    assert!(aligned(p));
    assert_eq!(*p, Page(7));
    // nothing overlapped: every value is still the one written
    for (i, (b, q, w, s, x)) in held.into_iter().enumerate() {
        let i = i as u32;
        assert_eq!(
            (*b, *q, *w, *s, *x),
            (
                i as u8,
                u64::from(i) * 7,
                u128::from(i) << 70,
                (i as u16, i, i as u8),
                Wide(i as u8)
            )
        );
    }
    const { assert!(CHUNK_ALIGN >= std::mem::align_of::<u128>()) };
}

#[test]
fn zero_sized_values_are_each_one_byte_and_aligned() {
    #[derive(Clone, Copy)]
    struct Unit;
    let cx = QueryArena::new();
    let before = cx.bytes();
    let units: Vec<&Unit> = (0..10_000).map(|_| cx.alloc(Unit)).collect();
    assert_eq!(units.len(), 10_000);
    let empty: &[u64; 0] = cx.alloc([]);
    assert!(aligned(empty));
    let unit_slice = cx.alloc_slice(&[(), (), ()]);
    assert_eq!(unit_slice.len(), 3);
    // ten thousand and two bytes, so a few small chunks, never one per value
    assert!(cx.bytes() - before < 64 << 10, "{} bytes", cx.bytes());
}

#[test]
fn empty_slices_and_texts_take_no_space() {
    let cx = QueryArena::new();
    let empty: &[u32] = cx.alloc_slice(&[]);
    assert!(empty.is_empty());
    assert_eq!(cx.alloc_str(""), "");
    assert_eq!(cx.alloc_from_iter(Vec::<u8>::new()), &[] as &[u8]);
    assert_eq!(cx.chunks(), 0);
}

#[test]
fn slices_and_texts_are_copies() {
    let cx = QueryArena::new();
    let mut source = vec![1u32, 2, 3];
    let copy = cx.alloc_slice(&source);
    source[0] = 99;
    assert_eq!(copy, &[1, 2, 3]);
    let text = String::from("héllo, \u{1F600} ⊆");
    let held = cx.alloc_str(&text);
    drop(text);
    assert_eq!(held, "héllo, \u{1F600} ⊆");
    let squares = cx.alloc_from_iter((0..100u32).map(|i| u64::from(i * i)));
    assert_eq!(squares[99], 9801);
}

#[test]
fn an_iterator_may_allocate_while_it_is_copied() {
    let cx = QueryArena::new();
    let refs: &[&u64] = cx.alloc_from_iter((0..500u32).map(|i| cx.alloc(u64::from(i) + 1)));
    assert_eq!(refs.iter().map(|r| **r).sum::<u64>(), 500 * 501 / 2);
}

struct Lying(usize, usize);

impl Iterator for Lying {
    type Item = u8;
    fn next(&mut self) -> Option<u8> {
        if self.1 == 0 {
            return None;
        }
        self.1 -= 1;
        Some(1)
    }
}

impl ExactSizeIterator for Lying {
    fn len(&self) -> usize {
        self.0
    }
}

#[test]
#[should_panic(expected = "fewer values than its len")]
fn an_iterator_shorter_than_its_len_is_refused() {
    QueryArena::new().alloc_from_iter(Lying(5, 3));
}

#[test]
#[should_panic(expected = "more values than its len")]
fn an_iterator_longer_than_its_len_is_refused() {
    QueryArena::new().alloc_from_iter(Lying(3, 5));
}

#[test]
fn large_values_get_a_chunk_of_their_own_and_the_current_chunk_goes_on() {
    let cx = QueryArena::new();
    let small = cx.alloc(1u64);
    assert_eq!(cx.chunks(), 1);
    let big = cx.alloc([7u8; 64 << 10]);
    assert_eq!(cx.chunks(), 2);
    let huge = cx.alloc_slice(&vec![3u32; 4 << 20]);
    assert_eq!(cx.chunks(), 3);
    // the next small value goes into the first chunk still
    let after = cx.alloc(2u64);
    assert_eq!(cx.chunks(), 3);
    assert_eq!(
        (*small, big[65535], huge.len(), huge[(4 << 20) - 1], *after),
        (1, 7, 4 << 20, 3, 2)
    );
    assert!(cx.bytes() >= (64 << 10) + (16 << 20));
}

#[test]
fn chunks_grow_by_doubling_to_a_ceiling() {
    let cx = QueryArena::new();
    let values: Vec<&[u8; 64]> = (0..100_000).map(|i| cx.alloc([i as u8; 64])).collect();
    assert!(values.iter().enumerate().all(|(i, v)| v[63] == i as u8));
    // 6.4 MB: 4 KiB, 8 KiB, ... 1 MiB, then 1 MiB chunks: about a dozen, never one per value
    let chunks = cx.chunks();
    assert!((9..=20).contains(&chunks), "{chunks} chunks");
    let ceiling = FIRST_CHUNK * 2usize.pow(8);
    assert_eq!(ceiling, MAX_CHUNK);
}

struct Counted(Arc<AtomicUsize>, String);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn the_keep_pool_owns_its_values_until_the_arena_drops() {
    let drops = Arc::new(AtomicUsize::new(0));
    let source = Arc::new(String::from("a source node"));
    {
        let cx = QueryArena::new();
        let first = cx.keep(Counted(drops.clone(), "first".into()));
        let tree = cx.keep(Box::new(vec![String::from("a"), String::from("b")]));
        let node = cx.keep(Arc::clone(&source));
        // the pool's own vector grows; what it keeps does not move
        let more: Vec<&Counted> = (0..1000)
            .map(|i| cx.keep(Counted(drops.clone(), i.to_string())))
            .collect();
        assert_eq!(
            (first.1.as_str(), tree[1].as_str(), node.as_str(), more[999].1.as_str()),
            ("first", "b", "a source node", "999")
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(Arc::strong_count(&source), 2);
        assert_eq!(cx.kept(), 1003);
    }
    assert_eq!(drops.load(Ordering::SeqCst), 1001);
    assert_eq!(Arc::strong_count(&source), 1);
}

#[test]
fn a_query_arena_moves_to_another_thread() {
    let cx = QueryArena::new();
    let before = *cx.alloc(41u32);
    let kept_on = std::thread::spawn(move || {
        let v = cx.alloc(before + 1);
        let s = cx.keep(String::from("kept"));
        (*v, s.clone(), cx)
    })
    .join()
    .unwrap();
    assert_eq!((kept_on.0, kept_on.1.as_str()), (42, "kept"));
    drop(kept_on.2);
}

#[test]
fn the_model_arena_is_shared_by_threads() {
    let model = ModelArena::new();
    let per_thread: Vec<Vec<&(u64, u64)>> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..8u64)
            .map(|t| {
                let model = &model;
                s.spawn(move || {
                    (0..20_000u64)
                        .map(|i| {
                            let text = model.alloc_str(&format!("{t}:{i}"));
                            let pair = model.alloc((t, i));
                            assert_eq!(text, format!("{}:{}", pair.0, pair.1));
                            pair
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap()).collect()
    });
    // every value, made on eight threads at once, read back on this one
    for (t, values) in per_thread.iter().enumerate() {
        for (i, v) in values.iter().enumerate() {
            assert_eq!(**v, (t as u64, i as u64));
        }
    }
    let slice = model.alloc_slice(&[1u8, 2, 3]);
    assert_eq!(slice, &[1, 2, 3]);
    assert!(model.chunks() > 1);
}
