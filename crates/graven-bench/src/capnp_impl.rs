use crate::bench_capnp::{chunk, entity, frame, telemetry, transaction};
use crate::data::{self, BankTx, Chunk, Frame, Telemetry};
use crate::Sum;
use capnp::message::{self, ReaderOptions, ScratchSpaceHeapAllocator};
use capnp::serialize;

/// Reused scratch allocator (zero heap allocation per message once warm) + output buffer.
pub struct Ctx {
    alloc: Option<ScratchSpaceHeapAllocator<'static>>,
    out: Vec<u8>,
    packed: bool,
}

impl Default for Ctx {
    fn default() -> Self {
        Self::new()
    }
}

impl Ctx {
    pub fn new() -> Self {
        let words: &'static mut [capnp::Word] = Box::leak(capnp::Word::allocate_zeroed_vec(16 * 1024).into_boxed_slice());
        let scratch = capnp::Word::words_to_bytes_mut(words);
        Ctx { alloc: Some(ScratchSpaceHeapAllocator::new(scratch)), out: Vec::with_capacity(128 * 1024), packed: false }
    }

    /// Writes use `serialize_packed` (size comparison only; readers expect unpacked).
    pub fn new_packed() -> Self {
        Ctx { packed: true, ..Self::new() }
    }

    #[inline]
    fn build(&mut self, f: impl FnOnce(&mut message::Builder<ScratchSpaceHeapAllocator<'static>>)) -> &[u8] {
        let mut msg = message::Builder::new(self.alloc.take().unwrap());
        f(&mut msg);
        self.out.clear();
        if self.packed {
            capnp::serialize_packed::write_message(&mut self.out, &msg).unwrap();
        } else {
            serialize::write_message(&mut self.out, &msg).unwrap();
        }
        self.alloc = Some(msg.into_allocator());
        &self.out
    }
}

const UNCHECKED: ReaderOptions = ReaderOptions { traversal_limit_in_words: None, nesting_limit: 64 };

macro_rules! root {
    ($buf:expr, $opts:expr, $ty:ty, |$r:ident| $body:expr) => {{
        let mut sl: &[u8] = $buf;
        let msg = serialize::read_message_from_flat_slice_no_alloc(&mut sl, $opts).unwrap();
        let $r = msg.get_root::<$ty>().unwrap();
        $body
    }};
}

// ------------------------------------------------------------------ bank

pub fn write_bank<'c>(c: &'c mut Ctx, d: &BankTx) -> &'c [u8] {
    c.build(|m| {
        let mut t = m.init_root::<transaction::Builder>();
        t.set_id(d.id);
        t.set_ts(d.ts);
        t.set_currency(d.currency);
        t.set_from_shard(d.from_shard);
        t.set_to_shard(d.to_shard);
        if let Some(n) = &d.note {
            t.set_note(n.as_str());
        }
        let mut l = t.init_entries(d.entries.len() as u32);
        for (i, e) in d.entries.iter().enumerate() {
            let mut w = l.reborrow().get(i as u32);
            w.set_account(e.account);
            w.set_amount(e.amount);
            if let Some(memo) = &e.memo {
                w.set_memo(memo.as_str());
            }
        }
    })
}

fn sum_bank(t: transaction::Reader<'_>) -> u64 {
    let mut s = Sum::default();
    s.u(t.get_id());
    s.u(t.get_ts());
    s.u(t.get_currency() as u64);
    s.u(t.get_from_shard() as u64);
    s.u(t.get_to_shard() as u64);
    s.s(t.has_note().then(|| t.get_note().unwrap().as_bytes()));
    for e in t.get_entries().unwrap().iter() {
        s.u(e.get_account());
        s.u(e.get_amount() as u64);
        s.s(e.has_memo().then(|| e.get_memo().unwrap().as_bytes()));
    }
    s.0
}

pub fn read_all_bank(buf: &[u8]) -> u64 {
    root!(buf, UNCHECKED, transaction::Reader, |t| sum_bank(t))
}

pub fn read_all_bank_verified(buf: &[u8]) -> u64 {
    root!(buf, ReaderOptions::new(), transaction::Reader, |t| sum_bank(t))
}

pub fn read_one_bank(buf: &[u8]) -> u64 {
    root!(buf, UNCHECKED, transaction::Reader, |t| t.get_to_shard() as u64)
}

// ----------------------------------------------------------------- frame

pub fn write_frame<'c>(c: &'c mut Ctx, d: &Frame) -> &'c [u8] {
    c.build(|m| {
        let mut f = m.init_root::<frame::Builder>();
        f.set_tick(d.tick);
        let mut l = f.init_entities(d.entities.len() as u32);
        for (i, e) in d.entities.iter().enumerate() {
            let mut w = l.reborrow().get(i as u32);
            w.set_id(e.id);
            {
                let mut p = w.reborrow().init_pos();
                p.set_x(e.pos[0]);
                p.set_y(e.pos[1]);
                p.set_z(e.pos[2]);
            }
            {
                let mut v = w.reborrow().init_vel();
                v.set_x(e.vel[0]);
                v.set_y(e.vel[1]);
                v.set_z(e.vel[2]);
            }
            w.set_hp(e.hp);
            w.set_alive(e.alive);
        }
    })
}

fn sum_entity(s: &mut Sum, e: entity::Reader<'_>) {
    let (p, v) = (e.get_pos(), e.get_vel());
    data::sum_entity(s, e.get_id(), [p.get_x(), p.get_y(), p.get_z()], [v.get_x(), v.get_y(), v.get_z()], e.get_hp(), e.get_alive());
}

fn sum_frame(f: frame::Reader<'_>) -> u64 {
    let mut s = Sum::default();
    s.u(f.get_tick());
    for e in f.get_entities().unwrap().iter() {
        sum_entity(&mut s, e);
    }
    s.0
}

pub fn read_all_frame(buf: &[u8]) -> u64 {
    root!(buf, UNCHECKED, frame::Reader, |f| sum_frame(f))
}

pub fn read_all_frame_verified(buf: &[u8]) -> u64 {
    root!(buf, ReaderOptions::new(), frame::Reader, |f| sum_frame(f))
}

pub fn read_one_frame(buf: &[u8]) -> u64 {
    root!(buf, UNCHECKED, frame::Reader, |f| {
        let l = f.get_entities().unwrap();
        l.get(l.len() / 2).get_hp() as u64
    })
}

// ------------------------------------------------------------- telemetry

pub fn write_telemetry<'c>(c: &'c mut Ctx, d: &Telemetry) -> &'c [u8] {
    c.build(|m| {
        let mut t = m.init_root::<telemetry::Builder>();
        t.set_device(d.device);
        t.set_ts(d.ts);
        t.set_temp(d.temp);
        match d.humidity {
            Some(h) => t.reborrow().init_humidity().set_value(h),
            None => t.reborrow().init_humidity().set_none(()),
        }
        t.set_battery(d.battery);
    })
}

fn sum_telemetry(t: telemetry::Reader<'_>) -> u64 {
    let mut s = Sum::default();
    s.u(t.get_device() as u64);
    s.u(t.get_ts());
    s.u(t.get_temp().to_bits() as u64);
    let h = match t.get_humidity().which().unwrap() {
        telemetry::humidity::Which::Value(h) => h.to_bits() as u64,
        telemetry::humidity::Which::None(()) => u64::MAX,
    };
    s.u(h);
    s.u(t.get_battery() as u64);
    s.0
}

pub fn read_all_telemetry(buf: &[u8]) -> u64 {
    root!(buf, UNCHECKED, telemetry::Reader, |t| sum_telemetry(t))
}

pub fn read_all_telemetry_verified(buf: &[u8]) -> u64 {
    root!(buf, ReaderOptions::new(), telemetry::Reader, |t| sum_telemetry(t))
}

pub fn read_one_telemetry(buf: &[u8]) -> u64 {
    root!(buf, UNCHECKED, telemetry::Reader, |t| t.get_temp().to_bits() as u64)
}

// ----------------------------------------------------------------- chunk

pub fn write_chunk<'c>(c: &'c mut Ctx, d: &Chunk) -> &'c [u8] {
    c.build(|m| {
        let mut ch = m.init_root::<chunk::Builder>();
        ch.set_stream_id(d.stream_id);
        ch.set_seq(d.seq);
        ch.set_data(&d.data);
    })
}

fn sum_chunk(ch: chunk::Reader<'_>) -> u64 {
    let mut s = Sum::default();
    s.u(ch.get_stream_id());
    s.u(ch.get_seq() as u64);
    s.s(Some(ch.get_data().unwrap()));
    s.0
}

pub fn read_all_chunk(buf: &[u8]) -> u64 {
    root!(buf, UNCHECKED, chunk::Reader, |c| sum_chunk(c))
}

pub fn read_all_chunk_verified(buf: &[u8]) -> u64 {
    root!(buf, ReaderOptions::new(), chunk::Reader, |c| sum_chunk(c))
}

pub fn read_one_chunk(buf: &[u8]) -> u64 {
    root!(buf, UNCHECKED, chunk::Reader, |c| c.get_seq() as u64)
}
