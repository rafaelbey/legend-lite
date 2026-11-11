// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The two arenas of the arena ruling (docs/rust-port/PHASE3_DESIGN.md §1.6, PHASE3_SHIM_DESIGN.md §7.3 c and §10,
//! 2026-11-09/10), and the crate's one `unsafe` module.
//!
//! A [`QueryArena`] holds one query's records: Java's `new` is `cx.alloc(record)`, an alias is a copy of the `&'q`
//! reference, and everything goes when the query's handle does. It is `Send` (a handle's next call may run on another
//! worker) and not `Sync` (one call at a time per handle). Its keep pool, [`QueryArena::keep`], owns the values a
//! record points at that own heap memory -- a `Box` SQL tree, an `Arc` source node -- and drops them with the arena.
//!
//! A [`ModelArena`] holds a frozen model's `Type` values: shared by every thread planning on the model, so its bump
//! pointer is behind a mutex; a value is written outside the lock, into space the lock handed out.
//!
//! Both bump through chunks that never move or shrink: a value's address is its identity (`crate::identity`) for as
//! long as the arena lives. A record is `Copy` (so it owns no heap and has no destructor to run), every chunk is
//! aligned to [`CHUNK_ALIGN`] and a value is placed at an offset rounded up to its alignment, so no address is ever
//! read here. A value is at least one byte: two zero-sized records made by two `alloc`s are two objects, as Java's
//! two `new`s of a record with no field are. Every empty slice is the one `&[]`, as Java's `List.of()` is one object.

use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::Mutex;

/// Every chunk's alignment: a value aligned more strictly gets a chunk of its own.
pub const CHUNK_ALIGN: usize = 16;
const FIRST_CHUNK: usize = 4 << 10;
const MAX_CHUNK: usize = 1 << 20;

/// The bump state both arenas share: the chunks, and where the next value goes in the newest.
struct Bump {
    /// Every chunk, as `alloc` returned it, freed when the arena drops.
    chunks: Vec<(*mut u8, Layout)>,
    current: Option<NonNull<u8>>,
    used: usize,
    cap: usize,
    next: usize,
    bytes: usize,
}

// SAFETY: a Bump's pointers are to chunks it alone allocated and alone frees; moving it to another thread moves that
// ownership, and no `&T` into a chunk can outlive the borrow of the arena it came through.
unsafe impl Send for Bump {}

impl Bump {
    const fn new() -> Bump {
        Bump {
            chunks: Vec::new(),
            current: None,
            used: 0,
            cap: 0,
            next: FIRST_CHUNK,
            bytes: 0,
        }
    }

    /// Space for one value of `layout` (at least one byte), aligned, never handed out before.
    fn reserve(&mut self, layout: Layout) -> NonNull<u8> {
        let size = layout.size().max(1);
        let align = layout.align();
        if align <= CHUNK_ALIGN
            && let Some(base) = self.current
        {
            let start = (self.used + align - 1) & !(align - 1);
            if let Some(end) = start.checked_add(size)
                && end <= self.cap
            {
                self.used = end;
                // SAFETY: start + size <= cap, so the offset is inside the current chunk
                return unsafe { base.add(start) };
            }
        }
        if align > CHUNK_ALIGN || size > self.next / 4 {
            // a large or strictly aligned value: a chunk of its own, the current one kept for what follows
            let own = Layout::from_size_align(size, align.max(CHUNK_ALIGN)).expect("arena: a value too large");
            return self.chunk(own);
        }
        let cap = self.next;
        self.next = (cap * 2).min(MAX_CHUNK);
        let base = self.chunk(Layout::from_size_align(cap, CHUNK_ALIGN).expect("arena: chunk size"));
        (self.current, self.used, self.cap) = (Some(base), size, cap);
        base
    }

    fn chunk(&mut self, layout: Layout) -> NonNull<u8> {
        // SAFETY: every layout asked for here has a nonzero size
        let raw = unsafe { alloc(layout) };
        let Some(chunk) = NonNull::new(raw) else {
            handle_alloc_error(layout)
        };
        self.chunks.push((raw, layout));
        self.bytes += layout.size();
        chunk
    }
}

impl Drop for Bump {
    fn drop(&mut self) {
        for &(raw, layout) in &self.chunks {
            // SAFETY: each chunk was allocated by `alloc` with this layout and is freed once, here
            unsafe { dealloc(raw, layout) };
        }
    }
}

/// What both arenas answer: a `Copy` value, a slice or a text, put in the arena and borrowed from it.
pub trait Alloc: sealed::Reserve {
    /// `value`, moved into the arena: Java's `new`.
    fn alloc<T: Copy>(&self, value: T) -> &T {
        let place = self.reserve(Layout::new::<T>()).cast::<T>();
        // SAFETY: `place` is fresh, aligned for T and large enough; it is written before it is read, never written
        // again, and lives as long as the arena, which the returned borrow cannot outlive
        unsafe {
            place.write(value);
            place.as_ref()
        }
    }

    /// The values, copied into the arena as one slice.
    fn alloc_slice<T: Copy>(&self, values: &[T]) -> &[T] {
        self.alloc_from_iter(values.iter().copied())
    }

    /// The text, copied into the arena.
    fn alloc_str(&self, text: &str) -> &str {
        let bytes = self.alloc_slice(text.as_bytes());
        // SAFETY: the bytes are a copy of a str's
        unsafe { std::str::from_utf8_unchecked(bytes) }
    }

    /// An exact-size iterator's values as one slice. The iterator may allocate in this arena as it goes; one that
    /// yields more or fewer values than its `len` is refused with a panic.
    fn alloc_from_iter<T: Copy, I>(&self, values: I) -> &[T]
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: ExactSizeIterator,
    {
        let mut values = values.into_iter();
        let len = values.len();
        if len == 0 {
            assert!(
                values.next().is_none(),
                "arena: an iterator yielded more values than its len"
            );
            return &[];
        }
        let place = self
            .reserve(Layout::array::<T>(len).expect("arena: a slice too large"))
            .cast::<T>();
        for i in 0..len {
            let value = values
                .next()
                .expect("arena: an iterator yielded fewer values than its len");
            // SAFETY: i < len, so the place is inside the space reserved for the slice, which nothing else holds
            unsafe { place.add(i).write(value) };
        }
        assert!(
            values.next().is_none(),
            "arena: an iterator yielded more values than its len"
        );
        // SAFETY: all len values were written above, and the space lives as long as the arena
        unsafe { NonNull::slice_from_raw_parts(place, len).as_ref() }
    }
}

mod sealed {
    /// Reserving space: an arena's own, so that `Alloc`'s writes can trust the space they are handed.
    pub trait Reserve {
        fn reserve(&self, layout: std::alloc::Layout) -> std::ptr::NonNull<u8>;
    }
}

/// One query's arena: `Send`, not `Sync`; its records and its keep pool go with it.
pub struct QueryArena {
    bump: RefCell<Bump>,
    kept: RefCell<Vec<Kept>>,
}

/// A value of the keep pool: a leaked `Box` and the function that frees it.
struct Kept {
    value: *mut (),
    free: unsafe fn(*mut ()),
}

// SAFETY: `keep` admits only `Send` values, so the box a Kept owns may be freed on whichever thread drops the arena
unsafe impl Send for Kept {}

impl Drop for Kept {
    fn drop(&mut self) {
        // SAFETY: `free` is `free_box::<T>` for the T `value` was boxed as, and a Kept is dropped once
        unsafe { (self.free)(self.value) }
    }
}

/// # Safety
/// `value` is a `Box<T>` turned into a raw pointer, not freed before.
unsafe fn free_box<T>(value: *mut ()) {
    // SAFETY: the caller's contract
    drop(unsafe { Box::from_raw(value.cast::<T>()) });
}

impl QueryArena {
    pub const fn new() -> QueryArena {
        QueryArena {
            bump: RefCell::new(Bump::new()),
            kept: RefCell::new(Vec::new()),
        }
    }

    /// `value`, owned by the arena from now on and dropped with it: the keep pool (PHASE3_SHIM_DESIGN.md §7.2), for
    /// what a record points at that owns heap memory, a `Box` SQL tree or an `Arc` source node.
    pub fn keep<T: Send + 'static>(&self, value: T) -> &T {
        let boxed = Box::into_raw(Box::new(value));
        self.kept.borrow_mut().push(Kept {
            value: boxed.cast(),
            free: free_box::<T>,
        });
        // SAFETY: the box is freed only when the arena drops, which the returned borrow cannot outlive; nothing
        // takes a `&mut` to it
        unsafe { &*boxed }
    }

    /// The bytes of the chunks the arena holds (for a shim that counts live arena bytes, §7.6).
    pub fn bytes(&self) -> usize {
        self.bump.borrow().bytes
    }

    /// How many chunks the arena holds.
    pub fn chunks(&self) -> usize {
        self.bump.borrow().chunks.len()
    }

    /// How many values the keep pool holds.
    pub fn kept(&self) -> usize {
        self.kept.borrow().len()
    }
}

impl Default for QueryArena {
    fn default() -> QueryArena {
        QueryArena::new()
    }
}

impl sealed::Reserve for QueryArena {
    fn reserve(&self, layout: Layout) -> NonNull<u8> {
        self.bump.borrow_mut().reserve(layout)
    }
}

impl Alloc for QueryArena {}

/// A frozen model's arena, shared by the threads planning on the model: `Send` and `Sync`.
pub struct ModelArena {
    bump: Mutex<Bump>,
}

impl ModelArena {
    pub const fn new() -> ModelArena {
        ModelArena {
            bump: Mutex::new(Bump::new()),
        }
    }

    /// The bytes of the chunks the arena holds.
    pub fn bytes(&self) -> usize {
        self.lock().bytes
    }

    /// How many chunks the arena holds.
    pub fn chunks(&self) -> usize {
        self.lock().chunks.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Bump> {
        // a panic while the lock was held left the bump state whole: `reserve` panics only before it changes anything
        // (an allocation failure aborts)
        self.bump.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for ModelArena {
    fn default() -> ModelArena {
        ModelArena::new()
    }
}

impl sealed::Reserve for ModelArena {
    fn reserve(&self, layout: Layout) -> NonNull<u8> {
        self.lock().reserve(layout)
    }
}

impl Alloc for ModelArena {}

#[cfg(test)]
mod tests;
