//! A counting global allocator (BUILD-028): it wraps the system allocator and counts every
//! allocation, the bytes asked for, the bytes alive and their peak, so a scenario can say how
//! much memory a frame or a scan costs. Only in the benchmark build; the numbers include the
//! (small) cost of the counting itself.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

pub struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn added(n: usize) {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(n as u64, Relaxed);
    let live = LIVE.fetch_add(n, Relaxed).wrapping_add(n);
    PEAK.fetch_max(live, Relaxed);
}

fn removed(n: usize) {
    LIVE.fetch_sub(n, Relaxed);
}

// SAFETY: every call is passed on unchanged to the system allocator, which upholds the contract.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's layout, passed on as it is
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            added(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's layout, passed on as it is
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            added(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` come from an earlier call of this allocator, which got them from `System`
        unsafe { System.dealloc(ptr, layout) };
        removed(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as in `dealloc`; `new_size` is the caller's
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            removed(layout.size());
            added(new_size);
        }
        p
    }
}

/// The counters at one moment.
#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    /// allocations so far (a `realloc` counts as one)
    pub allocs: u64,
    /// bytes asked for so far
    pub bytes: u64,
    /// bytes alive now
    pub live: usize,
    /// the most bytes alive since the last `reset_peak`
    pub peak: usize,
}

pub fn snapshot() -> Snapshot {
    Snapshot { allocs: ALLOCS.load(Relaxed), bytes: BYTES.load(Relaxed), live: LIVE.load(Relaxed), peak: PEAK.load(Relaxed) }
}

/// Start a new peak: from what is alive now.
pub fn reset_peak() {
    PEAK.store(LIVE.load(Relaxed), Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: BUILD-028
    #[test]
    fn allocations_bytes_and_the_peak_are_counted() {
        let before = snapshot();
        reset_peak();
        let v: Vec<u8> = Vec::with_capacity(1 << 20);
        let during = snapshot();
        assert!(during.allocs > before.allocs, "the allocation is counted");
        assert!(during.bytes >= before.bytes + (1 << 20), "its bytes are counted");
        assert!(during.live >= (1 << 20), "it is alive");
        assert!(during.peak >= (1 << 20));
        std::hint::black_box(&v);
        drop(v);
        let after = snapshot();
        assert!(after.peak >= (1 << 20), "the peak stays after the memory is freed");
        assert!(after.bytes >= during.bytes, "the bytes asked for only grow");
    }
}
