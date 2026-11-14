//! Deterministic scenario data and reference checksums.

use crate::Sum;

pub struct BankEntry {
    pub account: u64,
    pub amount: i64,
    pub memo: Option<String>,
}
pub struct BankTx {
    pub id: u64,
    pub ts: u64,
    pub currency: u16,
    pub from_shard: u32,
    pub to_shard: u32,
    pub entries: Vec<BankEntry>,
    pub note: Option<String>,
}
pub struct Ent {
    pub id: u32,
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub hp: u16,
    pub alive: bool,
}
pub struct Frame {
    pub tick: u64,
    pub entities: Vec<Ent>,
}
pub struct Telemetry {
    pub device: u32,
    pub ts: u64,
    pub temp: f32,
    pub humidity: Option<f32>,
    pub battery: u8,
}
pub struct Chunk {
    pub stream_id: u64,
    pub seq: u32,
    pub data: Vec<u8>,
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn f(&mut self) -> f32 {
        (self.next() % 20_000) as f32 / 100.0 - 100.0
    }
}

pub fn bank(seed: u64, n_entries: usize) -> BankTx {
    let mut r = Rng(seed);
    let entries = (0..n_entries)
        .map(|i| BankEntry {
            account: r.next() % 1_000_000_000,
            amount: (r.next() % 2_000_000) as i64 - 1_000_000,
            memo: (i % 2 == 0).then(|| format!("ledger entry {i} ref {}", r.next() % 100_000)),
        })
        .collect();
    BankTx {
        id: r.next(),
        ts: 1_700_000_000_000 + r.next() % 1_000_000,
        currency: (r.next() % 3) as u16,
        from_shard: (r.next() % 64) as u32,
        to_shard: (r.next() % 64) as u32,
        entries,
        note: (!seed.is_multiple_of(3)).then(|| "transfer between shards".to_string()),
    }
}

pub fn frame(seed: u64, n: usize) -> Frame {
    let mut r = Rng(seed);
    let entities = (0..n)
        .map(|i| Ent {
            id: i as u32,
            pos: [r.f(), r.f(), r.f()],
            vel: [r.f(), r.f(), r.f()],
            hp: (r.next() % 1000) as u16,
            alive: !r.next().is_multiple_of(5),
        })
        .collect();
    Frame { tick: r.next() % 1_000_000, entities }
}

pub fn telemetry(seed: u64) -> Telemetry {
    let mut r = Rng(seed);
    Telemetry {
        device: r.next() as u32,
        ts: 1_700_000_000_000 + r.next() % 1_000_000,
        temp: r.f(),
        humidity: seed.is_multiple_of(2).then(|| r.f().abs()),
        battery: (r.next() % 101) as u8,
    }
}

pub fn chunk(seed: u64, size: usize) -> Chunk {
    let mut r = Rng(seed);
    Chunk { stream_id: r.next(), seq: r.next() as u32, data: (0..size).map(|_| r.next() as u8).collect() }
}

pub fn checksum_bank(d: &BankTx) -> u64 {
    let mut s = Sum::default();
    s.u(d.id);
    s.u(d.ts);
    s.u(d.currency as u64);
    s.u(d.from_shard as u64);
    s.u(d.to_shard as u64);
    s.s(d.note.as_deref().map(str::as_bytes));
    for e in &d.entries {
        s.u(e.account);
        s.u(e.amount as u64);
        s.s(e.memo.as_deref().map(str::as_bytes));
    }
    s.0
}

pub fn sum_entity(s: &mut Sum, id: u32, pos: [f32; 3], vel: [f32; 3], hp: u16, alive: bool) {
    s.u(id as u64);
    for v in pos.iter().chain(&vel) {
        s.u(v.to_bits() as u64);
    }
    s.u(hp as u64);
    s.u(alive as u64);
}

pub fn checksum_frame(d: &Frame) -> u64 {
    let mut s = Sum::default();
    s.u(d.tick);
    for e in &d.entities {
        sum_entity(&mut s, e.id, e.pos, e.vel, e.hp, e.alive);
    }
    s.0
}

pub fn checksum_telemetry(d: &Telemetry) -> u64 {
    let mut s = Sum::default();
    s.u(d.device as u64);
    s.u(d.ts);
    s.u(d.temp.to_bits() as u64);
    s.u(d.humidity.map_or(u64::MAX, |h| h.to_bits() as u64));
    s.u(d.battery as u64);
    s.0
}

pub fn checksum_chunk(d: &Chunk) -> u64 {
    let mut s = Sum::default();
    s.u(d.stream_id);
    s.u(d.seq as u64);
    s.s(Some(&d.data));
    s.0
}
