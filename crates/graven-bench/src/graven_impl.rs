use crate::data::{self, BankTx, Chunk, Frame, Telemetry};
use crate::Sum;
use bytes::Bytes;
use graven::{Builder, SimplePool, VerifyCtx};
use graven_schemas::bank::{Currency, Transaction, TransactionVerified, TransactionW};
use graven_schemas::game::{self, FrameW, Vec3};
use graven_schemas::iot::{self, TelemetryW};
use graven_schemas::stream::{self, ChunkW};

/// Reused builder, same contract as the other formats' contexts: the
/// returned `&[u8]` borrows the builder (like FlatBuffers `finished_data()`)
/// and is valid until the next write. `owned` switches to `flush()`, which
/// hands out an owned `Bytes` (extra atomic refcount per message).
pub struct Ctx {
    builder: Builder<'static>,
    last: Option<Bytes>,
    owned: bool,
    /// Batched mode: seal each message as a frame; every `BATCH` frames or
    /// `BATCH_BYTES` bytes take one `Bytes` for the whole batch (as a sender
    /// would) and drop it.
    batch: usize,
    pending: usize,
}

pub const BATCH: usize = 64;
/// Ship a batch once it reaches this many bytes (half a pool chunk).
pub const BATCH_BYTES: usize = 32 * 1024;

impl Default for Ctx {
    fn default() -> Self {
        Self::new()
    }
}

impl Ctx {
    pub fn new() -> Self {
        let pool: &'static SimplePool = Box::leak(Box::new(SimplePool::new(64 * 1024)));
        Ctx { builder: Builder::new(pool), last: None, owned: false, batch: 0, pending: 0 }
    }
    pub fn new_owned() -> Self {
        Ctx { owned: true, ..Self::new() }
    }
    pub fn new_batched() -> Self {
        Ctx { batch: BATCH, ..Self::new() }
    }
    #[inline]
    fn build(&mut self, f: impl FnOnce(&mut Builder<'static>)) -> &[u8] {
        if self.batch > 0 {
            // Seal the previous message; ship the batch when full.
            self.builder.seal().expect("message too large");
            self.pending += 1;
            if self.pending == self.batch || self.builder.sealed_len() >= BATCH_BYTES {
                drop(self.builder.take_frames());
                self.pending = 0;
            }
        }
        f(&mut self.builder);
        if self.owned {
            self.last = None; // previous message sent and dropped
            self.last = Some(self.builder.flush().expect("message too large"));
            self.last.as_deref().unwrap()
        } else {
            self.builder.finished().expect("message too large")
        }
    }
}

fn v3(a: [f32; 3]) -> Vec3 {
    Vec3 { x: a[0], y: a[1], z: a[2] }
}

// ------------------------------------------------------------------ bank

fn bank_header(t: &mut TransactionW<'_>, d: &BankTx) {
    t.id(d.id).ts(d.ts).currency(Currency::from_raw(d.currency)).from_shard(d.from_shard).to_shard(d.to_shard);
    if let Some(n) = &d.note {
        t.note(n);
    }
}

/// Entries pushed with unknown count (streaming list) — the hard path.
pub fn write_bank<'c>(c: &'c mut Ctx, d: &BankTx) -> &'c [u8] {
    c.build(|b| {
        let mut t = TransactionW::root(b);
        bank_header(&mut t, d);
        let mut l = t.entries_streaming();
        for e in &d.entries {
            let mut w = l.push();
            w.account(e.account).amount(e.amount);
            if let Some(m) = &e.memo {
                w.memo(m);
            }
        }
    })
}

/// Entries with known count.
pub fn write_bank_known<'c>(c: &'c mut Ctx, d: &BankTx) -> &'c [u8] {
    c.build(|b| {
        let mut t = TransactionW::root(b);
        bank_header(&mut t, d);
        let mut l = t.entries(d.entries.len() as u32);
        for e in &d.entries {
            let mut w = l.push();
            w.account(e.account).amount(e.amount);
            if let Some(m) = &e.memo {
                w.memo(m);
            }
        }
    })
}

pub fn read_all_bank(buf: &[u8]) -> u64 {
    let t = Transaction::read(buf).unwrap();
    let mut s = Sum::default();
    s.u(t.id());
    s.u(t.ts());
    s.u(t.currency().to_raw() as u64);
    s.u(t.from_shard() as u64);
    s.u(t.to_shard() as u64);
    s.s(t.note().unwrap().map(str::as_bytes));
    for e in t.entries().unwrap().iter() {
        let e = e.unwrap();
        s.u(e.account());
        s.u(e.amount() as u64);
        s.s(e.memo().unwrap().map(str::as_bytes));
    }
    s.0
}

/// Read through a verified view: strings skip UTF-8 validation (already done by
/// `verify()`) — the mode comparable to FlatBuffers `root_unchecked`. The caller builds
/// the view once, so this measures only the trusted read.
pub fn read_all_bank_trusted(t: &TransactionVerified<'_>) -> u64 {
    let mut s = Sum::default();
    s.u(t.id());
    s.u(t.ts());
    s.u(t.currency().to_raw() as u64);
    s.u(t.from_shard() as u64);
    s.u(t.to_shard() as u64);
    s.s(t.note().unwrap().map(str::as_bytes));
    for e in t.entries().unwrap().iter() {
        let e = e.unwrap();
        s.u(e.account());
        s.u(e.amount() as u64);
        s.s(e.memo().unwrap().map(str::as_bytes));
    }
    s.0
}

/// `verify()` validates every string's UTF-8 once, so the read after it can
/// skip validation (the intended usage for untrusted input).
pub fn read_all_bank_verified(buf: &[u8]) -> u64 {
    read_all_bank_trusted(&Transaction::verified(buf).unwrap())
}

pub fn read_one_bank(buf: &[u8]) -> u64 {
    Transaction::read(buf).unwrap().to_shard() as u64
}

// ----------------------------------------------------------------- frame

pub fn write_frame<'c>(c: &'c mut Ctx, d: &Frame) -> &'c [u8] {
    c.build(|b| {
        let mut f = FrameW::root(b);
        f.tick(d.tick);
        let mut l = f.entities(d.entities.len() as u32);
        for e in &d.entities {
            l.push().id(e.id).pos(v3(e.pos)).vel(v3(e.vel)).hp(e.hp).alive(e.alive);
        }
    })
}

pub fn read_all_frame(buf: &[u8]) -> u64 {
    let f = game::Frame::read(buf).unwrap();
    let mut s = Sum::default();
    s.u(f.tick());
    for e in f.entities().unwrap().iter() {
        let e = e.unwrap();
        let (p, v) = (e.pos(), e.vel());
        data::sum_entity(&mut s, e.id(), [p.x, p.y, p.z], [v.x, v.y, v.z], e.hp(), e.alive());
    }
    s.0
}

pub fn read_all_frame_verified(buf: &[u8]) -> u64 {
    game::Frame::read(buf).unwrap().verify(&mut VerifyCtx::new(buf.len())).unwrap();
    read_all_frame(buf)
}

pub fn read_one_frame(buf: &[u8]) -> u64 {
    let l = game::Frame::read(buf).unwrap().entities().unwrap();
    l.get(l.len() / 2).unwrap().hp() as u64
}

// ------------------------------------------------------------- telemetry

pub fn write_telemetry<'c>(c: &'c mut Ctx, d: &Telemetry) -> &'c [u8] {
    c.build(|b| {
        let mut t = TelemetryW::root(b);
        t.device(d.device).ts(d.ts).temp(d.temp).battery(d.battery);
        if let Some(h) = d.humidity {
            t.humidity(h);
        }
    })
}

pub fn read_all_telemetry(buf: &[u8]) -> u64 {
    let t = iot::Telemetry::read(buf).unwrap();
    let mut s = Sum::default();
    s.u(t.device() as u64);
    s.u(t.ts());
    s.u(t.temp().to_bits() as u64);
    s.u(t.humidity().map_or(u64::MAX, |h| h.to_bits() as u64));
    s.u(t.battery() as u64);
    s.0
}

pub fn read_all_telemetry_verified(buf: &[u8]) -> u64 {
    iot::Telemetry::read(buf).unwrap().verify(&mut VerifyCtx::new(buf.len())).unwrap();
    read_all_telemetry(buf)
}

pub fn read_one_telemetry(buf: &[u8]) -> u64 {
    iot::Telemetry::read(buf).unwrap().temp().to_bits() as u64
}

// ----------------------------------------------------------------- chunk

pub fn write_chunk<'c>(c: &'c mut Ctx, d: &Chunk) -> &'c [u8] {
    c.build(|b| {
        ChunkW::root(b).stream_id(d.stream_id).seq(d.seq).data(&d.data);
    })
}

pub fn read_all_chunk(buf: &[u8]) -> u64 {
    let c = stream::Chunk::read(buf).unwrap();
    let mut s = Sum::default();
    s.u(c.stream_id());
    s.u(c.seq() as u64);
    s.s(Some(c.data().unwrap()));
    s.0
}

pub fn read_all_chunk_verified(buf: &[u8]) -> u64 {
    stream::Chunk::read(buf).unwrap().verify(&mut VerifyCtx::new(buf.len())).unwrap();
    read_all_chunk(buf)
}

pub fn read_one_chunk(buf: &[u8]) -> u64 {
    stream::Chunk::read(buf).unwrap().seq() as u64
}

// -------------------------------------------------------------- envelope

/// Like [`Ctx`] in owned mode, but every message leaves as an envelope
/// (header + payload + CRC32C) from `flush_envelope()`.
pub struct EnvCtx {
    builder: Builder<'static>,
    last: Option<Bytes>,
}

impl Default for EnvCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl EnvCtx {
    pub fn new() -> Self {
        let pool: &'static SimplePool = Box::leak(Box::new(SimplePool::new(64 * 1024)));
        EnvCtx { builder: Builder::new_enveloped(pool), last: None }
    }
    #[inline]
    fn build(&mut self, schema_id: u64, f: impl FnOnce(&mut Builder<'static>)) -> &[u8] {
        f(&mut self.builder);
        self.last = None; // previous message sent and dropped
        self.last = Some(self.builder.flush_envelope(schema_id).expect("message too large"));
        self.last.as_deref().unwrap()
    }
}

pub fn write_telemetry_enveloped<'c>(c: &'c mut EnvCtx, d: &Telemetry) -> &'c [u8] {
    c.build(iot::Telemetry::SCHEMA_ID, |b| {
        let mut t = TelemetryW::root(b);
        t.device(d.device).ts(d.ts).temp(d.temp).battery(d.battery);
        if let Some(h) = d.humidity {
            t.humidity(h);
        }
    })
}

pub fn write_chunk_enveloped<'c>(c: &'c mut EnvCtx, d: &Chunk) -> &'c [u8] {
    c.build(stream::Chunk::SCHEMA_ID, |b| {
        ChunkW::root(b).stream_id(d.stream_id).seq(d.seq).data(&d.data);
    })
}

/// Entries with known count (compare with `write_bank_known`).
pub fn write_bank_enveloped<'c>(c: &'c mut EnvCtx, d: &BankTx) -> &'c [u8] {
    c.build(graven_schemas::bank::Transaction::SCHEMA_ID, |b| {
        let mut t = TransactionW::root(b);
        bank_header(&mut t, d);
        let mut l = t.entries(d.entries.len() as u32);
        for e in &d.entries {
            let mut w = l.push();
            w.account(e.account).amount(e.amount);
            if let Some(m) = &e.memo {
                w.memo(m);
            }
        }
    })
}

/// Bare-message read: `verified()` (decode + full verify), then one field.
pub fn verified_one_telemetry(buf: &[u8]) -> u64 {
    iot::Telemetry::verified(buf).unwrap().temp().to_bits() as u64
}
pub fn verified_one_chunk(buf: &[u8]) -> u64 {
    stream::Chunk::verified(buf).unwrap().seq() as u64
}
pub fn verified_one_bank(buf: &[u8]) -> u64 {
    Transaction::verified(buf).unwrap().to_shard() as u64
}

/// Envelope read: `from_bytes()` (envelope decode + CRC + schema id + full verify), then one field.
pub fn from_bytes_one_telemetry(buf: &[u8]) -> u64 {
    iot::Telemetry::from_bytes(buf).unwrap().temp().to_bits() as u64
}
pub fn from_bytes_one_chunk(buf: &[u8]) -> u64 {
    stream::Chunk::from_bytes(buf).unwrap().seq() as u64
}
pub fn from_bytes_one_bank(buf: &[u8]) -> u64 {
    Transaction::from_bytes(buf).unwrap().to_shard() as u64
}

/// Envelope read, all fields (compare with `read_all_bank_verified`).
pub fn from_bytes_all_bank(buf: &[u8]) -> u64 {
    read_all_bank_trusted(&Transaction::from_bytes(buf).unwrap())
}
