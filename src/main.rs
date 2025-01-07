use {malloc_hook::*, std::time::Duration};

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
    mps.add("Boo");
    init_allocator(mps);
    let _s = format!("allocating a string!");
    print_allocations();

    let jh1 = std::thread::Builder::new()
        .name("Foo thread 1".to_string())
        .spawn(|| {
            let _s2 = format!("allocating a string!");
            let _s3 = format!("allocating a string!");
            let _s4 = format!("allocating a string!");
            let jh2 = std::thread::Builder::new()
                .name("Boo thread 1".to_string())
                .spawn(|| {
                    let _s2 = format!("allocating a string!");
                    let _s3 = format!("allocating a string!");
                    let _s4 = format!("allocating a string!");
                    std::thread::sleep(Duration::from_millis(200));
                })
                .unwrap();
            std::thread::sleep(Duration::from_millis(200));
            jh2.join().unwrap();
        })
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    print_allocations();
    jh1.join().unwrap();
    print_allocations();
    deinit_allocator();
    dbg!(&SYSCALL_CNT);
}
