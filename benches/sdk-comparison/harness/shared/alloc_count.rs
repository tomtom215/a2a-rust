// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Counting global allocator (feature `alloc-count`), identical in both agents.
//! Every `ALLOC_REPORT_MS` it prints `allocs=<n> bytes=<n>` cumulative totals to
//! stderr; allocations per request = Δallocs / Δrequests from the load run.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

pub struct Counting;
pub static ALLOCS: AtomicU64 = AtomicU64::new(0);
pub static BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(l.size() as u64, Relaxed);
        // SAFETY: forwards the caller's layout unchanged to the system allocator,
        // which upholds GlobalAlloc's contract for it.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: `p` was returned by `System.alloc` with this same `l` (every
        // allocation goes through `alloc`/`realloc` above, which use `System`).
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(n as u64, Relaxed);
        // SAFETY: same provenance argument as `dealloc`; `n` is passed through
        // as the caller requested.
        unsafe { System.realloc(p, l, n) }
    }
}

pub fn spawn_reporter() {
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        eprintln!("allocs={} bytes={}", ALLOCS.load(Relaxed), BYTES.load(Relaxed));
    });
}
