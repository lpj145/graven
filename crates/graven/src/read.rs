//! Zero-copy reading.

use crate::{Error, Le};

/// Location of a presence bit: byte offset within the fixed part and bit shift.
/// For `bool`, presence is `shift` and the value is `shift + 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bit {
    pub byte: u16,
    pub shift: u8,
}

/// A view over one table's fixed part. Never panics on malformed input.
#[derive(Debug, Clone, Copy)]
pub struct TableRef<'a> {
    buf: &'a [u8],
    /// The fixed part, pre-sliced so field reads need a single bounds check.
    fixed: &'a [u8],
    fixed_start: usize,
}

impl<'a> TableRef<'a> {
    /// Table with a `[u16 fixed_len]` header at `pos`.
    #[inline]
    pub fn at(buf: &'a [u8], pos: usize) -> Result<Self, Error> {
        let h = buf.get(pos..pos.checked_add(2).ok_or(Error::OutOfBounds)?).ok_or(Error::OutOfBounds)?;
        let fixed_len = u16::read_le(h);
        Self::inline(buf, pos + 2, fixed_len)
    }

    /// Header-less table (list element) whose fixed part starts at `fixed_start`.
    #[inline]
    pub fn inline(buf: &'a [u8], fixed_start: usize, fixed_len: u16) -> Result<Self, Error> {
        let end = fixed_start.checked_add(fixed_len as usize).ok_or(Error::OutOfBounds)?;
        let fixed = buf.get(fixed_start..end).ok_or(Error::OutOfBounds)?;
        Ok(TableRef { buf, fixed, fixed_start })
    }

    #[inline]
    pub fn buf(&self) -> &'a [u8] {
        self.buf
    }

    #[inline]
    pub fn fixed_len(&self) -> u16 {
        self.fixed.len() as u16
    }

    /// Absolute position of the fixed part's first byte.
    #[inline]
    pub fn fixed_start(&self) -> usize {
        self.fixed_start
    }

    /// The fixed part bytes (always in bounds by construction).
    #[inline]
    fn fixed(&self) -> &'a [u8] {
        self.fixed
    }

    /// One byte of the fixed part; outside `fixed_len` ⇒ 0.
    #[inline(always)]
    pub fn fixed_byte(&self, i: u16) -> u8 {
        self.fixed.get(i as usize).copied().unwrap_or(0)
    }

    /// Bit value; a bit byte outside `fixed_len` reads as `false` (absent).
    #[inline]
    pub fn bit(&self, b: Bit) -> bool {
        match self.fixed().get(b.byte as usize) {
            Some(v) => (v >> (b.shift & 7)) & 1 == 1,
            None => false,
        }
    }

    /// `Ok(true)` if the slot is fully inside `fixed_len`, `Ok(false)` if fully
    /// outside, `Err(Truncated)` if partially covered.
    #[inline]
    pub fn slot_state(&self, off: u16, size: u16) -> Result<bool, Error> {
        let (off, end) = (off as u32, off as u32 + size as u32);
        let fl = self.fixed.len() as u32;
        if end <= fl {
            Ok(true)
        } else if off >= fl {
            Ok(false)
        } else {
            Err(Error::Truncated)
        }
    }

    #[inline(always)]
    fn slot_bytes(&self, off: u16, size: usize) -> Option<&'a [u8]> {
        let o = off as usize;
        self.fixed.get(o..o + size)
    }

    /// Scalar with presence bit, following the field read rule.
    #[inline]
    pub fn scalar<T: Le>(&self, off: u16, presence: Bit) -> Result<Option<T>, Error> {
        if !self.slot_state(off, T::SIZE as u16)? || !self.bit(presence) {
            return Ok(None);
        }
        Ok(self.slot_bytes(off, T::SIZE).map(T::read_le))
    }

    /// Scalar for an already-validated required field; any anomaly yields `default`.
    #[inline]
    pub fn scalar_or<T: Le>(&self, off: u16, presence: Bit, default: T) -> T {
        match self.scalar(off, presence) {
            Ok(Some(v)) => v,
            _ => default,
        }
    }

    /// Slot value ignoring presence, for required fields already validated by
    /// the generated `read()`. Slot not fully inside `fixed_len` ⇒ `T::default()`.
    #[inline(always)]
    pub fn raw<T: Le>(&self, off: u16) -> T {
        match self.slot_bytes(off, T::SIZE) {
            Some(b) => T::read_le(b),
            None => T::default(),
        }
    }

    /// Bool stored as presence bit + value bit (`shift + 1`).
    #[inline]
    pub fn bool(&self, presence: Bit) -> Option<bool> {
        if !self.bit(presence) {
            return None;
        }
        Some(self.bit(Bit { byte: presence.byte, shift: presence.shift + 1 }))
    }
}

// ---------------------------------------------------------------------------
// Variable data
// ---------------------------------------------------------------------------

#[inline]
fn read_at<T: Le>(buf: &[u8], pos: usize) -> Result<T, Error> {
    let end = pos.checked_add(T::SIZE).ok_or(Error::OutOfBounds)?;
    buf.get(pos..end).map(T::read_le).ok_or(Error::OutOfBounds)
}

/// Resolve the i32 pointer stored at absolute `slot_abs`. `0` ⇒ `None`.
#[inline]
fn resolve(buf: &[u8], slot_abs: usize) -> Result<Option<usize>, Error> {
    let rel = read_at::<i32>(buf, slot_abs)?;
    if rel == 0 {
        return Ok(None);
    }
    let target = slot_abs as i64 + rel as i64;
    if target < 0 || target >= buf.len() as i64 {
        return Err(Error::BadOffset);
    }
    Ok(Some(target as usize))
}

/// `[u32 len][len * elem bytes]` at `pos` ⇒ (len, data slice start).
#[inline]
fn counted(buf: &[u8], pos: usize, elem: usize) -> Result<(usize, usize), Error> {
    let len = read_at::<u32>(buf, pos)? as usize;
    let data = pos + 4;
    let bytes = len.checked_mul(elem).ok_or(Error::OutOfBounds)?;
    let end = data.checked_add(bytes).ok_or(Error::OutOfBounds)?;
    if end > buf.len() {
        return Err(Error::OutOfBounds);
    }
    Ok((len, data))
}

#[inline]
fn bytes_at(buf: &[u8], pos: usize) -> Result<&[u8], Error> {
    let (len, data) = counted(buf, pos, 1)?;
    Ok(&buf[data..data + len])
}

#[inline]
fn str_at(buf: &[u8], pos: usize) -> Result<&str, Error> {
    core::str::from_utf8(bytes_at(buf, pos)?).map_err(|_| Error::InvalidUtf8)
}

impl<'a> TableRef<'a> {
    /// Absolute target of the pointer at slot `off`. Slot outside `fixed_len` or `0` ⇒ `None`.
    #[inline]
    pub fn ptr(&self, off: u16) -> Result<Option<usize>, Error> {
        if !self.slot_state(off, 4)? {
            return Ok(None);
        }
        resolve(self.buf, self.fixed_start + off as usize)
    }

    #[inline]
    pub fn bytes(&self, off: u16) -> Result<Option<&'a [u8]>, Error> {
        match self.ptr(off)? {
            Some(p) => bytes_at(self.buf, p).map(Some),
            None => Ok(None),
        }
    }

    #[inline]
    pub fn str(&self, off: u16) -> Result<Option<&'a str>, Error> {
        match self.ptr(off)? {
            Some(p) => str_at(self.buf, p).map(Some),
            None => Ok(None),
        }
    }

    /// Support for generated `*Verified` views; not part of the public API. See
    /// [`crate::rt`] ("Invariants for the code generator") for the pattern that calls it.
    ///
    /// # Safety
    /// The caller guarantees the string bytes at `off` are valid UTF-8. In generated
    /// `*Verified` views this holds only if all of the following hold:
    ///
    /// 1. `self` was reached by a root `verify()` that succeeded on the same (immutable)
    ///    buffer, along the **same edge** and as the **same reader type** whose view makes
    ///    this call (the same bytes can be reached through two pointers as different table
    ///    types; verify only validated the type of the edge it followed);
    /// 2. that type's `verify` calls the validating [`str`](Self::str) at this same `off`
    ///    (not merely charging `bytes(off).len()`);
    /// 3. the view never reaches `self` through an edge verify skips (unknown union tags,
    ///    fields beyond the reader's layout, reserved fields);
    /// 4. any `VListR<T>` on the path was built with the `T` that verify used for that list.
    #[doc(hidden)]
    #[allow(unsafe_code)]
    #[inline]
    pub unsafe fn str_unchecked(&self, off: u16) -> Result<Option<&'a str>, Error> {
        match self.bytes(off)? {
            // SAFETY: the caller guarantees `b` is valid UTF-8 (function contract above).
            Some(b) => Ok(Some(unsafe { core::str::from_utf8_unchecked(b) })),
            None => Ok(None),
        }
    }

    #[inline]
    pub fn table(&self, off: u16) -> Result<Option<TableRef<'a>>, Error> {
        match self.ptr(off)? {
            Some(p) => TableRef::at(self.buf, p).map(Some),
            None => Ok(None),
        }
    }

    /// Union slot `[u16 tag][i32 ptr]`; tag 0 ⇒ `None`.
    #[inline]
    pub fn union(&self, off: u16) -> Result<Option<(u16, TableRef<'a>)>, Error> {
        if !self.slot_state(off, 6)? {
            return Ok(None);
        }
        let tag = read_at::<u16>(self.buf, self.fixed_start + off as usize)?;
        if tag == 0 {
            return Ok(None);
        }
        match resolve(self.buf, self.fixed_start + off as usize + 2)? {
            Some(p) => Ok(Some((tag, TableRef::at(self.buf, p)?))),
            None => Ok(None),
        }
    }

    #[inline]
    pub fn scalars<T: Le>(&self, off: u16) -> Result<Option<ScalarList<'a, T>>, Error> {
        match self.ptr(off)? {
            Some(p) => ScalarList::at(self.buf, p).map(Some),
            None => Ok(None),
        }
    }

    #[inline]
    pub fn tables(&self, off: u16) -> Result<Option<TableList<'a>>, Error> {
        match self.ptr(off)? {
            Some(p) => TableList::at(self.buf, p).map(Some),
            None => Ok(None),
        }
    }

    #[inline]
    pub fn strs(&self, off: u16) -> Result<Option<StrList<'a>>, Error> {
        match self.ptr(off)? {
            Some(p) => {
                let (len, data) = counted(self.buf, p, 4)?;
                Ok(Some(StrList { buf: self.buf, slots: data, len }))
            }
            None => Ok(None),
        }
    }
}

/// Split a batch produced by `Builder::take_frames` into messages.
/// Each frame is `[u32 len][message]`; truncated input yields one
/// `Err(OutOfBounds)` and stops. Never panics.
pub fn frames(buf: &[u8]) -> Frames<'_> {
    Frames { buf, failed: false }
}

pub struct Frames<'a> {
    buf: &'a [u8],
    failed: bool,
}

impl<'a> Iterator for Frames<'a> {
    type Item = Result<&'a [u8], Error>;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.buf.is_empty() || self.failed {
            return None;
        }
        let msg = (|| {
            let len = read_at::<u32>(self.buf, 0)? as usize;
            let end = 4usize.checked_add(len).ok_or(Error::OutOfBounds)?;
            let m = self.buf.get(4..end).ok_or(Error::OutOfBounds)?;
            Ok((m, end))
        })();
        match msg {
            Ok((m, end)) => {
                self.buf = &self.buf[end..];
                Some(Ok(m))
            }
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}

/// `[u32 count][count × T]`
#[derive(Debug, Clone, Copy)]
pub struct ScalarList<'a, T: Le> {
    data: &'a [u8],
    len: usize,
    _t: core::marker::PhantomData<T>,
}

impl<'a, T: Le> ScalarList<'a, T> {
    pub fn at(buf: &'a [u8], pos: usize) -> Result<Self, Error> {
        let (len, data) = counted(buf, pos, T::SIZE)?;
        Ok(ScalarList { data: &buf[data..data + len * T::SIZE], len, _t: core::marker::PhantomData })
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    #[inline]
    pub fn get(&self, i: usize) -> Option<T> {
        if i >= self.len {
            return None;
        }
        self.data.get(i * T::SIZE..(i + 1) * T::SIZE).map(T::read_le)
    }
    /// Raw little-endian bytes of all elements.
    #[inline]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = T> + 'a
    where
        T: 'a,
    {
        self.data.chunks_exact(T::SIZE).map(T::read_le)
    }
}

impl<'a, T: crate::le::Plain> ScalarList<'a, T> {
    /// The elements as a slice, without copying: `Some` when they are aligned in memory for
    /// `T` and the target is little-endian, `None` otherwise (then use [`iter`](Self::iter)
    /// or [`get`](Self::get)). A list written with `#[align(to = N)]`, in a container that
    /// lies at an aligned address, is always aligned.
    #[allow(unsafe_code)]
    #[inline]
    pub fn as_slice(&self) -> Option<&'a [T]> {
        if cfg!(target_endian = "big") {
            return None;
        }
        let ptr = self.data.as_ptr();
        if !(ptr as usize).is_multiple_of(core::mem::align_of::<T>()) {
            return None;
        }
        // SAFETY: `data` holds exactly `len * size_of::<T>()` bytes, borrowed for `'a`; `ptr`
        // is aligned for `T` (checked above); `T: Plain` makes every bit pattern a valid `T`
        // with no padding, and on a little-endian target its memory form is the wire form.
        Some(unsafe { core::slice::from_raw_parts(ptr.cast::<T>(), self.len) })
    }
}

/// `[u32 count][u16 elem_fixed_len][count × fixed part]`
#[derive(Debug, Clone, Copy)]
pub struct TableList<'a> {
    buf: &'a [u8],
    first: usize,
    len: usize,
    elem_fixed_len: u16,
}

impl<'a> TableList<'a> {
    pub fn at(buf: &'a [u8], pos: usize) -> Result<Self, Error> {
        let len = read_at::<u32>(buf, pos)? as usize;
        let elem_fixed_len = read_at::<u16>(buf, pos.checked_add(4).ok_or(Error::OutOfBounds)?)?;
        let first = pos + 6;
        let bytes = len.checked_mul(elem_fixed_len as usize).ok_or(Error::OutOfBounds)?;
        let end = first.checked_add(bytes).ok_or(Error::OutOfBounds)?;
        if end > buf.len() {
            return Err(Error::OutOfBounds);
        }
        Ok(TableList { buf, first, len, elem_fixed_len })
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    #[inline]
    pub fn elem_fixed_len(&self) -> u16 {
        self.elem_fixed_len
    }
    #[inline]
    pub fn get(&self, i: usize) -> Result<TableRef<'a>, Error> {
        if i >= self.len {
            return Err(Error::OutOfBounds);
        }
        TableRef::inline(self.buf, self.first + i * self.elem_fixed_len as usize, self.elem_fixed_len)
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = Result<TableRef<'a>, Error>> + 'a {
        let s = *self;
        (0..s.len).map(move |i| s.get(i))
    }
}

/// `[u32 count][count × i32 ptr]` to strings (or bytes).
#[derive(Debug, Clone, Copy)]
pub struct StrList<'a> {
    buf: &'a [u8],
    slots: usize,
    len: usize,
}

impl<'a> StrList<'a> {
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn get_bytes(&self, i: usize) -> Result<&'a [u8], Error> {
        if i >= self.len {
            return Err(Error::OutOfBounds);
        }
        match resolve(self.buf, self.slots + i * 4)? {
            Some(p) => bytes_at(self.buf, p),
            None => Ok(&[]),
        }
    }
    pub fn get(&self, i: usize) -> Result<&'a str, Error> {
        core::str::from_utf8(self.get_bytes(i)?).map_err(|_| Error::InvalidUtf8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, Le};

    // fixed_len=7: [bits][u32 a @1][u16 b @5]; a=bit0, b=bit1, bool=bits2/3
    fn sample() -> [u8; 9] {
        [7, 0, 0b0000_1101, 42, 0, 0, 0, 0, 0]
    }

    #[test]
    fn reads_present_scalar() {
        let b = sample();
        let t = TableRef::at(&b, 0).unwrap();
        assert_eq!(t.scalar::<u32>(1, Bit { byte: 0, shift: 0 }).unwrap(), Some(42));
    }

    #[test]
    fn absent_bit_is_none() {
        let b = sample();
        let t = TableRef::at(&b, 0).unwrap();
        assert_eq!(t.scalar::<u16>(5, Bit { byte: 0, shift: 1 }).unwrap(), None);
    }

    #[test]
    fn bool_two_bits() {
        let b = sample();
        let t = TableRef::at(&b, 0).unwrap();
        assert_eq!(t.bool(Bit { byte: 0, shift: 2 }), Some(true));
        assert_eq!(t.bool(Bit { byte: 0, shift: 4 }), None);
    }

    #[test]
    fn slot_beyond_fixed_len_is_none() {
        let b = sample();
        let t = TableRef::at(&b, 0).unwrap();
        assert_eq!(t.scalar::<u32>(7, Bit { byte: 0, shift: 4 }).unwrap(), None);
    }

    #[test]
    fn bit_byte_beyond_fixed_len_is_none() {
        let b = sample();
        let t = TableRef::at(&b, 0).unwrap();
        assert!(!t.bit(Bit { byte: 9, shift: 0 }));
        assert_eq!(t.scalar::<u8>(1, Bit { byte: 9, shift: 0 }).unwrap(), None);
    }

    #[test]
    fn partial_slot_is_truncated() {
        let b = sample();
        let t = TableRef::at(&b, 0).unwrap();
        assert_eq!(t.slot_state(5, 4), Err(Error::Truncated));
        assert_eq!(t.scalar::<u32>(5, Bit { byte: 0, shift: 1 }), Err(Error::Truncated));
    }

    #[test]
    fn header_past_end_is_err() {
        assert_eq!(TableRef::at(&[9, 0, 1], 0).err(), Some(Error::OutOfBounds));
        assert_eq!(TableRef::at(&[9], 0).err(), Some(Error::OutOfBounds));
        assert_eq!(TableRef::at(&[0, 0], 5).err(), Some(Error::OutOfBounds));
    }

    #[test]
    fn scalar_or_never_panics() {
        let b = sample();
        let t = TableRef::at(&b, 0).unwrap();
        assert_eq!(t.scalar_or::<u32>(1, Bit { byte: 0, shift: 0 }, 9), 42);
        assert_eq!(t.scalar_or::<u32>(6, Bit { byte: 0, shift: 0 }, 9), 9);
    }

    fn rt<T: Le + PartialEq + core::fmt::Debug>(v: T) {
        let mut buf = [0u8; 16];
        v.write_le(&mut buf[..T::SIZE]);
        assert_eq!(T::read_le(&buf[..T::SIZE]), v);
    }

    #[test]
    fn le_roundtrip_all_types() {
        rt(0xABu8);
        rt(0xABCDu16);
        rt(0xDEADBEEFu32);
        rt(u64::MAX - 7);
        rt(u128::MAX - 3);
        rt(-5i8);
        rt(i16::MIN);
        rt(i32::MIN);
        rt(i64::MIN);
        rt(i128::MIN);
        rt(1.5f32);
        rt(-2.25f64);
        let mut buf = [0u8; 8];
        f64::NAN.write_le(&mut buf);
        assert_eq!(f64::read_le(&buf).to_bits(), f64::NAN.to_bits());
        let mut b4 = [0u8; 4];
        0x01020304u32.write_le(&mut b4);
        assert_eq!(b4, [4, 3, 2, 1]);
    }

    // ---- Task 2: variable data ----

    /// Table with fixed_len=4 holding one i32 ptr at slot 0 (abs pos 2), followed by `tail`.
    fn table_with_ptr(rel: i32, tail: &[u8]) -> alloc::vec::Vec<u8> {
        let mut v = alloc::vec![4, 0];
        v.extend_from_slice(&rel.to_le_bytes());
        v.extend_from_slice(tail);
        v
    }

    #[test]
    fn string_forward_offset() {
        // slot abs 2, tail starts at 6 => rel 4
        let b = table_with_ptr(4, &[2, 0, 0, 0, b'o', b'i']);
        let t = TableRef::at(&b, 0).unwrap();
        assert_eq!(t.str(0).unwrap(), Some("oi"));
        assert_eq!(t.bytes(0).unwrap(), Some(&b"oi"[..]));
    }

    #[test]
    fn negative_offset_into_buffer() {
        // string at pos 0, table at pos 6; slot abs 8 => rel -8
        let mut b = alloc::vec![2, 0, 0, 0, b'h', b'i'];
        b.extend_from_slice(&[4, 0]);
        b.extend_from_slice(&(-8i32).to_le_bytes());
        let t = TableRef::at(&b, 6).unwrap();
        assert_eq!(t.str(0).unwrap(), Some("hi"));
    }

    #[test]
    fn offset_before_zero_is_bad_offset() {
        let b = table_with_ptr(-3, &[]);
        assert_eq!(TableRef::at(&b, 0).unwrap().ptr(0), Err(Error::BadOffset));
        let b = table_with_ptr(i32::MIN, &[]);
        assert_eq!(TableRef::at(&b, 0).unwrap().ptr(0), Err(Error::BadOffset));
    }

    #[test]
    fn offset_past_end_is_bad_offset() {
        let b = table_with_ptr(4, &[]);
        assert_eq!(TableRef::at(&b, 0).unwrap().ptr(0), Err(Error::BadOffset));
        let b = table_with_ptr(i32::MAX, &[]);
        assert_eq!(TableRef::at(&b, 0).unwrap().str(0), Err(Error::BadOffset));
    }

    #[test]
    fn len_u32_max_is_out_of_bounds() {
        let b = table_with_ptr(4, &u32::MAX.to_le_bytes());
        let t = TableRef::at(&b, 0).unwrap();
        assert_eq!(t.bytes(0), Err(Error::OutOfBounds));
        assert_eq!(t.scalars::<u64>(0).err(), Some(Error::OutOfBounds));
        assert_eq!(t.tables(0).err(), Some(Error::OutOfBounds));
        assert_eq!(t.strs(0).err(), Some(Error::OutOfBounds));
    }

    #[test]
    fn invalid_utf8_is_err() {
        let b = table_with_ptr(4, &[1, 0, 0, 0, 0xFF]);
        assert_eq!(TableRef::at(&b, 0).unwrap().str(0), Err(Error::InvalidUtf8));
    }

    #[test]
    fn zero_offset_is_none() {
        let b = table_with_ptr(0, &[]);
        let t = TableRef::at(&b, 0).unwrap();
        assert_eq!(t.ptr(0), Ok(None));
        assert_eq!(t.str(0), Ok(None));
        assert!(t.table(0).unwrap().is_none());
        assert!(t.tables(0).unwrap().is_none());
        // slot outside fixed_len also None
        assert_eq!(t.ptr(4), Ok(None));
        assert_eq!(t.ptr(2), Err(Error::Truncated));
    }

    #[test]
    fn table_list_stride_uses_elem_fixed_len() {
        // [u32 count=2][u16 elem_fixed_len=5][e0: bits=1, u32=10][e1: bits=1, u32=20]
        let mut tail = alloc::vec![2, 0, 0, 0, 5, 0];
        tail.extend_from_slice(&[1, 10, 0, 0, 0, 1, 20, 0, 0, 0]);
        let b = table_with_ptr(4, &tail);
        let l = TableRef::at(&b, 0).unwrap().tables(0).unwrap().unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l.elem_fixed_len(), 5);
        let p = Bit { byte: 0, shift: 0 };
        assert_eq!(l.get(0).unwrap().scalar::<u32>(1, p).unwrap(), Some(10));
        assert_eq!(l.get(1).unwrap().scalar::<u32>(1, p).unwrap(), Some(20));
        assert!(l.get(2).is_err());
        assert_eq!(l.iter().count(), 2);
    }

    #[test]
    fn scalar_list_reads() {
        let mut tail = alloc::vec![3, 0, 0, 0];
        for v in [7u16, 8, 9] {
            tail.extend_from_slice(&v.to_le_bytes());
        }
        let b = table_with_ptr(4, &tail);
        let l = TableRef::at(&b, 0).unwrap().scalars::<u16>(0).unwrap().unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(l.get(1), Some(8));
        assert_eq!(l.get(3), None);
        assert_eq!(l.iter().collect::<alloc::vec::Vec<_>>(), [7, 8, 9]);
    }

    #[test]
    fn str_list_reads() {
        // [count=2][rel0][rel1]["a"]["bc"]; offsets slots at tail+4 and tail+8
        // positions: tail starts at 6; slot0 at 10, slot1 at 14; s0 at 18, s1 at 23
        let mut tail = alloc::vec![2, 0, 0, 0];
        tail.extend_from_slice(&8i32.to_le_bytes());
        tail.extend_from_slice(&9i32.to_le_bytes());
        tail.extend_from_slice(&[1, 0, 0, 0, b'a', 2, 0, 0, 0, b'b', b'c']);
        let b = table_with_ptr(4, &tail);
        let l = TableRef::at(&b, 0).unwrap().strs(0).unwrap().unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l.get(0), Ok("a"));
        assert_eq!(l.get(1), Ok("bc"));
    }

    #[test]
    fn union_unknown_tag_still_returns_tag() {
        // fixed_len=6: [u16 tag=99][i32 rel] ; slot ptr abs pos 4 -> child at 8 => rel 4
        let mut b = alloc::vec![6, 0, 99, 0];
        b.extend_from_slice(&4i32.to_le_bytes());
        b.extend_from_slice(&[0, 0]); // empty child table
        let t = TableRef::at(&b, 0).unwrap();
        let (tag, child) = t.union(0).unwrap().unwrap();
        assert_eq!(tag, 99);
        assert_eq!(child.fixed_len(), 0);
        let none = [6u8, 0, 0, 0, 0, 0, 0, 0];
        assert!(TableRef::at(&none, 0).unwrap().union(0).unwrap().is_none());
    }

    #[test]
    fn nested_table() {
        let b = table_with_ptr(4, &[1, 0, 0]);
        let c = TableRef::at(&b, 0).unwrap().table(0).unwrap().unwrap();
        assert_eq!(c.fixed_len(), 1);
    }

    #[test]
    fn raw_reads_slot_without_presence_and_never_panics() {
        let b = sample();
        let t = TableRef::at(&b, 0).unwrap();
        assert_eq!(t.raw::<u32>(1), 42);
        assert_eq!(t.raw::<u32>(5), 0); // partial -> default
        assert_eq!(t.raw::<u64>(100), 0);
    }
}
