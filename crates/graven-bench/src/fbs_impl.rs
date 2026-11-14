use crate::data::{self, BankTx, Chunk, Frame, Telemetry};
use crate::fbs_gen::bench_fbs as g;
use crate::Sum;
use flatbuffers::{FlatBufferBuilder, WIPOffset};

pub struct Ctx {
    b: FlatBufferBuilder<'static>,
    entries: Vec<WIPOffset<g::Entry<'static>>>,
    entities: Vec<WIPOffset<g::Entity<'static>>>,
}

impl Default for Ctx {
    fn default() -> Self {
        Self::new()
    }
}

impl Ctx {
    pub fn new() -> Self {
        Ctx { b: FlatBufferBuilder::with_capacity(64 * 1024), entries: Vec::with_capacity(1024), entities: Vec::with_capacity(1024) }
    }
}

// ------------------------------------------------------------------ bank

pub fn write_bank<'c>(c: &'c mut Ctx, d: &BankTx) -> &'c [u8] {
    let b = &mut c.b;
    b.reset();
    c.entries.clear();
    // Backward builder: children (strings, entries) must exist before parents.
    for e in &d.entries {
        let memo = e.memo.as_deref().map(|m| b.create_string(m));
        c.entries.push(g::Entry::create(b, &g::EntryArgs { account: e.account, amount: e.amount, memo }));
    }
    let entries = Some(b.create_vector(&c.entries));
    let note = d.note.as_deref().map(|n| b.create_string(n));
    let root = g::Transaction::create(
        b,
        &g::TransactionArgs {
            id: d.id,
            ts: d.ts,
            currency: d.currency,
            from_shard: d.from_shard,
            to_shard: d.to_shard,
            entries,
            note,
        },
    );
    b.finish_minimal(root);
    b.finished_data()
}

fn sum_bank(t: g::Transaction<'_>) -> u64 {
    let mut s = Sum::default();
    s.u(t.id());
    s.u(t.ts());
    s.u(t.currency() as u64);
    s.u(t.from_shard() as u64);
    s.u(t.to_shard() as u64);
    s.s(t.note().map(str::as_bytes));
    if let Some(es) = t.entries() {
        for e in es.iter() {
            s.u(e.account());
            s.u(e.amount() as u64);
            s.s(e.memo().map(str::as_bytes));
        }
    }
    s.0
}

pub fn read_all_bank(buf: &[u8]) -> u64 {
    sum_bank(unsafe { flatbuffers::root_unchecked::<g::Transaction>(buf) })
}

pub fn read_all_bank_verified(buf: &[u8]) -> u64 {
    sum_bank(flatbuffers::root::<g::Transaction>(buf).unwrap())
}

pub fn read_one_bank(buf: &[u8]) -> u64 {
    unsafe { flatbuffers::root_unchecked::<g::Transaction>(buf) }.to_shard() as u64
}

// ----------------------------------------------------------------- frame

pub fn write_frame<'c>(c: &'c mut Ctx, d: &Frame) -> &'c [u8] {
    let b = &mut c.b;
    b.reset();
    c.entities.clear();
    for e in &d.entities {
        let pos = g::Vec3::new(e.pos[0], e.pos[1], e.pos[2]);
        let vel = g::Vec3::new(e.vel[0], e.vel[1], e.vel[2]);
        c.entities.push(g::Entity::create(
            b,
            &g::EntityArgs { id: e.id, pos: Some(&pos), vel: Some(&vel), hp: e.hp, alive: e.alive },
        ));
    }
    let entities = Some(b.create_vector(&c.entities));
    let root = g::Frame::create(b, &g::FrameArgs { tick: d.tick, entities });
    b.finish_minimal(root);
    b.finished_data()
}

fn sum_frame(f: g::Frame<'_>) -> u64 {
    let mut s = Sum::default();
    s.u(f.tick());
    if let Some(es) = f.entities() {
        for e in es.iter() {
            let (p, v) = (e.pos().copied().unwrap_or_default(), e.vel().copied().unwrap_or_default());
            data::sum_entity(&mut s, e.id(), [p.x(), p.y(), p.z()], [v.x(), v.y(), v.z()], e.hp(), e.alive());
        }
    }
    s.0
}

pub fn read_all_frame(buf: &[u8]) -> u64 {
    sum_frame(unsafe { flatbuffers::root_unchecked::<g::Frame>(buf) })
}

pub fn read_all_frame_verified(buf: &[u8]) -> u64 {
    sum_frame(flatbuffers::root::<g::Frame>(buf).unwrap())
}

pub fn read_one_frame(buf: &[u8]) -> u64 {
    let es = unsafe { flatbuffers::root_unchecked::<g::Frame>(buf) }.entities().unwrap();
    es.get(es.len() / 2).hp() as u64
}

// ------------------------------------------------------------- telemetry

pub fn write_telemetry<'c>(c: &'c mut Ctx, d: &Telemetry) -> &'c [u8] {
    let b = &mut c.b;
    b.reset();
    let root = g::Telemetry::create(
        b,
        &g::TelemetryArgs { device: d.device, ts: d.ts, temp: d.temp, humidity: d.humidity, battery: d.battery },
    );
    b.finish_minimal(root);
    b.finished_data()
}

fn sum_telemetry(t: g::Telemetry<'_>) -> u64 {
    let mut s = Sum::default();
    s.u(t.device() as u64);
    s.u(t.ts());
    s.u(t.temp().to_bits() as u64);
    s.u(t.humidity().map_or(u64::MAX, |h| h.to_bits() as u64));
    s.u(t.battery() as u64);
    s.0
}

pub fn read_all_telemetry(buf: &[u8]) -> u64 {
    sum_telemetry(unsafe { flatbuffers::root_unchecked::<g::Telemetry>(buf) })
}

pub fn read_all_telemetry_verified(buf: &[u8]) -> u64 {
    sum_telemetry(flatbuffers::root::<g::Telemetry>(buf).unwrap())
}

pub fn read_one_telemetry(buf: &[u8]) -> u64 {
    unsafe { flatbuffers::root_unchecked::<g::Telemetry>(buf) }.temp().to_bits() as u64
}

// ----------------------------------------------------------------- chunk

pub fn write_chunk<'c>(c: &'c mut Ctx, d: &Chunk) -> &'c [u8] {
    let b = &mut c.b;
    b.reset();
    let data = Some(b.create_vector(&d.data));
    let root = g::Chunk::create(b, &g::ChunkArgs { stream_id: d.stream_id, seq: d.seq, data });
    b.finish_minimal(root);
    b.finished_data()
}

fn sum_chunk(ch: g::Chunk<'_>) -> u64 {
    let mut s = Sum::default();
    s.u(ch.stream_id());
    s.u(ch.seq() as u64);
    s.s(Some(ch.data().map(|v| v.bytes()).unwrap_or(&[])));
    s.0
}

pub fn read_all_chunk(buf: &[u8]) -> u64 {
    sum_chunk(unsafe { flatbuffers::root_unchecked::<g::Chunk>(buf) })
}

pub fn read_all_chunk_verified(buf: &[u8]) -> u64 {
    sum_chunk(flatbuffers::root::<g::Chunk>(buf).unwrap())
}

pub fn read_one_chunk(buf: &[u8]) -> u64 {
    unsafe { flatbuffers::root_unchecked::<g::Chunk>(buf) }.seq() as u64
}
