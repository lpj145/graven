//! Graven vs FlatBuffers vs Cap'n Proto: equivalent scenarios for benchmarking.

pub mod capnp_impl;
pub mod data;
pub mod fbs_impl;
pub mod graven_impl;

#[allow(clippy::all, unused_imports, dead_code, non_camel_case_types, mismatched_lifetime_syntaxes)]
#[path = "fbs_gen/bench_generated.rs"]
pub mod fbs_gen;

#[allow(clippy::all, unused_imports, dead_code)]
pub mod bench_capnp {
    include!(concat!(env!("OUT_DIR"), "/bench_capnp.rs"));
}

/// 8-byte aligned copy of a message (Cap'n Proto requires aligned input).
pub struct Aligned {
    words: Vec<capnp::Word>,
    len: usize,
}

impl Aligned {
    pub fn from(b: &[u8]) -> Self {
        let mut words = capnp::Word::allocate_zeroed_vec(b.len().div_ceil(8));
        capnp::Word::words_to_bytes_mut(&mut words)[..b.len()].copy_from_slice(b);
        Aligned { words, len: b.len() }
    }
    pub fn bytes(&self) -> &[u8] {
        &capnp::Word::words_to_bytes(&self.words)[..self.len]
    }
}

/// Order-sensitive checksum shared by every format's `read_all_*`.
#[derive(Default)]
pub struct Sum(pub u64);

impl Sum {
    /// Cheap on purpose (~1 cycle latency): a multiply here would add a
    /// serial dependency chain that dominates the read benchmarks and hides
    /// differences between formats.
    #[inline(always)]
    pub fn u(&mut self, v: u64) {
        self.0 = self.0.rotate_left(5) ^ v;
    }
    /// Absent ⇒ `u64::MAX`; present ⇒ len, first byte.
    #[inline(always)]
    pub fn s(&mut self, s: Option<&[u8]>) {
        match s {
            None => self.u(u64::MAX),
            Some(b) => {
                self.u(b.len() as u64);
                self.u(b.first().copied().unwrap_or(0) as u64);
            }
        }
    }
}
