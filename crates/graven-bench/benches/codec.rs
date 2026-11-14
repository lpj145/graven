use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::Duration;
use graven_bench::{Aligned, capnp_impl as cp, data, fbs_impl as fb, graven_impl as zi};

/// One input of one scenario, pre-encoded in every format.
struct Case {
    name: String,
    graven: Vec<u8>,
    fbs: Vec<u8>,
    capnp: Aligned,
    reads: [fn(&[u8]) -> u64; 3],
    reads_one: [fn(&[u8]) -> u64; 3],
    reads_verified: [fn(&[u8]) -> u64; 3],
}

macro_rules! case {
    ($name:expr, $d:expr, $w:ident, $ra:ident, $ro:ident, $rv:ident) => {{
        let d = $d;
        Case {
            name: $name,
            graven: zi::$w(&mut zi::Ctx::new(), &d).to_vec(),
            fbs: fb::$w(&mut fb::Ctx::new(), &d).to_vec(),
            capnp: Aligned::from(cp::$w(&mut cp::Ctx::new(), &d)),
            reads: [zi::$ra, fb::$ra, cp::$ra],
            reads_one: [zi::$ro, fb::$ro, cp::$ro],
            reads_verified: [zi::$rv, fb::$rv, cp::$rv],
        }
    }};
}

fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    for n in [1, 10, 100] {
        v.push(case!(format!("bank/{n}"), data::bank(7, n), write_bank, read_all_bank, read_one_bank, read_all_bank_verified));
    }
    for n in [10, 500] {
        v.push(case!(format!("frame/{n}"), data::frame(7, n), write_frame, read_all_frame, read_one_frame, read_all_frame_verified));
    }
    v.push(case!(
        "telemetry".to_string(),
        data::telemetry(8),
        write_telemetry,
        read_all_telemetry,
        read_one_telemetry,
        read_all_telemetry_verified
    ));
    for n in [64, 65536] {
        v.push(case!(format!("chunk/{n}"), data::chunk(7, n), write_chunk, read_all_chunk, read_one_chunk, read_all_chunk_verified));
    }
    v
}

fn bench_write(c: &mut Criterion) {
    let mut g = c.benchmark_group("write");
    let (mut z, mut f, mut p) = (zi::Ctx::new(), fb::Ctx::new(), cp::Ctx::new());
    for n in [1usize, 10, 100] {
        let d = data::bank(7, n);
        let id = format!("bank/{n}");
        g.bench_function(BenchmarkId::new("graven", &id), |b| b.iter(|| black_box(zi::write_bank(&mut z, black_box(&d))).len()));
        g.bench_function(BenchmarkId::new("graven_known", &id), |b| {
            b.iter(|| black_box(zi::write_bank_known(&mut z, black_box(&d))).len())
        });
        g.bench_function(BenchmarkId::new("fbs", &id), |b| b.iter(|| black_box(fb::write_bank(&mut f, black_box(&d))).len()));
        g.bench_function(BenchmarkId::new("capnp", &id), |b| b.iter(|| black_box(cp::write_bank(&mut p, black_box(&d))).len()));
    }
    for n in [10usize, 500] {
        let d = data::frame(7, n);
        let id = format!("frame/{n}");
        g.bench_function(BenchmarkId::new("graven", &id), |b| b.iter(|| black_box(zi::write_frame(&mut z, black_box(&d))).len()));
        g.bench_function(BenchmarkId::new("fbs", &id), |b| b.iter(|| black_box(fb::write_frame(&mut f, black_box(&d))).len()));
        g.bench_function(BenchmarkId::new("capnp", &id), |b| b.iter(|| black_box(cp::write_frame(&mut p, black_box(&d))).len()));
    }
    // Owned-`Bytes` variant (flush): cost of handing the message off by value.
    let mut zo = zi::Ctx::new_owned();
    let d = data::chunk(7, 64);
    g.bench_function(BenchmarkId::new("graven_owned", "chunk/64"), |b| b.iter(|| black_box(zi::write_chunk(&mut zo, black_box(&d))).len()));
    let mut zb = zi::Ctx::new_batched();
    g.bench_function(BenchmarkId::new("graven_batched", "chunk/64"), |b| b.iter(|| black_box(zi::write_chunk(&mut zb, black_box(&d))).len()));
    let dt = data::telemetry(8);
    g.bench_function(BenchmarkId::new("graven_owned", "telemetry"), |b| b.iter(|| black_box(zi::write_telemetry(&mut zo, black_box(&dt))).len()));
    g.bench_function(BenchmarkId::new("graven_batched", "telemetry"), |b| b.iter(|| black_box(zi::write_telemetry(&mut zb, black_box(&dt))).len()));
    let d = data::frame(7, 500);
    g.bench_function(BenchmarkId::new("graven_batched", "frame/500"), |b| b.iter(|| black_box(zi::write_frame(&mut zb, black_box(&d))).len()));
    let d = data::frame(7, 500);
    g.bench_function(BenchmarkId::new("graven_owned", "frame/500"), |b| b.iter(|| black_box(zi::write_frame(&mut zo, black_box(&d))).len()));
    let d = data::telemetry(8);
    g.bench_function(BenchmarkId::new("graven", "telemetry"), |b| b.iter(|| black_box(zi::write_telemetry(&mut z, black_box(&d))).len()));
    g.bench_function(BenchmarkId::new("fbs", "telemetry"), |b| b.iter(|| black_box(fb::write_telemetry(&mut f, black_box(&d))).len()));
    g.bench_function(BenchmarkId::new("capnp", "telemetry"), |b| b.iter(|| black_box(cp::write_telemetry(&mut p, black_box(&d))).len()));
    for n in [64usize, 65536] {
        let d = data::chunk(7, n);
        let id = format!("chunk/{n}");
        g.bench_function(BenchmarkId::new("graven", &id), |b| b.iter(|| black_box(zi::write_chunk(&mut z, black_box(&d))).len()));
        g.bench_function(BenchmarkId::new("fbs", &id), |b| b.iter(|| black_box(fb::write_chunk(&mut f, black_box(&d))).len()));
        g.bench_function(BenchmarkId::new("capnp", &id), |b| b.iter(|| black_box(cp::write_chunk(&mut p, black_box(&d))).len()));
    }
    g.finish();
}

fn bench_reads(c: &mut Criterion) {
    let cases = cases();
    for (group, pick) in [("read_all", 0usize), ("read_one", 1), ("read_all_verified", 2)] {
        let mut g = c.benchmark_group(group);
        for cs in &cases {
            let fns = match pick {
                0 => cs.reads,
                1 => cs.reads_one,
                _ => cs.reads_verified,
            };
            let bufs: [&[u8]; 3] = [&cs.graven, &cs.fbs, cs.capnp.bytes()];
            for (i, fmt) in ["graven", "fbs", "capnp"].iter().enumerate() {
                let (f, buf) = (fns[i], bufs[i]);
                g.bench_function(BenchmarkId::new(*fmt, &cs.name), |b| b.iter(|| f(black_box(buf))));
            }
            if pick == 0 && cs.name.starts_with("bank") {
                // View built once, outside the timed loop: measures only the trusted read.
                let v = graven_schemas::bank::Transaction::verified(&cs.graven).unwrap();
                g.bench_function(BenchmarkId::new("graven_trusted", &cs.name), |b| {
                    b.iter(|| zi::read_all_bank_trusted(black_box(&v)))
                });
            }
        }
        g.finish();
    }
}

/// Cost of the envelope (header + CRC32C + schema id check) against the bare message.
/// `owned` = `flush()` (one `Bytes` per message), `enveloped` = `flush_envelope()`.
/// Reads: `verified` = `Type::verified(payload)`, `from_bytes` = `Type::from_bytes(envelope)`.
fn bench_envelope(c: &mut Criterion) {
    let mut g = c.benchmark_group("envelope");
    let (mut zo, mut ze) = (zi::Ctx::new_owned(), zi::EnvCtx::new());

    let d = data::telemetry(8);
    g.bench_function(BenchmarkId::new("write_owned", "telemetry"), |b| b.iter(|| black_box(zi::write_telemetry(&mut zo, black_box(&d))).len()));
    g.bench_function(BenchmarkId::new("write_enveloped", "telemetry"), |b| {
        b.iter(|| black_box(zi::write_telemetry_enveloped(&mut ze, black_box(&d))).len())
    });
    let bare = zi::write_telemetry(&mut zo, &d).to_vec();
    let env = zi::write_telemetry_enveloped(&mut ze, &d).to_vec();
    g.bench_function(BenchmarkId::new("read_verified_one", "telemetry"), |b| b.iter(|| zi::verified_one_telemetry(black_box(&bare))));
    g.bench_function(BenchmarkId::new("read_from_bytes_one", "telemetry"), |b| b.iter(|| zi::from_bytes_one_telemetry(black_box(&env))));

    for n in [64usize, 65536] {
        let d = data::chunk(7, n);
        let id = format!("chunk/{n}");
        g.bench_function(BenchmarkId::new("write_owned", &id), |b| b.iter(|| black_box(zi::write_chunk(&mut zo, black_box(&d))).len()));
        g.bench_function(BenchmarkId::new("write_enveloped", &id), |b| {
            b.iter(|| black_box(zi::write_chunk_enveloped(&mut ze, black_box(&d))).len())
        });
        let bare = zi::write_chunk(&mut zo, &d).to_vec();
        let env = zi::write_chunk_enveloped(&mut ze, &d).to_vec();
        g.bench_function(BenchmarkId::new("read_verified_one", &id), |b| b.iter(|| zi::verified_one_chunk(black_box(&bare))));
        g.bench_function(BenchmarkId::new("read_from_bytes_one", &id), |b| b.iter(|| zi::from_bytes_one_chunk(black_box(&env))));
    }

    let d = data::bank(7, 10);
    g.bench_function(BenchmarkId::new("write_owned", "bank/10"), |b| b.iter(|| black_box(zi::write_bank_known(&mut zo, black_box(&d))).len()));
    g.bench_function(BenchmarkId::new("write_enveloped", "bank/10"), |b| {
        b.iter(|| black_box(zi::write_bank_enveloped(&mut ze, black_box(&d))).len())
    });
    let bare = zi::write_bank_known(&mut zo, &d).to_vec();
    let env = zi::write_bank_enveloped(&mut ze, &d).to_vec();
    g.bench_function(BenchmarkId::new("read_verified_one", "bank/10"), |b| b.iter(|| zi::verified_one_bank(black_box(&bare))));
    g.bench_function(BenchmarkId::new("read_from_bytes_one", "bank/10"), |b| b.iter(|| zi::from_bytes_one_bank(black_box(&env))));
    g.bench_function(BenchmarkId::new("read_verified_all", "bank/10"), |b| b.iter(|| zi::read_all_bank_verified(black_box(&bare))));
    g.bench_function(BenchmarkId::new("read_from_bytes_all", "bank/10"), |b| b.iter(|| zi::from_bytes_all_bank(black_box(&env))));
    g.finish();

    // CRC32C throughput (hardware-accelerated path used by the envelope).
    let mut g = c.benchmark_group("crc32c");
    for n in [64usize, 1024, 65536] {
        let buf: Vec<u8> = (0..n).map(|i| (i * 31 + 7) as u8).collect();
        g.throughput(criterion::Throughput::Bytes(n as u64));
        g.bench_function(BenchmarkId::new("hw", n), |b| b.iter(|| graven::crc32c(black_box(&buf))));
        g.bench_function(BenchmarkId::new("sw", n), |b| b.iter(|| graven::envelope::crc32c_sw(black_box(&buf))));
    }
    g.finish();
}

fn config() -> Criterion {
    Criterion::default().warm_up_time(Duration::from_millis(500)).measurement_time(Duration::from_secs(2)).sample_size(40)
}

criterion_group! { name = benches; config = config(); targets = bench_write, bench_reads, bench_envelope }
criterion_main!(benches);
