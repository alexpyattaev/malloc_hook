use crossbeam::atomic::AtomicConsume;
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

use std::cell::{Cell, OnceCell};
use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, RandomState};
use std::process::id;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::RwLock;
use std::thread::ThreadId;

const MAX_SUPPORTED_ALIGN: usize = 4096;

#[derive(Default, Debug)]
struct Counters {
    allocations: AtomicUsize,
    bytes: AtomicUsize,
    junk: [usize; 32],
    junk2: [usize; 64 - 32 - 8 - 8],
}

#[repr(C, align(4096))] // 4096 == MAX_SUPPORTED_ALIGN
struct JemWrapAllocator {
    jemalloc: Jemalloc,
    mapping: OnceCell<Vec<(ThreadId, usize)>>,
    stats: OnceCell<Vec<Counters>>,
    enable_maps: AtomicBool,
}

pub struct MemPoolStats {
    stats: BTreeMap<String, Counters>,
}

#[global_allocator]
static ALLOCATOR: JemWrapAllocator = JemWrapAllocator {
    jemalloc: Jemalloc,
    mapping: OnceCell::new(),
    stats: OnceCell::new(),
    enable_maps: AtomicBool::new(false),
};

pub fn init_mappings(mappings_to_set: Vec<Vec<ThreadId>>) {
    let mut mapping = Vec::with_capacity(512);
    let mut stats = Vec::with_capacity(128);
    for group in mappings_to_set {
        stats.push(Counters::default());
        for thread in group {
            mapping.push((thread, stats.len() - 1));
        }
    }
    ALLOCATOR.stats.set(stats).unwrap();
    ALLOCATOR.mapping.set(mapping).unwrap();
    ALLOCATOR.enable_maps.store(true, Ordering::Relaxed);
}
pub fn deinit_allocator() {
    ALLOCATOR.enable_maps.store(false, Ordering::SeqCst);
}

unsafe impl Sync for JemWrapAllocator {}

unsafe impl GlobalAlloc for JemWrapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let alloc = self.jemalloc.alloc(layout);
        if alloc.is_null() {
            return alloc;
        }
        let mapping = if let Some(s) = self.mapping.get() {
            s
        } else {
            return alloc;
        };
        let thread = std::thread::current();
        let tid = thread.id();

        for (t, idx) in mapping.iter() {
            if !(*t == tid) {
                continue;
            }
            let stats = self.stats.get().unwrap();
            let c = stats.get(*idx).unwrap();
            c.bytes.fetch_add(layout.size(), Ordering::Relaxed);
            c.allocations.fetch_add(1, Ordering::Relaxed);
        }
        alloc
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.jemalloc.dealloc(ptr, layout);
        if ptr.is_null() {
            return;
        }
        if !self.enable_maps.load_consume() {
            return;
        }
        let mapping = if let Some(s) = self.mapping.get() {
            s
        } else {
            return;
        };
        if mapping.is_empty() {
            return;
        }
        let thread = std::thread::current();
        let tid = thread.id();

        for (t, idx) in mapping.iter() {
            if !(*t == tid) {
                continue;
            }
            let stats = self.stats.get().unwrap();
            let c = stats.get(*idx).unwrap();
            c.bytes.fetch_sub(layout.size(), Ordering::Relaxed);
            c.allocations.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

fn main() {
    let cur = std::thread::current();
    init_mappings(vec![vec![cur.id()]]);
    let _s = format!("allocating a string!");
    let currently = ALLOCATOR.stats.get().unwrap();
    println!("allocated so far: {:?}", currently);
    let _s2 = format!("allocating a string!");
    let _s3 = format!("allocating a string!");
    let _s4 = format!("allocating a string!");
    let currently = ALLOCATOR.stats.get().unwrap();
    println!("allocated so far: {:?}", currently);
    deinit_allocator();
}
