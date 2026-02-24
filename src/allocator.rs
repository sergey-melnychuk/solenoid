//! Wrapper allocator that counts alloc/dealloc/realloc. Call log_stats() periodically
//! to print totals. Per-call logging is not possible (tracing/eprintln allocate → RefCell panic).
//!
//! Usage in your binary (e.g. examples/runner.rs):
//!
//!   use solenoid::alloc_log::LoggingAllocator;
//!   use std::alloc::System;
//!
//!   #[global_allocator]
//!   static GLOBAL: LoggingAllocator<System> = LoggingAllocator(System);
//!
//! Or with mimalloc (cargo run --features alloc-log):
//!
//!   #[global_allocator]
//!   static GLOBAL: LoggingAllocator<mimalloc::MiMalloc> = LoggingAllocator(mimalloc::MiMalloc);

use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct LoggingAllocator<A>(pub A);

static ALLOC: AtomicU64 = AtomicU64::new(0);
static FREED: AtomicU64 = AtomicU64::new(0);
static USED: AtomicU64 = AtomicU64::new(0);

unsafe impl<A: GlobalAlloc> GlobalAlloc for LoggingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.0.alloc(layout) };
        if !ptr.is_null() {
            ALLOC.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        FREED.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { self.0.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { self.0.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            ALLOC.fetch_add(new_size as u64, Ordering::Relaxed);
            FREED.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        new_ptr
    }
}

pub fn stats() -> (u64, i64) {
    let alloc = ALLOC.load(Ordering::Relaxed);
    let freed = FREED.load(Ordering::Relaxed);
    let bytes = alloc - freed;
    let used = USED.load(Ordering::Relaxed);
    USED.store(bytes, Ordering::Relaxed);
    let diff = bytes as i64 - used as i64;
    (bytes, diff)
}
