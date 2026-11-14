//! The optional envelope that wraps a raw Graven message for disk or network.
//!
//! Layout (little-endian):
//!
//! ```text
//! offset  size  field
//! 0       4     magic = "GRVN"
//! 4       1     format_version = 1
//! 5       1     flags = 0
//! 6       2     reserved = 0
//! 8       8     schema_id
//! 16      4     payload_len
//! 20      N     payload
//! 20+N    4     crc32c of bytes [0, 20+N)
//! ```
use crate::{Error, crc32c};

/// Software-only CRC32C (for benchmarking against the accelerated version). Not a stable API.
#[doc(hidden)]
pub use crate::crc::crc32c_sw;
use bytes::{BufMut, BytesMut};
use core::iter::FusedIterator;

/// The four magic bytes that start every envelope.
pub const MAGIC: [u8; 4] = *b"GRVN";
/// The only envelope format version this crate reads and writes.
pub const FORMAT_VERSION: u8 = 1;
/// Size of the fixed header preceding the payload.
pub const HEADER_LEN: usize = 20;
/// Size of the CRC32C trailer following the payload.
pub const TRAILER_LEN: usize = 4;

/// FNV-1a 64-bit hash of a fully qualified type name, e.g. `"bank.Transaction"`.
pub const fn schema_id(qualified_name: &str) -> u64 {
    let b = qualified_name.as_bytes();
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// Appends an envelope around `payload` to `out`.
///
/// Fails with [`Error::TooLarge`] if `payload` is longer than `i32::MAX` bytes; `out` is
/// left untouched in that case.
pub fn encode(schema_id: u64, payload: &[u8], out: &mut BytesMut) -> Result<(), Error> {
    if payload.len() as u64 > i32::MAX as u64 {
        return Err(Error::TooLarge);
    }
    let start = out.len();
    out.reserve(HEADER_LEN + payload.len() + TRAILER_LEN);
    out.put_slice(&header(schema_id, payload.len() as u32));
    out.put_slice(payload);
    let crc = crc32c(&out[start..]);
    out.put_u32_le(crc);
    Ok(())
}

/// The 20-byte header for a payload of `payload_len` bytes. Shared by [`encode`] and the
/// `Builder`, which writes it into the slot reserved before each message.
#[inline]
pub(crate) fn header(schema_id: u64, payload_len: u32) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..4].copy_from_slice(&MAGIC);
    h[4] = FORMAT_VERSION;
    // h[5] flags = 0, h[6..8] reserved = 0
    h[8..16].copy_from_slice(&schema_id.to_le_bytes());
    h[16..20].copy_from_slice(&payload_len.to_le_bytes());
    h
}

/// A validated envelope borrowed from the input buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Envelope<'a> {
    /// The root type's schema id.
    pub schema_id: u64,
    /// The raw Graven message.
    pub payload: &'a [u8],
    len: usize,
}

impl Envelope<'_> {
    /// Total bytes this envelope occupied in the input (`24 + payload.len()`).
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.len
    }
}

/// Parses and validates the envelope at the start of `buf`.
///
/// Trailing bytes after the CRC are accepted; use [`Envelope::len`] to step to the next
/// envelope. Checks run in this order: header length, magic, version, flags/reserved,
/// payload length, CRC. [`Error::Incomplete`] means "more bytes needed" (e.g. a file cut
/// by a crash); [`Error::ChecksumMismatch`] means a complete envelope whose CRC is wrong,
/// i.e. real corruption. Never panics.
pub fn decode(buf: &[u8]) -> Result<Envelope<'_>, Error> {
    if buf.len() < HEADER_LEN {
        return Err(Error::Incomplete { needed: HEADER_LEN });
    }
    if buf[..4] != MAGIC {
        return Err(Error::BadMagic);
    }
    if buf[4] != FORMAT_VERSION {
        return Err(Error::UnsupportedVersion(buf[4]));
    }
    if buf[5] != 0 || buf[6] != 0 || buf[7] != 0 {
        return Err(Error::BadHeader);
    }
    let schema_id = u64::from_le_bytes(buf[8..16].try_into().unwrap());
    let payload_len = u32::from_le_bytes(buf[16..20].try_into().unwrap());

    // u64 cannot overflow here: 20 + u32::MAX + 4 < 2^33.
    let needed64 = HEADER_LEN as u64 + payload_len as u64 + TRAILER_LEN as u64;
    let total = match usize::try_from(needed64) {
        Ok(t) if buf.len() >= t => t,
        Ok(t) => return Err(Error::Incomplete { needed: t }),
        Err(_) => return Err(Error::Incomplete { needed: usize::MAX }),
    };

    let crc_at = total - TRAILER_LEN;
    let stored = u32::from_le_bytes(buf[crc_at..total].try_into().unwrap());
    if crc32c(&buf[..crc_at]) != stored {
        return Err(Error::ChecksumMismatch);
    }
    Ok(Envelope { schema_id, payload: &buf[HEADER_LEN..crc_at], len: total })
}

/// Iterates over envelopes concatenated in `buf` (e.g. a batch from
/// `Builder::seal_envelope` … `take_frames`, or an append-only file).
///
/// Each item is [`decode`] of the remaining bytes. After the first `Err` (corruption or a
/// truncated tail) the iterator yields `None`. An empty buffer yields nothing.
#[inline]
pub fn envelopes(buf: &[u8]) -> Envelopes<'_> {
    Envelopes { buf, failed: false }
}

/// Iterator returned by [`envelopes`].
#[derive(Debug, Clone)]
pub struct Envelopes<'a> {
    buf: &'a [u8],
    failed: bool,
}

impl<'a> Iterator for Envelopes<'a> {
    type Item = Result<Envelope<'a>, Error>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.buf.is_empty() || self.failed {
            return None;
        }
        match decode(self.buf) {
            Ok(env) => {
                self.buf = &self.buf[env.len..];
                Some(Ok(env))
            }
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}

impl FusedIterator for Envelopes<'_> {}
