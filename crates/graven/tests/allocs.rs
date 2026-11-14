//! Zero allocations per message in steady state.
use std::alloc::{GlobalAlloc, Layout, System};
use graven::{Bit, Builder, SimplePool};

struct Counting;
// Per-thread count: the test harness allocates on other threads concurrently.
thread_local! {
    static LOCAL: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
fn bump() {
    let _ = LOCAL.try_with(|c| c.set(c.get() + 1));
}
fn allocs() -> usize {
    LOCAL.with(|c| c.get())
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        bump();
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        bump();
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;



fn build(pool: &SimplePool, i: u64) -> usize {
    let mut b = Builder::new(pool);
    {
        let mut t = b.root(17);
        t.set::<u64>(1, Bit { byte: 0, shift: 0 }, i);
        t.set_str(9, "uma transação qualquer");
        t.set_scalars::<u32>(13, &[1, 2, 3, 4]);
    }
    let msg = b.finish().unwrap();
    msg.len()
}

#[test]
fn steady_state_zero_allocs() {
    let pool = SimplePool::new(4096);
    for i in 0..10 {
        build(&pool, i);
    }
    let before = allocs();
    for i in 0..100 {
        build(&pool, i);
    }
    let after = allocs();
    assert_eq!(after - before, 0, "allocations in steady state");
}

#[test]
fn in_flight_messages_amortize_allocations() {
    // A send queue keeps several messages alive while new ones are built.
    let pool = SimplePool::new(64 * 1024);
    let mut queue = std::collections::VecDeque::with_capacity(8);
    let payload = "y".repeat(300);
    let mut b = Builder::new(&pool);
    b.root(17).set_str(9, "x");
    queue.push_back(b.finish().unwrap());
    let before = allocs();
    for i in 0..1000u64 {
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(17);
            t.set::<u64>(1, Bit { byte: 0, shift: 0 }, i);
            t.set_str(9, &payload);
        }
        queue.push_back(b.finish().unwrap());
        if queue.len() > 4 {
            queue.pop_front();
        }
    }
    let per_msg = (allocs() - before) as f64 / 1000.0;
    assert!(per_msg < 0.05, "allocations per message with 4 in flight: {per_msg}");
}

#[test]
fn batched_frames_zero_allocs() {
    let pool = SimplePool::new(64 * 1024);
    let mut b = Builder::new(&pool);
    let mut run = |n: u64| {
        for i in 0..n {
            for _ in 0..32 {
                let mut t = b.root(17);
                t.set::<u64>(1, Bit { byte: 0, shift: 0 }, i);
                t.set_str(9, "uma transação qualquer");
                b.seal().unwrap();
            }
            let batch = b.take_frames(); // sent, then dropped
            assert_eq!(graven::frames(&batch).count(), 32);
        }
    };
    run(10);
    let before = allocs();
    run(100);
    assert_eq!(allocs() - before, 0, "allocations with batched frames");
}

fn build_enveloped(pool: &SimplePool, i: u64) -> usize {
    let mut b = Builder::new_enveloped(pool);
    {
        let mut t = b.root(17);
        t.set::<u64>(1, Bit { byte: 0, shift: 0 }, i);
        t.set_str(9, "uma transação qualquer");
        t.set_scalars::<u32>(13, &[1, 2, 3, 4]);
    }
    let msg = b.flush_envelope(graven::envelope::schema_id("t.v1.T")).unwrap();
    msg.len()
}

#[test]
fn enveloped_steady_state_zero_allocs() {
    let pool = SimplePool::new(4096);
    for i in 0..10 {
        build_enveloped(&pool, i);
    }
    let before = allocs();
    for i in 0..100 {
        build_enveloped(&pool, i);
    }
    let after = allocs();
    assert_eq!(after - before, 0, "allocations in steady state (enveloped)");
}
