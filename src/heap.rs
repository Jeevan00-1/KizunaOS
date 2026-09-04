// KizunaOS v0.0.6 — free-list allocator. Freed chunks are recycled.
//
// This is the milestone where use-after-free becomes REAL. A bump allocator
// never reuses memory, so it's UAF-immune. This one reuses freed chunks —
// which is exactly the mechanism a use-after-free exploit abuses:
//   alloc A -> addr X ; free A -> X on free list ; alloc B -> X returned again.
// A stale pointer to "A" now aliases "B". That is the bug class, in miniature.
#![allow(unsafe_op_in_unsafe_fn)]

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;

const HEAP_SIZE: usize = 1024 * 1024; // 1 MiB arena
const NUM_BINS: usize = 16;           // size classes: 16,32,48,... bytes

#[repr(align(16))]
struct Arena(UnsafeCell<[u8; HEAP_SIZE]>);
unsafe impl Sync for Arena {}
static ARENA: Arena = Arena(UnsafeCell::new([0u8; HEAP_SIZE]));

/// A node in a free list. When a chunk is free, we reuse its own memory to
/// store the "next free chunk" pointer. Real allocators do exactly this —
/// which is also why corrupting a freed chunk's memory corrupts the free list.
struct FreeNode {
    next: *mut FreeNode,
}

/// Everything mutable lives behind one cell. Single-core kernel, IRQs off in
/// these paths for now, so no locking yet (that arrives with real SMP).
struct HeapState {
    next: usize,                        // bump pointer offset
    bins: [*mut FreeNode; NUM_BINS],    // free list head per size class
    total_allocs: usize,
    total_frees: usize,
    reuses: usize,                      // times we served from a free list
}

pub struct FreeListAllocator {
    state: UnsafeCell<HeapState>,
}
unsafe impl Sync for FreeListAllocator {}

/// Round a size up to its bin. bin i holds chunks of (i+1)*16 bytes.
fn size_to_bin(size: usize) -> Option<usize> {
    let slots = (size + 15) / 16;               // 16-byte granularity
    if slots == 0 || slots > NUM_BINS { None } else { Some(slots - 1) }
}
fn bin_chunk_size(bin: usize) -> usize { (bin + 1) * 16 }

impl FreeListAllocator {
    const fn new() -> Self {
        FreeListAllocator {
            state: UnsafeCell::new(HeapState {
                next: 0,
                bins: [core::ptr::null_mut(); NUM_BINS],
                total_allocs: 0,
                total_frees: 0,
                reuses: 0,
            }),
        }
    }
    fn arena_base(&self) -> usize { ARENA.0.get() as usize }

    // ---- stats accessors for the monitor ----
    pub fn base(&self) -> usize { self.arena_base() }
    pub fn total(&self) -> usize { HEAP_SIZE }
    pub fn used(&self) -> usize { unsafe { (*self.state.get()).next } }
    pub fn allocs(&self) -> usize { unsafe { (*self.state.get()).total_allocs } }
    pub fn frees(&self) -> usize { unsafe { (*self.state.get()).total_frees } }
    pub fn reuses(&self) -> usize { unsafe { (*self.state.get()).reuses } }
    pub fn bin_count(&self, bin: usize) -> usize {
        let s = unsafe { &*self.state.get() };
        let mut n = 0;
        let mut node = s.bins[bin];
        while !node.is_null() { n += 1; node = unsafe { (*node).next }; }
        n
    }
}

unsafe impl GlobalAlloc for FreeListAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let s = &mut *self.state.get();
        let size = layout.size().max(1);

        if let Some(bin) = size_to_bin(size) {
            // 1) Try the free list FIRST — this is the reuse path.
            let head = s.bins[bin];
            if !head.is_null() {
                s.bins[bin] = (*head).next;     // pop it
                s.total_allocs += 1;
                s.reuses += 1;                  // we recycled a freed chunk
                return head as *mut u8;
            }
            // 2) Nothing free — bump a fresh chunk of this bin's fixed size.
            let chunk = bin_chunk_size(bin);
            let base = self.arena_base();
            let aligned = (base + s.next + 15) & !15;   // 16-byte aligned
            let start = aligned - base;
            if start + chunk > HEAP_SIZE { return core::ptr::null_mut(); }
            s.next = start + chunk;
            s.total_allocs += 1;
            return (base + start) as *mut u8;
        }

        // Oversized: fall back to raw bump (won't be recycled).
        let align = layout.align().max(16);
        let base = self.arena_base();
        let aligned = (base + s.next + (align - 1)) & !(align - 1);
        let start = aligned - base;
        if start + size > HEAP_SIZE { return core::ptr::null_mut(); }
        s.next = start + size;
        s.total_allocs += 1;
        (base + start) as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let s = &mut *self.state.get();
        s.total_frees += 1;
        let size = layout.size().max(1);
        if let Some(bin) = size_to_bin(size) {
            // Push the freed chunk onto its bin's free list, storing the
            // "next" pointer INSIDE the freed chunk's own memory.
            let node = ptr as *mut FreeNode;
            (*node).next = s.bins[bin];
            s.bins[bin] = node;
        }
        // oversized frees are leaked for now (bump can't reclaim). fine.
    }
}

#[global_allocator]
pub static ALLOCATOR: FreeListAllocator = FreeListAllocator::new();
