//! CRC32C (Castagnoli) used by the envelope trailer.

/// Reflected Castagnoli polynomial.
const POLY: u32 = 0x82F6_3B78;

/// Slicing-by-8 lookup tables, computed at compile time.
static TABLES: [[u32; 256]; 8] = build_tables();

const fn build_tables() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut i = 0;
    while i < 256 {
        let mut s = 1;
        while s < 8 {
            let prev = t[s - 1][i];
            t[s][i] = (prev >> 8) ^ t[0][(prev & 0xFF) as usize];
            s += 1;
        }
        i += 1;
    }
    t
}

/// Portable software CRC32C (slicing-by-8). Always available, including `no_std`.
#[doc(hidden)]
pub fn crc32c_sw(data: &[u8]) -> u32 {
    let t = &TABLES;
    let mut crc = !0u32;
    let mut chunks = data.chunks_exact(8);
    for c in &mut chunks {
        let lo = u32::from_le_bytes([c[0], c[1], c[2], c[3]]) ^ crc;
        let hi = u32::from_le_bytes([c[4], c[5], c[6], c[7]]);
        crc = t[7][(lo & 0xFF) as usize]
            ^ t[6][((lo >> 8) & 0xFF) as usize]
            ^ t[5][((lo >> 16) & 0xFF) as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[3][(hi & 0xFF) as usize]
            ^ t[2][((hi >> 8) & 0xFF) as usize]
            ^ t[1][((hi >> 16) & 0xFF) as usize]
            ^ t[0][(hi >> 24) as usize];
    }
    for &b in chunks.remainder() {
        crc = (crc >> 8) ^ t[0][((crc ^ b as u32) & 0xFF) as usize];
    }
    !crc
}

/// CRC32C of `data`. Hardware-accelerated with `std`, software otherwise.
#[cfg(feature = "std")]
pub fn crc32c(data: &[u8]) -> u32 {
    ::crc32c::crc32c(data)
}

/// CRC32C of `data`. Hardware-accelerated with `std`, software otherwise.
#[cfg(not(feature = "std"))]
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c_sw(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn check_value() {
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c_sw(b"123456789"), 0xE306_9283);
    }

    #[test]
    fn empty_is_zero() {
        assert_eq!(crc32c(b""), 0);
        assert_eq!(crc32c_sw(b""), 0);
    }

    // Without `std`, `crc32c` *is* `crc32c_sw`: the comparison only makes sense with the accelerated one.
    #[cfg(feature = "std")]
    proptest! {
        #[test]
        fn sw_matches_accel(v in proptest::collection::vec(any::<u8>(), 0..5000)) {
            prop_assert_eq!(crc32c_sw(&v), crc32c(&v));
        }
    }
}
