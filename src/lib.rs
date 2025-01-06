use jemallocator::Jemalloc;

//#[global_allocator]
//static GLOBAL: Jemalloc = Jemalloc;
//use tikv_jemalloc_sys::extent_hooks_s;

/*#[repr(C)]
pub struct extent_hooks_s {
    pub alloc: Option<extent_alloc_t>,
    pub dalloc: Option<extent_dalloc_t>,
    pub destroy: Option<extent_destroy_t>,
    pub commit: Option<extent_commit_t>,
    pub decommit: Option<extent_decommit_t>,
    pub purge_lazy: Option<extent_purge_t>,
    pub purge_forced: Option<extent_purge_t>,
    pub split: Option<extent_split_t>,
    pub merge: Option<extent_merge_t>,
}*/

/*fn sethook() {
    extent_hooks_s
}*/
use std::alloc::{GlobalAlloc, Layout};

use std::sync::atomic::AtomicIsize;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    RwLock,
};

static SELF: JemWrapStats = JemWrapStats {
    named_thread_stats: RwLock::new(None),
    unnamed_thread_stats: Counters::new(),
    process_stats: Counters::new(),
};

#[derive(Debug)]
pub struct Counters {
    allocations_balance: AtomicIsize,
    allocations_total: AtomicUsize,
    deallocations_total: AtomicUsize,
    bytes_balance: AtomicIsize,
    bytes_allocated_total: AtomicUsize,
    bytes_deallocated_total: AtomicUsize,
}

pub struct CountersView {
    pub allocations_balance: isize,
    pub allocations_total: usize,
    pub deallocations_total: usize,
    pub bytes_balance: isize,
    pub bytes_allocated_total: usize,
    pub bytes_deallocated_total: usize,
}

impl Default for Counters {
    fn default() -> Self {
        Self::new()
    }
}

impl Counters {
    pub fn view(&self) -> CountersView {
        CountersView {
            allocations_balance: self.allocations_balance.load(Ordering::Relaxed),
            allocations_total: self.allocations_total.load(Ordering::Relaxed),
            deallocations_total: self.deallocations_total.load(Ordering::Relaxed),
            bytes_balance: self.bytes_balance.load(Ordering::Relaxed),
            bytes_allocated_total: self.bytes_allocated_total.load(Ordering::Relaxed),
            bytes_deallocated_total: self.bytes_deallocated_total.load(Ordering::Relaxed),
        }
    }

    const fn new() -> Self {
        Self {
            allocations_balance: AtomicIsize::new(0),
            allocations_total: AtomicUsize::new(0),
            deallocations_total: AtomicUsize::new(0),
            bytes_balance: AtomicIsize::new(0),
            bytes_allocated_total: AtomicUsize::new(0),
            bytes_deallocated_total: AtomicUsize::new(0),
        }
    }
}

impl Counters {
    pub fn alloc(&self, size: usize) {
        self.bytes_allocated_total
            .fetch_add(size, Ordering::Relaxed);
        self.allocations_total.fetch_add(1, Ordering::Relaxed);

        self.bytes_balance
            .fetch_add(size as isize, Ordering::Relaxed);
        self.allocations_balance.fetch_add(1, Ordering::Relaxed);
    }
    pub fn dealloc(&self, size: usize) {
        self.bytes_deallocated_total
            .fetch_add(size, Ordering::Relaxed);
        self.deallocations_total.fetch_add(1, Ordering::Relaxed);

        self.bytes_balance
            .fetch_sub(size as isize, Ordering::Relaxed);
        self.allocations_balance.fetch_sub(1, Ordering::Relaxed);
    }
}

#[repr(C, align(4096))] // 4096 == MAX_SUPPORTED_ALIGN
pub struct JemWrapAllocator {
    jemalloc: Jemalloc,
}
impl JemWrapAllocator {
    pub const fn new() -> Self {
        Self { jemalloc: Jemalloc }
    }
}

struct JemWrapStats {
    pub named_thread_stats: RwLock<Option<MemPoolStats>>,
    pub unnamed_thread_stats: Counters,
    pub process_stats: Counters,
}

pub fn view_allocations(f: impl FnOnce(&MemPoolStats)) {
    let lock_guard = &SELF.named_thread_stats.read().unwrap();
    if let Some(stats) = lock_guard.as_ref() {
        f(stats);
    }
}

#[derive(Debug, Default)]
pub struct MemPoolStats {
    pub data: Vec<(String, Counters)>,
}

impl MemPoolStats {
    pub fn add(&mut self, prefix: String) {
        self.data.push((prefix, Counters::default()));
    }
}

pub fn init_allocator(mps: MemPoolStats) {
    SELF.named_thread_stats.write().unwrap().replace(mps);
}

pub fn deinit_allocator() -> MemPoolStats {
    SELF.named_thread_stats.write().unwrap().take().unwrap()
}

unsafe impl Sync for JemWrapAllocator {}

fn match_thread_name_safely(stats: &MemPoolStats) -> Option<&Counters> {
    let mut name_buf = [0u8; 64];
    let name: &str;
    unsafe {
        let res = libc::pthread_getname_np(
            libc::pthread_self(),
            (&mut name_buf).as_mut_ptr() as *mut i8,
            name_buf.len(),
        );
        if res != 0 {
            return None;
        }
        let name_len = memchr::memchr(0, &name_buf).unwrap_or(name_buf.len());
        name = std::str::from_utf8_unchecked(&name_buf[0..name_len]);
    }

    for (prefix, stats) in stats.data.iter() {
        if !name.starts_with(prefix) {
            continue;
        }
        return Some(stats);
    }
    return None;
}

unsafe impl GlobalAlloc for JemWrapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let alloc = self.jemalloc.alloc(layout);
        if alloc.is_null() {
            return alloc;
        }
        SELF.process_stats.alloc(layout.size());
        if let Ok(stats) = SELF.named_thread_stats.try_read() {
            if let Some(stats) = stats.as_ref() {
                if let Some(stats) = match_thread_name_safely(stats) {
                    stats.alloc(layout.size());
                    return alloc;
                }
            }
        }
        SELF.unnamed_thread_stats.alloc(layout.size());
        alloc
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.jemalloc.dealloc(ptr, layout);
        if ptr.is_null() {
            return;
        }
        SELF.process_stats.dealloc(layout.size());
        if let Ok(stats) = SELF.named_thread_stats.try_read() {
            if let Some(stats) = stats.as_ref() {
                if let Some(stats) = match_thread_name_safely(stats) {
                    stats.dealloc(layout.size());
                    return;
                }
            }
        }
        SELF.unnamed_thread_stats.dealloc(layout.size());
    }
}
