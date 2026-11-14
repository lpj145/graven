//! Bytes per message, per format and scenario (Markdown table).
use graven_bench::{capnp_impl as cp, data, fbs_impl as fb, graven_impl as zi};

fn row(name: &str, z: usize, f: usize, c: usize, cp: usize) {
    println!("| {name} | {z} | {f} | {c} | {cp} |");
}

fn main() {
    let (mut z, mut f, mut c, mut k) = (zi::Ctx::new(), fb::Ctx::new(), cp::Ctx::new(), cp::Ctx::new_packed());
    println!("| scenario | graven | flatbuffers | capnp | capnp packed |");
    println!("|---|---:|---:|---:|---:|");
    for n in [1, 10, 100] {
        let d = data::bank(7, n);
        row(&format!("bank/{n}"), zi::write_bank(&mut z, &d).len(), fb::write_bank(&mut f, &d).len(), cp::write_bank(&mut c, &d).len(), cp::write_bank(&mut k, &d).len());
    }
    for n in [10, 500] {
        let d = data::frame(7, n);
        row(&format!("frame/{n}"), zi::write_frame(&mut z, &d).len(), fb::write_frame(&mut f, &d).len(), cp::write_frame(&mut c, &d).len(), cp::write_frame(&mut k, &d).len());
    }
    for seed in [8, 9] {
        let d = data::telemetry(seed);
        let name = if d.humidity.is_some() { "telemetry (with humidity)" } else { "telemetry (without humidity)" };
        row(name, zi::write_telemetry(&mut z, &d).len(), fb::write_telemetry(&mut f, &d).len(), cp::write_telemetry(&mut c, &d).len(), cp::write_telemetry(&mut k, &d).len());
    }
    for n in [64, 65536] {
        let d = data::chunk(7, n);
        row(&format!("chunk/{n}"), zi::write_chunk(&mut z, &d).len(), fb::write_chunk(&mut f, &d).len(), cp::write_chunk(&mut c, &d).len(), cp::write_chunk(&mut k, &d).len());
    }
}
