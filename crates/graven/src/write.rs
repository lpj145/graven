//! Forward-only writing into pooled `BytesMut`.

use crate::pool::BufPool;
use alloc::vec::Vec;
use crate::envelope::{self, HEADER_LEN};
use crate::{Bit, Error, Le};
use bytes::{Buf, Bytes, BytesMut};

/// Owns one pooled buffer. Build a message with `root()`, then:
/// - `finished()`: borrow it until the next `root()` (no atomics);
/// - `seal()` … `take_frames()`: batch length-prefixed frames into one `Bytes`
///   (one refcount per batch — the fast path for senders);
/// - `flush()`: one owned `Bytes` per message (one refcount per message);
/// - `finish()`: like `flush()`, and the buffer goes back to the pool.
///
/// Every message is written after a 4-byte slot that `seal()` fills with its
/// length, so frames are `[u32 len][message]` with no extra copy.
///
/// A builder made with [`Builder::new_enveloped`] reserves a
/// [`HEADER_LEN`]-byte slot instead, which `flush_envelope()` /
/// `seal_envelope()` fill with an envelope header (the CRC is appended after
/// the message) — again without copying the payload. `finished()` / `flush()`
/// still return the bare message in either mode; `seal()` only works in the
/// default mode and `seal_envelope()` / `flush_envelope()` only in the
/// enveloped one (otherwise: panic "builder mode mismatch").
pub struct Builder<'p> {
    pool: &'p dyn BufPool,
    buf: Option<BytesMut>,
    /// Aux buffers for streaming lists, reused across lists and messages.
    stash: Vec<BytesMut>,
    chunk: usize,
    /// End of the sealed frames at the front of `buf`.
    sealed: usize,
    /// Start of the current (unsealed) message; `None` when there is none.
    msg_start: Option<usize>,
    /// Bytes reserved before each message: 4 (frame length) or `HEADER_LEN` (envelope).
    prefix: usize,
    /// Envelope size multiple set by `with_record_align` (0: none).
    record_align: usize,
}

const FRAME_PREFIX: usize = 4;

/// Start a message after the sealed frames: drop any unsealed message, top up
/// the chunk when no frames are pending, write the length slot.
#[inline]
fn begin_msg(main: &mut BytesMut, sealed: usize, chunk: usize, prefix: usize) -> usize {
    main.truncate(sealed);
    // Reserve a fresh chunk only when running low and nothing is pending: by
    // now earlier messages have usually been dropped, so `reserve` reclaims
    // the old allocation instead of allocating.
    if sealed == 0 && main.capacity() < chunk / 4 {
        main.reserve(chunk);
    }
    // The default (frame) mode runs exactly the old code plus one predictable
    // compare; the envelope mode widens the slot out of line.
    main.extend_from_slice(&[0; FRAME_PREFIX]);
    if prefix != FRAME_PREFIX {
        widen_slot(main);
    }
    sealed + prefix
}

#[cold]
#[inline(never)]
fn widen_slot(main: &mut BytesMut) {
    main.extend_from_slice(&[0; HEADER_LEN - FRAME_PREFIX]);
}

#[cold]
#[inline(never)]
#[track_caller]
fn mode_mismatch() -> ! {
    panic!("builder mode mismatch")
}

impl<'p> Builder<'p> {
    #[inline]
    pub fn new(pool: &'p dyn BufPool) -> Self {
        Self::with_prefix(pool, FRAME_PREFIX)
    }

    /// A builder whose messages can be emitted as envelopes
    /// ([`crate::envelope`]) with `flush_envelope()` / `seal_envelope()`
    /// without copying: a `HEADER_LEN`-byte slot is reserved before each one.
    #[inline]
    pub fn new_enveloped(pool: &'p dyn BufPool) -> Self {
        Self::with_prefix(pool, HEADER_LEN)
    }

    #[inline]
    fn with_prefix(pool: &'p dyn BufPool, prefix: usize) -> Self {
        Builder {
            pool,
            buf: Some(pool.take()),
            stash: Vec::new(),
            chunk: pool.chunk_size(),
            sealed: 0,
            msg_start: None,
            prefix,
            record_align: 0,
        }
    }

    /// Pads the end of every envelope this builder writes with zero bytes, so that each
    /// envelope (`24 + payload_len` bytes) is a multiple of `r`. Envelopes written one after
    /// another from offset 0 (an append-only file) then all start at a multiple of `r`: a
    /// cache line (64) or a page (4096). The padding is inside the payload, after the message:
    /// unreferenced bytes, which every v1 reader accepts.
    ///
    /// # Panics
    /// Unless the builder was made with [`new_enveloped`](Self::new_enveloped) and `r` is a
    /// power of two from 2 to 65536.
    #[inline]
    pub fn with_record_align(mut self, r: usize) -> Self {
        if self.prefix != HEADER_LEN {
            mode_mismatch();
        }
        assert!(r.is_power_of_two() && (2..=65536).contains(&r), "with_record_align: {r} is not a power of two from 2 to 65536");
        self.record_align = r;
        self
    }

    /// Start the root table of a new message (an unsealed previous message is discarded).
    #[inline]
    pub fn root(&mut self, fixed_len: u16) -> TableW<'_> {
        let main = self.buf.as_mut().expect("builder already finished");
        let ms = begin_msg(main, self.sealed, self.chunk, self.prefix);
        self.msg_start = Some(ms);
        let base = if self.prefix == HEADER_LEN { ms - HEADER_LEN } else { ms };
        let start = alloc_table(main, fixed_len);
        TableW { main, aux: None, start, base, pool: self.pool, stash: &mut self.stash }
    }

    /// Root table with only scalar fields (no pointers) and compile-time size `N`.
    #[inline]
    pub fn root_fixed<const N: usize>(&mut self) -> FixedW<'_, N> {
        let main = self.buf.as_mut().expect("builder already finished");
        let ms = begin_msg(main, self.sealed, self.chunk, self.prefix);
        self.msg_start = Some(ms);
        let start = alloc_table(main, N as u16);
        FixedW::new(&mut main[start..start + N])
    }

    #[inline]
    fn current(&self) -> &[u8] {
        let buf = self.buf.as_ref().expect("builder already finished");
        match self.msg_start {
            Some(ms) => &buf[ms..],
            None => &[],
        }
    }

    /// Borrow the current message without freezing it (no refcount traffic).
    /// Valid until the next `root()` — the equivalent of FlatBuffers'
    /// `finished_data()`.
    #[inline]
    pub fn finished(&self) -> Result<&[u8], Error> {
        let m = self.current();
        if m.len() > i32::MAX as usize {
            return Err(Error::TooLarge);
        }
        Ok(m)
    }

    /// Seal the current message as a frame `[u32 len][message]` in the
    /// pending batch. No atomics; a no-op when there is no current message.
    #[inline]
    pub fn seal(&mut self) -> Result<(), Error> {
        if self.prefix != FRAME_PREFIX {
            mode_mismatch();
        }
        let Some(ms) = self.msg_start.take() else { return Ok(()) };
        let buf = self.buf.as_mut().expect("builder already finished");
        let len = buf.len() - ms;
        if len > i32::MAX as usize {
            buf.truncate(self.sealed);
            return Err(Error::TooLarge);
        }
        buf[ms - 4..ms].copy_from_slice(&(len as u32).to_le_bytes());
        self.sealed = buf.len();
        Ok(())
    }

    /// Bytes of sealed frames pending in the batch (to ship by size).
    #[inline]
    pub fn sealed_len(&self) -> usize {
        self.sealed
    }

    /// Freeze every sealed frame as one `Bytes` (one refcount for the whole
    /// batch) and start an empty batch. An unsealed message is discarded.
    /// Split the result back into messages with [`crate::frames`].
    #[inline]
    pub fn take_frames(&mut self) -> Bytes {
        let buf = self.buf.as_mut().expect("builder already finished");
        buf.truncate(self.sealed);
        self.sealed = 0;
        self.msg_start = None;
        buf.split().freeze()
    }

    /// Freeze the current message alone (without its length slot) and keep
    /// the builder. One refcount per message. Panics if sealed frames are
    /// pending (call `take_frames` first).
    #[inline]
    pub fn flush(&mut self) -> Result<Bytes, Error> {
        assert!(self.sealed == 0, "flush: sealed frames pending; call take_frames first");
        let buf = self.buf.as_mut().expect("builder already finished");
        let Some(ms) = self.msg_start.take() else {
            buf.clear();
            return Ok(Bytes::new());
        };
        if buf.len() - ms > i32::MAX as usize {
            buf.clear();
            return Err(Error::TooLarge);
        }
        let mut msg = buf.split().freeze();
        msg.advance(ms); // skip the unused length slot: pointer bump, no atomics
        Ok(msg)
    }

    /// Write the envelope header into the slot before the message at `ms` and
    /// append the CRC. `buf` must end at the end of the message.
    #[inline]
    fn close_envelope(buf: &mut BytesMut, ms: usize, schema_id: u64, record_align: usize) {
        if record_align > 1 {
            let total = HEADER_LEN + (buf.len() - ms) + envelope::TRAILER_LEN;
            let pad = (record_align - total % record_align) % record_align;
            buf.resize(buf.len() + pad, 0);
        }
        let len = buf.len() - ms;
        buf[ms - HEADER_LEN..ms].copy_from_slice(&envelope::header(schema_id, len as u32));
        let crc = crate::crc32c(&buf[ms - HEADER_LEN..]);
        buf.extend_from_slice(&crc.to_le_bytes());
    }

    /// Seal the current message as an envelope in the pending batch, so
    /// `take_frames()` returns concatenated envelopes (split them with
    /// [`envelope::envelopes`]). A no-op when there is no current message.
    /// Panics with "builder mode mismatch" unless built with `new_enveloped`.
    #[inline]
    pub fn seal_envelope(&mut self, schema_id: u64) -> Result<(), Error> {
        if self.prefix != HEADER_LEN {
            mode_mismatch();
        }
        let Some(ms) = self.msg_start.take() else { return Ok(()) };
        let buf = self.buf.as_mut().expect("builder already finished");
        if buf.len() - ms + self.record_align > i32::MAX as usize {
            buf.truncate(self.sealed);
            return Err(Error::TooLarge);
        }
        Self::close_envelope(buf, ms, schema_id, self.record_align);
        self.sealed = buf.len();
        Ok(())
    }

    /// Freeze the current message as one envelope (header + message + CRC)
    /// and keep the builder; the payload is not copied. Returns an empty
    /// `Bytes` when there is no current message. Panics with "builder mode
    /// mismatch" unless built with `new_enveloped`, and if sealed frames are
    /// pending (call `take_frames` first).
    #[inline]
    pub fn flush_envelope(&mut self, schema_id: u64) -> Result<Bytes, Error> {
        if self.prefix != HEADER_LEN {
            mode_mismatch();
        }
        assert!(self.sealed == 0, "flush_envelope: sealed frames pending; call take_frames first");
        let buf = self.buf.as_mut().expect("builder already finished");
        let Some(ms) = self.msg_start.take() else {
            buf.clear();
            return Ok(Bytes::new());
        };
        if buf.len() - ms + self.record_align > i32::MAX as usize {
            buf.clear();
            return Err(Error::TooLarge);
        }
        Self::close_envelope(buf, ms, schema_id, self.record_align);
        let mut env = buf.split().freeze();
        env.advance(ms - HEADER_LEN); // 0 today (sealed == 0); kept symmetric with `flush`
        Ok(env)
    }

    /// Freeze the message; the buffer goes back to the pool.
    #[inline]
    pub fn finish(mut self) -> Result<Bytes, Error> {
        self.flush()
    }
}

impl Drop for Builder<'_> {
    fn drop(&mut self) {
        if let Some(b) = self.buf.take() {
            self.pool.give(b);
        }
        for b in self.stash.drain(..) {
            self.pool.give(b);
        }
    }
}

/// Append `[u16 fixed_len][zeros]`; returns the fixed part start.
#[inline]
fn alloc_table(main: &mut BytesMut, fixed_len: u16) -> usize {
    let pos = main.len();
    main.resize(pos + 2 + fixed_len as usize, 0);
    main[pos..pos + 2].copy_from_slice(&fixed_len.to_le_bytes());
    pos + 2
}

/// Bytes of `n` elements of `efl` bytes each; panics beyond `i32::MAX` (checked
/// in `u64`, so it cannot wrap on 32-bit targets) before anything is allocated.
#[inline]
fn list_size(n: u32, efl: u64) -> usize {
    let size = n as u64 * efl;
    assert!(size <= i32::MAX as u64, "list too large: {n} × {efl} bytes exceeds i32::MAX");
    size as usize
}

/// `first + size` without `usize` overflow (on 32-bit targets, `main` near 2 GiB plus a list
/// of up to `i32::MAX` bytes would exceed `usize::MAX`). Same programmer-error panic.
#[inline]
fn list_end(first: usize, size: usize) -> usize {
    first.checked_add(size).unwrap_or_else(|| panic!("list too large: buffer offset {first} + {size} bytes overflows usize"))
}

#[inline]
fn put<T: Le>(main: &mut BytesMut, v: T) {
    let mut a = [0u8; 16];
    v.write_le(&mut a[..T::SIZE]);
    main.extend_from_slice(&a[..T::SIZE]);
}

/// Writer for a table whose fields are all scalars (no pointers). The fixed
/// part is a `[u8; N]` known at compile time, so field writes with constant
/// offsets compile without bounds checks.
pub struct FixedW<'a, const N: usize> {
    fixed: &'a mut [u8; N],
}

impl<'a, const N: usize> FixedW<'a, N> {
    #[inline(always)]
    fn new(s: &'a mut [u8]) -> Self {
        FixedW { fixed: s.try_into().expect("FixedW: element size mismatch (writer bug)") }
    }

    #[inline(always)]
    pub fn set<T: Le>(&mut self, off: u16, presence: Bit, v: T) {
        let i = off as usize;
        v.write_le(&mut self.fixed[i..i + T::SIZE]);
        self.fixed[presence.byte as usize] |= 1 << presence.shift;
    }

    /// Whether the presence bit `b` is set: the field was written.
    #[inline(always)]
    pub fn is_set(&self, b: Bit) -> bool {
        (self.fixed[b.byte as usize] >> b.shift) & 1 == 1
    }

    /// See [`TableW::copy_inline`].
    #[inline]
    pub fn copy_inline(&mut self, src: &crate::TableRef<'_>, off: u16, size: u16, presence: Bit) {
        if !src.bit(presence) {
            return;
        }
        for k in 0..size {
            self.fixed[(off + k) as usize] = src.fixed_byte(off + k);
        }
        self.fixed[presence.byte as usize] |= 1 << presence.shift;
    }

    /// See [`TableW::copy_bool`].
    #[inline]
    pub fn copy_bool(&mut self, src: &crate::TableRef<'_>, presence: Bit) {
        if src.bit(presence) {
            self.set_bool(presence, src.bit(Bit { byte: presence.byte, shift: presence.shift + 1 }));
        }
    }

    #[inline(always)]
    pub fn set_bool(&mut self, presence: Bit, v: bool) {
        let b = &mut self.fixed[presence.byte as usize];
        *b |= 1 << presence.shift;
        if v {
            *b |= 1 << (presence.shift + 1);
        } else {
            *b &= !(1 << (presence.shift + 1));
        }
    }
}

/// Writer for one table. The fixed part lives in `aux` when it is a streaming
/// list element, otherwise in `main`; variable data always goes to `main`.
///
/// # Panics
///
/// Writer panics are programmer errors, never a consequence of data read from
/// outside: pushing more than the `n` elements reserved for a list, reserving a
/// list larger than `i32::MAX` bytes (2 GiB), or using a `Builder` after
/// `finish`.
pub struct TableW<'b> {
    main: &'b mut BytesMut,
    aux: Option<&'b mut BytesMut>,
    start: usize,
    /// Offset in `main` that `align_data` counts from (start of the container).
    base: usize,
    pool: &'b dyn BufPool,
    stash: &'b mut Vec<BytesMut>,
}

impl<'b> TableW<'b> {
    #[inline]
    fn fixed_buf(&mut self) -> &mut BytesMut {
        match self.aux.as_deref_mut() {
            Some(a) => a,
            None => self.main,
        }
    }

    #[inline]
    fn set_bit(&mut self, b: Bit) {
        let i = self.start + b.byte as usize;
        self.fixed_buf()[i] |= 1 << b.shift;
    }

    #[inline]
    fn clear_bit(&mut self, b: Bit) {
        let i = self.start + b.byte as usize;
        self.fixed_buf()[i] &= !(1 << b.shift);
    }

    /// Store pointer at slot `off` targeting absolute `target` in `main`.
    /// In an aux buffer (streaming list element) the slot temporarily holds
    /// `target + 1` (never 0) and is converted to a relative offset on close.
    #[inline]
    fn patch(&mut self, off: u16, target: usize) {
        let slot = self.start + off as usize;
        let v = if self.aux.is_some() { (target as i64 + 1) as i32 } else { (target as i64 - slot as i64) as i32 };
        v.write_le(&mut self.fixed_buf()[slot..slot + 4]);
    }

    #[inline]
    pub fn set<T: Le>(&mut self, off: u16, presence: Bit, v: T) {
        let i = self.start + off as usize;
        v.write_le(&mut self.fixed_buf()[i..i + T::SIZE]);
        self.set_bit(presence);
    }

    #[inline]
    pub fn set_bool(&mut self, presence: Bit, v: bool) {
        self.set_bit(presence);
        let value = Bit { byte: presence.byte, shift: presence.shift + 1 };
        if v { self.set_bit(value) } else { self.clear_bit(value) }
    }

    /// Pads `main` with zero bytes so that the data of the next variable-size value (after
    /// its 4-byte length or count) starts at a multiple of `align`, counted from the start of
    /// the container (`#[align(to = N)]`). `align` is a power of two.
    #[inline]
    pub fn align_data(&mut self, align: usize) {
        debug_assert!(align.is_power_of_two());
        let at = self.main.len() + 4 - self.base;
        let pad = (align - at % align) % align;
        if pad > 0 {
            self.main.resize(self.main.len() + pad, 0);
        }
    }

    /// Whether the presence bit `b` is set: the field was written.
    #[inline]
    pub fn is_set(&self, b: Bit) -> bool {
        let fixed = match self.aux.as_deref() {
            Some(a) => a,
            None => &*self.main,
        };
        (fixed[self.start + b.byte as usize] >> b.shift) & 1 == 1
    }

    /// Whether the pointer slot `off` (or the union slot's tag at `off`) was written.
    #[inline]
    pub fn has_slot(&self, off: u16, size: u16) -> bool {
        let fixed = match self.aux.as_deref() {
            Some(a) => a,
            None => &*self.main,
        };
        let i = self.start + off as usize;
        fixed[i..i + size as usize].iter().any(|&b| b != 0)
    }

    /// Copies an inline field (scalar, enum, struct: `size` bytes at `off`) from `src` when it
    /// is present there, keeping its presence. Unknown enum values are copied as they are.
    #[inline]
    pub fn copy_inline(&mut self, src: &crate::TableRef<'_>, off: u16, size: u16, presence: Bit) {
        if !src.bit(presence) {
            return;
        }
        let i = self.start + off as usize;
        for k in 0..size {
            let v = src.fixed_byte(off + k);
            self.fixed_buf()[i + k as usize] = v;
        }
        self.set_bit(presence);
    }

    /// Copies a `bool` field (its presence and value bits) from `src` when it is present there.
    #[inline]
    pub fn copy_bool(&mut self, src: &crate::TableRef<'_>, presence: Bit) {
        if src.bit(presence) {
            self.set_bool(presence, src.bit(Bit { byte: presence.byte, shift: presence.shift + 1 }));
        }
    }

    /// A list of `count` scalars from their little-endian bytes (`raw.len()` = `count × size`).
    pub fn set_scalars_raw(&mut self, off: u16, count: u32, raw: &[u8]) {
        let pos = self.main.len();
        self.main.reserve(4 + raw.len());
        put(self.main, count);
        self.main.extend_from_slice(raw);
        self.patch(off, pos);
    }

    #[inline]
    pub fn set_bytes(&mut self, off: u16, b: &[u8]) {
        let pos = self.main.len();
        self.main.reserve(4 + b.len());
        put(self.main, b.len() as u32);
        self.main.extend_from_slice(b);
        self.patch(off, pos);
    }

    #[inline]
    pub fn set_str(&mut self, off: u16, s: &str) {
        self.set_bytes(off, s.as_bytes());
    }

    pub fn set_scalars<T: Le>(&mut self, off: u16, v: &[T]) {
        let pos = self.main.len();
        let n = 4 + v.len() * T::SIZE;
        self.main.resize(pos + n, 0);
        (v.len() as u32).write_le(&mut self.main[pos..pos + 4]);
        for (chunk, x) in self.main[pos + 4..pos + n].chunks_exact_mut(T::SIZE).zip(v) {
            x.write_le(chunk);
        }
        self.patch(off, pos);
    }

    /// `[string]`: same layout as [`set_bytes_list`](Self::set_bytes_list).
    pub fn set_strs<'s>(&mut self, off: u16, v: impl ExactSizeIterator<Item = &'s str>) {
        self.set_bytes_list(off, v.map(str::as_bytes));
    }

    /// `[bytes]`: `[u32 count][count × i32 offset]`, then each `[u32 len][bytes]` in order.
    pub fn set_bytes_list<'s>(&mut self, off: u16, v: impl ExactSizeIterator<Item = &'s [u8]>) {
        let pos = self.main.len();
        let n = v.len();
        self.main.resize(pos + 4 + 4 * n, 0);
        (n as u32).write_le(&mut self.main[pos..pos + 4]);
        for (i, b) in v.enumerate() {
            let slot = pos + 4 + 4 * i;
            let target = self.main.len();
            put(self.main, b.len() as u32);
            self.main.extend_from_slice(b);
            ((target - slot) as i32).write_le(&mut self.main[slot..slot + 4]);
        }
        self.patch(off, pos);
    }

    /// Nested table appended to `main`.
    pub fn table(&mut self, off: u16, fixed_len: u16) -> TableW<'_> {
        let pos = self.main.len();
        let start = alloc_table(self.main, fixed_len);
        self.patch(off, pos);
        TableW { main: &mut *self.main, aux: None, start, base: self.base, pool: self.pool, stash: &mut *self.stash }
    }

    /// Union slot `[u16 tag][i32 ptr]` with a nested table variant.
    pub fn union(&mut self, off: u16, tag: u16, fixed_len: u16) -> TableW<'_> {
        let i = self.start + off as usize;
        tag.write_le(&mut self.fixed_buf()[i..i + 2]);
        self.table(off + 2, fixed_len)
    }

    /// Nested all-scalar table of compile-time size `N` (same bytes as
    /// [`table`](Self::table)`(off, N)`), written through a `FixedW`.
    #[inline]
    pub fn table_fixed<const N: usize>(&mut self, off: u16) -> FixedW<'_, N> {
        let pos = self.main.len();
        let start = alloc_table(self.main, N as u16);
        self.patch(off, pos);
        FixedW::new(&mut self.main[start..start + N])
    }

    /// Union slot with an all-scalar variant of compile-time size `N` (same bytes as
    /// [`union`](Self::union)`(off, tag, N)`).
    #[inline]
    pub fn union_fixed<const N: usize>(&mut self, off: u16, tag: u16) -> FixedW<'_, N> {
        let i = self.start + off as usize;
        tag.write_le(&mut self.fixed_buf()[i..i + 2]);
        self.table_fixed(off + 2)
    }

    /// List of `n` tables, fixed parts reserved contiguously in `main`.
    pub fn tables(&mut self, off: u16, n: u32, elem_fixed_len: u16) -> TableListW<'_> {
        let size = list_size(n, elem_fixed_len as u64);
        let pos = self.main.len();
        let first = pos + 6;
        self.main.resize(list_end(first, size), 0);
        elem_fixed_len.write_le(&mut self.main[pos + 4..pos + 6]);
        self.patch(off, pos);
        TableListW {
            main: &mut *self.main,
            base: self.base,
            pool: self.pool,
            stash: &mut *self.stash,
            pos,
            first,
            efl: elem_fixed_len,
            cap: n,
            count: 0,
        }
    }

    /// List of `n` all-scalar tables of compile-time size `N`. The element
    /// region is split into `[u8; N]` chunks once, so `push` is an iterator
    /// step with no per-element slicing or bounds checks.
    #[inline]
    pub fn tables_fixed<const N: usize>(&mut self, off: u16, n: u32) -> FixedListW<'_, N> {
        let size = list_size(n, N as u64);
        let pos = self.main.len();
        let first = pos + 6;
        let end = list_end(first, size);
        self.main.resize(end, 0);
        (N as u16).write_le(&mut self.main[pos + 4..pos + 6]);
        self.patch(off, pos);
        let (head, region) = self.main[pos..end].split_at_mut(6);
        let (chunks, _) = region.as_chunks_mut::<N>();
        FixedListW { count_slot: (&mut head[..4]).try_into().unwrap(), elems: chunks.iter_mut(), count: 0 }
    }

    /// List of tables of unknown length. Element fixed parts are built in a
    /// pooled aux buffer and copied on close; `ptr_slots` lists every i32
    /// pointer offset inside an element's fixed part (fixed up on close).
    pub fn tables_streaming(&mut self, off: u16, elem_fixed_len: u16, ptr_slots: &'static [u16]) -> StreamListW<'_, 'b> {
        let aux = match self.stash.pop() {
            Some(a) => a,
            None => self.pool.take(),
        };
        StreamListW { parent: self, off, aux, efl: elem_fixed_len, ptr_slots, count: 0, closed: false }
    }

    /// Scalar list of unknown length appended directly to `main`.
    pub fn scalars_streaming<T: Le>(&mut self, off: u16) -> ScalarListW<'_, T> {
        let pos = self.main.len();
        self.main.resize(pos + 4, 0);
        self.patch(off, pos);
        ScalarListW { main: &mut *self.main, pos, count: 0, _t: core::marker::PhantomData }
    }
}

/// Known-count list writer. On drop the count is set to the number pushed.
pub struct TableListW<'a> {
    main: &'a mut BytesMut,
    base: usize,
    pool: &'a dyn BufPool,
    stash: &'a mut Vec<BytesMut>,
    pos: usize,
    first: usize,
    efl: u16,
    cap: u32,
    count: u32,
}

impl TableListW<'_> {
    /// Next element. Panics if more than the reserved `n` are pushed (writer bug).
    #[inline]
    pub fn push(&mut self) -> TableW<'_> {
        assert!(self.count < self.cap, "TableListW: pushed more elements than reserved");
        let start = self.first + self.count as usize * self.efl as usize;
        self.count += 1;
        TableW { main: &mut *self.main, aux: None, start, base: self.base, pool: self.pool, stash: &mut *self.stash }
    }
}

impl TableListW<'_> {
    /// Next element as an all-scalar `FixedW` (`N` must equal `elem_fixed_len`).
    #[inline]
    pub fn push_fixed<const N: usize>(&mut self) -> FixedW<'_, N> {
        assert!(self.count < self.cap, "TableListW: pushed more elements than reserved");
        let start = self.first + self.count as usize * self.efl as usize;
        self.count += 1;
        FixedW::new(&mut self.main[start..start + N])
    }
}

impl Drop for TableListW<'_> {
    #[inline]
    fn drop(&mut self) {
        let p = self.pos;
        self.count.write_le(&mut self.main[p..p + 4]);
    }
}

/// Known-count list of all-scalar elements (see `TableW::tables_fixed`).
/// On drop the count is set to the number pushed.
pub struct FixedListW<'a, const N: usize> {
    count_slot: &'a mut [u8; 4],
    elems: core::slice::IterMut<'a, [u8; N]>,
    count: u32,
}

impl<const N: usize> FixedListW<'_, N> {
    /// Next element. Panics if more than the reserved `n` are pushed (writer bug).
    #[inline(always)]
    pub fn push(&mut self) -> FixedW<'_, N> {
        let fixed = self.elems.next().expect("FixedListW: pushed more elements than reserved");
        self.count += 1;
        FixedW { fixed }
    }
}

impl<const N: usize> Drop for FixedListW<'_, N> {
    #[inline]
    fn drop(&mut self) {
        *self.count_slot = self.count.to_le_bytes();
    }
}

/// Unknown-count table list writer; finalized by `close()` or on drop.
pub struct StreamListW<'a, 'b> {
    parent: &'a mut TableW<'b>,
    off: u16,
    aux: BytesMut,
    efl: u16,
    ptr_slots: &'static [u16],
    count: u32,
    closed: bool,
}

impl<'b> StreamListW<'_, 'b> {
    #[inline]
    pub fn push(&mut self) -> TableW<'_> {
        let start = self.aux.len();
        self.aux.resize(start + self.efl as usize, 0);
        self.count += 1;
        TableW {
            main: &mut *self.parent.main,
            aux: Some(&mut self.aux),
            start,
            base: self.parent.base,
            pool: self.parent.pool,
            stash: &mut *self.parent.stash,
        }
    }

    /// Next element as an all-scalar `FixedW` (`N` must equal `elem_fixed_len`).
    #[inline]
    pub fn push_fixed<const N: usize>(&mut self) -> FixedW<'_, N> {
        let start = self.aux.len();
        self.aux.resize(start + self.efl as usize, 0);
        self.count += 1;
        FixedW::new(&mut self.aux[start..start + N])
    }

    pub fn close(mut self) {
        self.finalize();
    }

    fn finalize(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let main = &mut *self.parent.main;
        let header = main.len();
        put(main, self.count);
        put(main, self.efl);
        let base = main.len();
        main.extend_from_slice(&self.aux);
        let efl = self.efl as usize;
        for i in 0..self.count as usize {
            for &slot in self.ptr_slots {
                let p = base + i * efl + slot as usize;
                let stored = i32::read_le(&main[p..p + 4]);
                if stored != 0 {
                    let rel = (stored as i64 - 1) - p as i64;
                    (rel as i32).write_le(&mut main[p..p + 4]);
                }
            }
        }
        self.parent.patch(self.off, header);
        let mut aux = core::mem::take(&mut self.aux);
        aux.clear();
        self.parent.stash.push(aux);
    }
}

impl Drop for StreamListW<'_, '_> {
    fn drop(&mut self) {
        self.finalize();
    }
}

/// Unknown-count scalar list writer; count is written on drop.
pub struct ScalarListW<'a, T: Le> {
    main: &'a mut BytesMut,
    pos: usize,
    count: u32,
    _t: core::marker::PhantomData<T>,
}

impl<T: Le> ScalarListW<'_, T> {
    #[inline]
    pub fn push(&mut self, v: T) {
        put(self.main, v);
        self.count += 1;
    }
}

impl<T: Le> Drop for ScalarListW<'_, T> {
    #[inline]
    fn drop(&mut self) {
        let p = self.pos;
        self.count.write_le(&mut self.main[p..p + 4]);
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::pool::{BufPool, SimplePool};

    #[test]
    fn align_data_and_as_slice() {
        let pool = crate::LocalPool;
        for enveloped in [false, true] {
            let mut b = if enveloped { Builder::new_enveloped(&pool) } else { Builder::new(&pool) };
            {
                let mut w = b.root(8);
                w.set_str(0, "abc");
                w.align_data(16);
                w.set_scalars::<f64>(4, &[1.5, -2.0, 3.25]);
            }
            let (whole, base) = if enveloped {
                (b.flush_envelope(7).unwrap().to_vec(), 0)
            } else {
                (b.finished().unwrap().to_vec(), 0)
            };
            // Copy to a 16-aligned address, as a mapped file would be.
            let mut store = vec![0u8; whole.len() + 16];
            let at = (16 - store.as_ptr() as usize % 16) % 16;
            store[at..at + whole.len()].copy_from_slice(&whole);
            let whole = &store[at..at + whole.len()];
            let msg = if enveloped { crate::envelope::decode(whole).unwrap().payload } else { whole };
            let t = crate::TableRef::at(msg, 0).unwrap();
            let l = t.scalars::<f64>(4).unwrap().unwrap();
            let off = l.as_bytes().as_ptr() as usize - whole.as_ptr() as usize - base;
            assert_eq!(off % 16, 0, "enveloped = {enveloped}");
            assert_eq!(l.as_slice(), Some(&[1.5, -2.0, 3.25][..]));
            // One byte off: no slice, same values.
            let mut shifted = vec![0u8; whole.len() + 1];
            shifted[1..].copy_from_slice(whole);
            let whole2 = &shifted[1..];
            let msg2 = if enveloped { crate::envelope::decode(whole2).unwrap().payload } else { whole2 };
            let l2 = crate::TableRef::at(msg2, 0).unwrap().scalars::<f64>(4).unwrap().unwrap();
            assert_eq!(l2.as_slice(), None);
            assert_eq!(l2.iter().collect::<Vec<_>>(), vec![1.5, -2.0, 3.25]);
        }
    }

    #[test]
    fn record_align_pads_the_envelope() {
        let pool = crate::LocalPool;
        let mut b = Builder::new_enveloped(&pool).with_record_align(64);
        for n in [0usize, 1, 39, 40, 41, 100] {
            {
                let mut w = b.root(4);
                w.set_str(0, &"z".repeat(n));
            }
            let env = b.flush_envelope(1).unwrap();
            assert_eq!(env.len() % 64, 0, "{n}");
            let e = crate::envelope::decode(&env).unwrap();
            let t = crate::TableRef::at(e.payload, 0).unwrap();
            assert_eq!(t.str(0).unwrap().unwrap().len(), n);
        }
    }

    #[test]
    #[should_panic(expected = "builder mode mismatch")]
    fn record_align_needs_an_enveloped_builder() {
        let _ = Builder::new(&crate::LocalPool).with_record_align(64);
    }
    use crate::{Bit, TableRef};
    use bytes::BytesMut;
    use core::cell::Cell;

    const B0: Bit = Bit { byte: 0, shift: 0 };
    const B1: Bit = Bit { byte: 0, shift: 1 };
    const BB: Bit = Bit { byte: 0, shift: 2 }; // bool: bits 2,3

    // layout: [bits @0][u64 @1][i16 @9][ptr str @11][ptr bytes @15][ptr table @19][union @23] = 29
    const FL: u16 = 29;

    #[test]
    fn roundtrip_scalars_bool_str_bytes() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            t.set::<u64>(1, B0, 0xDEAD_BEEF_0000_0001);
            t.set::<i16>(9, B1, -7);
            t.set_bool(BB, true);
            t.set_str(11, "olá");
            t.set_bytes(15, &[1, 2, 3]);
        }
        let msg = b.finish().unwrap();
        let t = TableRef::at(&msg, 0).unwrap();
        assert_eq!(t.fixed_len(), FL);
        assert_eq!(t.scalar::<u64>(1, B0).unwrap(), Some(0xDEAD_BEEF_0000_0001));
        assert_eq!(t.scalar::<i16>(9, B1).unwrap(), Some(-7));
        assert_eq!(t.bool(BB), Some(true));
        assert_eq!(t.str(11).unwrap(), Some("olá"));
        assert_eq!(t.bytes(15).unwrap(), Some(&[1u8, 2, 3][..]));
        assert!(t.table(19).unwrap().is_none());
        assert!(t.union(23).unwrap().is_none());
    }

    #[test]
    fn bool_false_is_present() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        b.root(FL).set_bool(BB, false);
        let msg = b.finish().unwrap();
        assert_eq!(TableRef::at(&msg, 0).unwrap().bool(BB), Some(false));
    }

    #[test]
    fn nested_table_and_union_roundtrip() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            {
                let mut c = t.table(19, 5);
                c.set::<u32>(1, B0, 77);
            }
            t.set::<u64>(1, B0, 5);
            let mut u = t.union(23, 3, 5);
            u.set::<u32>(1, B0, 99);
        }
        let msg = b.finish().unwrap();
        let t = TableRef::at(&msg, 0).unwrap();
        assert_eq!(t.scalar::<u64>(1, B0).unwrap(), Some(5));
        let c = t.table(19).unwrap().unwrap();
        assert_eq!(c.fixed_len(), 5);
        assert_eq!(c.scalar::<u32>(1, B0).unwrap(), Some(77));
        let (tag, u) = t.union(23).unwrap().unwrap();
        assert_eq!(tag, 3);
        assert_eq!(u.scalar::<u32>(1, B0).unwrap(), Some(99));
    }

    #[test]
    fn empty_string_present_vs_absent() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            t.set_str(11, "");
            t.set_scalars::<u32>(19, &[]);
        }
        let msg = b.finish().unwrap();
        let t = TableRef::at(&msg, 0).unwrap();
        assert_eq!(t.str(11).unwrap(), Some(""));
        assert_eq!(t.bytes(15).unwrap(), None);
        assert_eq!(t.scalars::<u32>(19).unwrap().unwrap().len(), 0);
    }

    #[test]
    fn scalars_and_strs_roundtrip() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            t.set_scalars::<u16>(11, &[1, 2, 3]);
            t.set_strs(15, ["a", "", "ccc"].into_iter());
        }
        let msg = b.finish().unwrap();
        let t = TableRef::at(&msg, 0).unwrap();
        let s = t.scalars::<u16>(11).unwrap().unwrap();
        assert_eq!(s.iter().collect::<Vec<_>>(), [1, 2, 3]);
        let l = t.strs(15).unwrap().unwrap();
        assert_eq!((l.get(0), l.get(1), l.get(2)), (Ok("a"), Ok(""), Ok("ccc")));
    }

    #[test]
    fn old_bytes_survive_pool_reuse() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        b.root(FL).set_str(11, "first");
        let m1 = b.finish().unwrap();
        let snapshot = m1.to_vec();
        let mut b = Builder::new(&pool);
        b.root(FL).set_str(11, "SECOND-LONGER-MESSAGE");
        let m2 = b.finish().unwrap();
        assert_eq!(&m1[..], &snapshot[..]);
        assert_eq!(TableRef::at(&m1, 0).unwrap().str(11).unwrap(), Some("first"));
        assert_eq!(TableRef::at(&m2, 0).unwrap().str(11).unwrap(), Some("SECOND-LONGER-MESSAGE"));
    }

    pub(crate) struct CountingPool {
        pub takes: Cell<usize>,
        pub gives: Cell<usize>,
    }
    impl CountingPool {
        pub fn new() -> Self {
            CountingPool { takes: Cell::new(0), gives: Cell::new(0) }
        }
    }
    impl BufPool for CountingPool {
        fn take(&self) -> BytesMut {
            self.takes.set(self.takes.get() + 1);
            BytesMut::with_capacity(64)
        }
        fn give(&self, _b: BytesMut) {
            self.gives.set(self.gives.get() + 1);
        }
    }

    #[test]
    fn dropped_builder_returns_buffer() {
        let pool = CountingPool::new();
        {
            let mut b = Builder::new(&pool);
            b.root(FL).set::<u64>(1, B0, 1);
        }
        assert_eq!(pool.takes.get(), 1);
        assert_eq!(pool.gives.get(), 1);
        let b = Builder::new(&pool);
        let _ = b.finish();
        assert_eq!(pool.gives.get(), 2);
    }

    // ---- Task 4: lists of tables ----
    // element layout: [bits @0][u32 v @1][ptr memo @5][ptr child @9][union @13][ptr inner list @19] = 23
    const EFL: u16 = 23;
    const EPTRS: &[u16] = &[5, 9, 15, 19];

    fn read_elem(e: TableRef<'_>) -> (u32, Option<String>) {
        (e.scalar::<u32>(1, B0).unwrap().unwrap(), e.str(5).unwrap().map(String::from))
    }

    #[test]
    fn known_count_list_roundtrip() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            let mut l = t.tables(11, 3, EFL);
            for i in 0..3u32 {
                let mut e = l.push();
                e.set::<u32>(1, B0, i);
                e.set_str(5, &format!("m{i}"));
            }
        }
        let msg = b.finish().unwrap();
        let l = TableRef::at(&msg, 0).unwrap().tables(11).unwrap().unwrap();
        let got: Vec<_> = l.iter().map(|e| read_elem(e.unwrap())).collect();
        assert_eq!(got, [(0, Some("m0".into())), (1, Some("m1".into())), (2, Some("m2".into()))]);
    }

    #[test]
    fn known_count_closed_short_fixes_count() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            let mut l = t.tables(11, 5, EFL);
            l.push().set::<u32>(1, B0, 1);
            l.push().set::<u32>(1, B0, 2);
        }
        let msg = b.finish().unwrap();
        let l = TableRef::at(&msg, 0).unwrap().tables(11).unwrap().unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(read_elem(l.get(1).unwrap()).0, 2);
    }

    #[test]
    fn streaming_list_with_strings_offsets_fixed() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            t.set::<u64>(1, B0, 42);
            let mut l = t.tables_streaming(11, EFL, EPTRS);
            for i in 0..10u32 {
                let mut e = l.push();
                e.set::<u32>(1, B0, i * 10);
                if i % 3 != 0 {
                    e.set_str(5, &format!("memo-{i}"));
                }
            }
            l.close();
            t.set_str(15, "later");
        }
        let msg = b.finish().unwrap();
        let t = TableRef::at(&msg, 0).unwrap();
        assert_eq!(t.str(15).unwrap(), Some("later"));
        let l = t.tables(11).unwrap().unwrap();
        assert_eq!(l.len(), 10);
        for (i, e) in l.iter().enumerate() {
            let (v, memo) = read_elem(e.unwrap());
            assert_eq!(v, i as u32 * 10);
            let want = if i % 3 != 0 { Some(format!("memo-{i}")) } else { None };
            assert_eq!(memo, want);
        }
    }

    #[test]
    fn streaming_elem_with_nested_table_and_union() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            let mut l = t.tables_streaming(11, EFL, EPTRS);
            for i in 0..4u32 {
                let mut e = l.push();
                e.set_str(5, "m");
                e.table(9, 5).set::<u32>(1, B0, 100 + i);
                e.union(13, 2, 5).set::<u32>(1, B0, 200 + i);
            }
        }
        let msg = b.finish().unwrap();
        let l = TableRef::at(&msg, 0).unwrap().tables(11).unwrap().unwrap();
        for (i, e) in l.iter().enumerate() {
            let e = e.unwrap();
            assert_eq!(e.str(5).unwrap(), Some("m"));
            assert_eq!(e.table(9).unwrap().unwrap().scalar::<u32>(1, B0).unwrap(), Some(100 + i as u32));
            let (tag, u) = e.union(13).unwrap().unwrap();
            assert_eq!(tag, 2);
            assert_eq!(u.scalar::<u32>(1, B0).unwrap(), Some(200 + i as u32));
        }
    }

    #[test]
    fn streaming_inside_streaming() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            let mut outer = t.tables_streaming(11, EFL, EPTRS);
            for i in 0..3u32 {
                let mut e = outer.push();
                e.set::<u32>(1, B0, i);
                let mut inner = e.tables_streaming(19, EFL, EPTRS);
                for j in 0..=i {
                    let mut ie = inner.push();
                    ie.set::<u32>(1, B0, i * 10 + j);
                    ie.set_str(5, &format!("{i}.{j}"));
                }
                inner.close();
                e.set_str(5, &format!("o{i}"));
            }
        }
        let msg = b.finish().unwrap();
        let outer = TableRef::at(&msg, 0).unwrap().tables(11).unwrap().unwrap();
        assert_eq!(outer.len(), 3);
        for (i, e) in outer.iter().enumerate() {
            let e = e.unwrap();
            assert_eq!(read_elem(e), (i as u32, Some(format!("o{i}"))));
            let inner = e.tables(19).unwrap().unwrap();
            assert_eq!(inner.len(), i + 1);
            for (j, ie) in inner.iter().enumerate() {
                assert_eq!(read_elem(ie.unwrap()), ((i * 10 + j) as u32, Some(format!("{i}.{j}"))));
            }
        }
    }

    #[test]
    fn streaming_list_dropped_without_close() {
        fn fill(t: &mut TableW<'_>) -> Result<(), ()> {
            let mut l = t.tables_streaming(11, EFL, EPTRS);
            for i in 0..5u32 {
                let mut e = l.push();
                e.set::<u32>(1, B0, i);
                e.set_str(5, "x");
                if i == 2 {
                    return Err(());
                }
            }
            Ok(())
        }
        let pool = CountingPool::new();
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            assert!(fill(&mut t).is_err());
        }
        let msg = b.finish().unwrap();
        assert_eq!(pool.takes.get(), 2);
        assert_eq!(pool.gives.get(), 2);
        let l = TableRef::at(&msg, 0).unwrap().tables(11).unwrap().unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(read_elem(l.get(2).unwrap()), (2, Some("x".into())));
    }

    #[test]
    fn streaming_scalars_roundtrip() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            let mut l = t.scalars_streaming::<u64>(11);
            for i in 0..7 {
                l.push(i * i);
            }
            drop(l);
            t.set_str(15, "fim");
        }
        let msg = b.finish().unwrap();
        let t = TableRef::at(&msg, 0).unwrap();
        let l = t.scalars::<u64>(11).unwrap().unwrap();
        assert_eq!(l.iter().collect::<Vec<_>>(), [0, 1, 4, 9, 16, 25, 36]);
        assert_eq!(t.str(15).unwrap(), Some("fim"));
    }

    #[test]
    fn empty_streaming_list_is_present() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        b.root(FL).tables_streaming(11, EFL, EPTRS).close();
        let msg = b.finish().unwrap();
        assert_eq!(TableRef::at(&msg, 0).unwrap().tables(11).unwrap().unwrap().len(), 0);
    }

    // ---- FixedW: all-scalar tables with compile-time size ----
    #[test]
    fn fixed_writer_in_known_list_and_root() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            let mut l = t.tables(11, 2, 5);
            for i in 0..2u32 {
                let mut e = l.push_fixed::<5>();
                e.set::<u32>(1, B0, 10 + i);
            }
        }
        let msg = b.finish().unwrap();
        let l = TableRef::at(&msg, 0).unwrap().tables(11).unwrap().unwrap();
        assert_eq!(l.get(1).unwrap().scalar::<u32>(1, B0).unwrap(), Some(11));

        let mut b = Builder::new(&pool);
        {
            let mut r = b.root_fixed::<9>();
            r.set::<u64>(1, B0, 7);
            r.set_bool(B1, true);
        }
        let msg = b.finish().unwrap();
        let t = TableRef::at(&msg, 0).unwrap();
        assert_eq!(t.fixed_len(), 9);
        assert_eq!(t.scalar::<u64>(1, B0).unwrap(), Some(7));
        assert_eq!(t.bool(B1), Some(true));
    }

    #[test]
    fn fixed_writer_in_streaming_list() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            let mut l = t.tables_streaming(11, 5, &[]);
            for i in 0..3u32 {
                l.push_fixed::<5>().set::<u32>(1, B0, i);
            }
        }
        let msg = b.finish().unwrap();
        let l = TableRef::at(&msg, 0).unwrap().tables(11).unwrap().unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(l.get(2).unwrap().scalar::<u32>(1, B0).unwrap(), Some(2));
    }

    // ---- borrowed finish + fixed-element lists ----
    #[test]
    fn finished_borrows_current_message_and_next_root_resets() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        b.root(FL).set::<u64>(1, B0, 1);
        let first = b.finished().unwrap().to_vec();
        assert_eq!(TableRef::at(&first, 0).unwrap().scalar::<u64>(1, B0).unwrap(), Some(1));
        b.root(FL).set::<u64>(1, B0, 2);
        let second = b.finished().unwrap();
        assert_eq!(second.len(), first.len());
        assert_eq!(TableRef::at(second, 0).unwrap().scalar::<u64>(1, B0).unwrap(), Some(2));
    }

    #[test]
    fn fixed_list_roundtrip_and_short_count() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        {
            let mut t = b.root(FL);
            let mut l = t.tables_fixed::<5>(11, 4);
            for i in 0..3u32 {
                l.push().set::<u32>(1, B0, 100 + i);
            }
        }
        let msg = b.finish().unwrap();
        let l = TableRef::at(&msg, 0).unwrap().tables(11).unwrap().unwrap();
        assert_eq!((l.len(), l.elem_fixed_len()), (3, 5));
        assert_eq!(l.get(2).unwrap().scalar::<u32>(1, B0).unwrap(), Some(102));
    }

    #[test]
    #[should_panic(expected = "list too large")]
    fn list_end_overflow_panics() {
        // On 64-bit targets the buffer never gets close; the arithmetic is the same that overflows on 32 bits.
        let _ = list_end(usize::MAX - 3, 8);
    }

    #[test]
    fn list_end_in_range() {
        assert_eq!(list_end(6, 10), 16);
    }

    #[test]
    #[should_panic(expected = "more elements than reserved")]
    fn fixed_list_overflow_panics() {
        let pool = SimplePool::new(64);
        let mut b = Builder::new(&pool);
        let mut t = b.root(FL);
        let mut l = t.tables_fixed::<5>(11, 1);
        l.push();
        l.push();
    }

    // ---- length-prefixed frame batches ----
    #[test]
    fn sealed_frames_roundtrip_in_one_bytes() {
        let pool = SimplePool::new(256);
        let mut b = Builder::new(&pool);
        for i in 0..3u64 {
            let mut t = b.root(FL);
            t.set::<u64>(1, B0, i);
            t.set_str(11, &"x".repeat(i as usize));
            b.seal().unwrap();
        }
        let total: usize = (0..3).map(|i| 4 + 2 + FL as usize + 4 + i).sum();
        assert_eq!(b.sealed_len(), total);
        let batch = b.take_frames();
        assert_eq!(b.sealed_len(), 0);
        let msgs: Vec<&[u8]> = crate::frames(&batch).map(|f| f.unwrap()).collect();
        assert_eq!(msgs.len(), 3);
        for (i, m) in msgs.iter().enumerate() {
            let t = TableRef::at(m, 0).unwrap();
            assert_eq!(t.scalar::<u64>(1, B0).unwrap(), Some(i as u64));
            assert_eq!(t.str(11).unwrap().unwrap().len(), i);
        }
        // the builder is empty again and keeps working
        b.root(FL).set::<u64>(1, B0, 9);
        b.seal().unwrap();
        let next = b.take_frames();
        assert_eq!(crate::frames(&next).count(), 1);
        assert_eq!(crate::frames(&batch).count(), 3, "earlier batch intact");
    }

    #[test]
    fn unsealed_message_is_discarded_by_next_root() {
        let pool = SimplePool::new(256);
        let mut b = Builder::new(&pool);
        b.root(FL).set::<u64>(1, B0, 1);
        b.seal().unwrap();
        b.root(FL).set::<u64>(1, B0, 2); // abandoned
        b.root(FL).set::<u64>(1, B0, 3);
        b.seal().unwrap();
        let batch = b.take_frames();
        let vals: Vec<_> = crate::frames(&batch)
            .map(|f| TableRef::at(f.unwrap(), 0).unwrap().scalar::<u64>(1, B0).unwrap().unwrap())
            .collect();
        assert_eq!(vals, [1, 3]);
    }

    #[test]
    fn take_frames_excludes_unsealed_tail() {
        let pool = SimplePool::new(256);
        let mut b = Builder::new(&pool);
        b.root(FL).set::<u64>(1, B0, 1);
        b.seal().unwrap();
        b.root(FL).set::<u64>(1, B0, 2); // not sealed
        let batch = b.take_frames();
        assert_eq!(crate::frames(&batch).count(), 1);
    }

    #[test]
    fn frames_iterator_rejects_truncated_input() {
        let mut v = 10u32.to_le_bytes().to_vec();
        v.extend_from_slice(&[0; 4]);
        let r: Vec<_> = crate::frames(&v).collect();
        assert_eq!(r, [Err(Error::OutOfBounds)]);
        assert_eq!(crate::frames(&[1, 0]).collect::<Vec<_>>(), [Err(Error::OutOfBounds)]);
        assert_eq!(crate::frames(&[]).count(), 0);
    }

    #[test]
    #[should_panic(expected = "sealed frames pending")]
    fn flush_with_pending_frames_panics() {
        let pool = SimplePool::new(256);
        let mut b = Builder::new(&pool);
        b.root(FL);
        b.seal().unwrap();
        b.root(FL);
        let _ = b.flush();
    }
}
