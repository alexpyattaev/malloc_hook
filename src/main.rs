use std::time::Duration;

use malloc_hook::*;

#[global_allocator]
static GLOBAL_ALLOC_WRAP: JemWrapAllocator = JemWrapAllocator::new();
pub fn print_allocations() {
    view_allocations(|stats| {
        println!("allocated so far: {:?}", stats);
    });
}

fn main() {
    let mut mps = MemPoolStats::default();
    mps.add("Foo");
    init_allocator(mps);
    let _s = format!("allocating a string!");
    print_allocations();

    let jh = std::thread::Builder::new()
        .name("Foo".to_string())
        .spawn(|| {
            let _s2 = format!("allocating a string!");
            let _s3 = format!("allocating a string!");
            let _s4 = format!("allocating a string!");
            std::thread::sleep(Duration::from_millis(200));
        })
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    print_allocations();
    jh.join().unwrap();
    print_allocations();
    deinit_allocator();
}
