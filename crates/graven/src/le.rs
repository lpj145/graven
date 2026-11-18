/// Fixed-size little-endian value. `read_le`/`write_le` require a slice of exactly `SIZE` bytes.
pub trait Le: Copy + Default {
    const SIZE: usize;
    fn read_le(b: &[u8]) -> Self;
    fn write_le(self, out: &mut [u8]);
}

macro_rules! impl_le {
    ($($t:ty),*) => {$(
        impl Le for $t {
            const SIZE: usize = core::mem::size_of::<$t>();
            #[inline(always)]
            fn read_le(b: &[u8]) -> Self {
                let mut a = [0u8; core::mem::size_of::<$t>()];
                a.copy_from_slice(b);
                <$t>::from_le_bytes(a)
            }
            #[inline(always)]
            fn write_le(self, out: &mut [u8]) {
                out.copy_from_slice(&self.to_le_bytes());
            }
        }
    )*};
}

impl_le!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128, f32, f64);

/// A number whose in-memory form, on a little-endian target, is exactly its wire form: every
/// bit pattern is a valid value. Lets [`ScalarList::as_slice`](crate::ScalarList::as_slice)
/// hand out the elements as a slice.
///
/// # Safety
///
/// Implemented only for the primitive integers and floats, which have no padding and no
/// invalid bit patterns.
#[allow(unsafe_code)]
pub unsafe trait Plain: Le {}

macro_rules! impl_plain {
    ($($t:ty),*) => {$(
        // SAFETY: a primitive integer or float: no padding, every bit pattern valid.
        #[allow(unsafe_code)]
        unsafe impl Plain for $t {}
    )*};
}

impl_plain!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128, f32, f64);

/// 1 byte (`[bool]` elements, `bool` inside a struct). Spec §5.3 / §6.3: the writer writes
/// `00` or `01`; the reader reads `00` as false and any other value as true.
///
/// Not used for a `bool` **table field**: that one has no slot, only 2 bits (presence +
/// value, spec §4.4), written with `TableW::set_bool` / `FixedW::set_bool` and read with
/// [`TableRef::bool`](crate::TableRef::bool). This 1-byte form is only for struct fields
/// and `[bool]` elements.
impl Le for bool {
    const SIZE: usize = 1;
    #[inline(always)]
    fn read_le(b: &[u8]) -> Self {
        b[0] != 0
    }
    #[inline(always)]
    fn write_le(self, out: &mut [u8]) {
        out[0] = self as u8;
    }
}
