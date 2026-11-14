//! Heap allocations per written message after warm-up.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use graven_bench::{capnp_impl as cp, data, fbs_impl as fb, graven_impl as zi};

struct Counting;
static N: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        N.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        N.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn per_msg(mut f: impl FnMut() -> usize) -> f64 {
    for _ in 0..100 {
        f();
    }
    let before = N.load(Ordering::SeqCst);
    for _ in 0..1000 {
        std::hint::black_box(f());
    }
    (N.load(Ordering::SeqCst) - before) as f64 / 1000.0
}

fn main() {
    let (mut z, mut f, mut c) = (zi::Ctx::new(), fb::Ctx::new(), cp::Ctx::new());
    println!("| scenario | graven (streaming) | graven (known) | flatbuffers | capnp |");
    println!("|---|---:|---:|---:|---:|");
    let d = data::bank(7, 100);
    println!(
        "| bank/100 | {} | {} | {} | {} |",
        per_msg(|| zi::write_bank(&mut z, &d).len()),
        per_msg(|| zi::write_bank_known(&mut z, &d).len()),
        per_msg(|| fb::write_bank(&mut f, &d).len()),
        per_msg(|| cp::write_bank(&mut c, &d).len())
    );
    let d = data::frame(7, 500);
    let zz = per_msg(|| zi::write_frame(&mut z, &d).len());
    println!("| frame/500 | {zz} | {zz} | {} | {} |", per_msg(|| fb::write_frame(&mut f, &d).len()), per_msg(|| cp::write_frame(&mut c, &d).len()));
    let d = data::telemetry(8);
    let zz = per_msg(|| zi::write_telemetry(&mut z, &d).len());
    println!(
        "| telemetry | {zz} | {zz} | {} | {} |",
        per_msg(|| fb::write_telemetry(&mut f, &d).len()),
        per_msg(|| cp::write_telemetry(&mut c, &d).len())
    );
    let d = data::chunk(7, 65536);
    let zz = per_msg(|| zi::write_chunk(&mut z, &d).len());
    println!("| chunk/65536 | {zz} | {zz} | {} | {} |", per_msg(|| fb::write_chunk(&mut f, &d).len()), per_msg(|| cp::write_chunk(&mut c, &d).len()));
}
