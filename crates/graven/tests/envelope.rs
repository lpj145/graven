//! Envelope encode/decode: layout, truncation, corruption, and fuzz-style properties.
use bytes::BytesMut;
use proptest::prelude::*;
use graven::Error;
use graven::envelope::{HEADER_LEN, decode, encode, schema_id};

fn enc(id: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = BytesMut::new();
    encode(id, payload, &mut out).unwrap();
    out.to_vec()
}

#[test]
fn schema_id_known() {
    assert_eq!(schema_id(""), 0xcbf29ce484222325);
    assert_eq!(schema_id("a"), 0xaf63dc4c8601ec8c);
}

#[test]
fn roundtrip() {
    let e = enc(7, b"hello");
    let env = decode(&e).unwrap();
    assert_eq!(env.schema_id, 7);
    assert_eq!(env.payload, b"hello");
    assert_eq!(env.len(), 29);
}

#[test]
fn layout_bytes() {
    let e = enc(0x0102030405060708, b"");
    let mut want = b"GRVN\x01\0\0\0".to_vec();
    want.extend_from_slice(&[8, 7, 6, 5, 4, 3, 2, 1]);
    want.extend_from_slice(&[0, 0, 0, 0]);
    assert_eq!(&e[..HEADER_LEN], &want[..]);
    assert_eq!(e.len(), 24);
}

#[test]
fn encode_appends_to_existing_buffer() {
    let mut out = BytesMut::new();
    out.extend_from_slice(b"pre");
    encode(7, b"hello", &mut out).unwrap();
    assert_eq!(&out[..3], b"pre");
    assert_eq!(decode(&out[3..]).unwrap().payload, b"hello");
}

#[test]
fn truncated_everywhere() {
    let e = enc(7, b"hello");
    for k in 0..e.len() {
        let needed = if k < HEADER_LEN { 20 } else { 29 };
        assert_eq!(decode(&e[..k]), Err(Error::Incomplete { needed }), "k={k}");
    }
}

#[test]
fn bitflip_everywhere() {
    let e = enc(7, b"hello world");
    for byte in 0..e.len() {
        for bit in 0..8 {
            let mut m = e.clone();
            m[byte] ^= 1 << bit;
            let r = decode(&m);
            match byte {
                0..4 => assert_eq!(r, Err(Error::BadMagic), "byte={byte} bit={bit}"),
                4 => assert!(matches!(r, Err(Error::UnsupportedVersion(_))), "bit={bit}: {r:?}"),
                5..8 => assert_eq!(r, Err(Error::BadHeader), "byte={byte} bit={bit}"),
                16..20 => assert!(
                    matches!(r, Err(Error::ChecksumMismatch | Error::Incomplete { .. })),
                    "byte={byte} bit={bit}: {r:?}"
                ),
                _ => assert_eq!(r, Err(Error::ChecksumMismatch), "byte={byte} bit={bit}"),
            }
        }
    }
}

#[test]
fn trailing_bytes_ok() {
    let mut e = enc(7, b"hello");
    e.extend_from_slice(b"xyz");
    let env = decode(&e).unwrap();
    assert_eq!(env.payload, b"hello");
    assert_eq!(env.len(), 24 + 5);
}

#[test]
fn decode_huge_len_is_incomplete() {
    let mut e = enc(1, b"");
    e[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(e.len(), 24);
    let r = decode(&e);
    #[cfg(target_pointer_width = "64")]
    assert_eq!(r, Err(Error::Incomplete { needed: 20 + u32::MAX as usize + 4 }));
    #[cfg(not(target_pointer_width = "64"))]
    assert_eq!(r, Err(Error::Incomplete { needed: usize::MAX }));
}

#[test]
fn decode_empty_payload() {
    let e = enc(1, b"");
    let env = decode(&e).unwrap();
    assert!(env.payload.is_empty());
    assert_eq!(env.len(), 24);
}

#[test]
fn flags_and_reserved_nonzero_are_bad_header() {
    // Re-checksum so only the header check can reject.
    for (i, v) in [(5usize, 1u8), (6, 1), (7, 0x80)] {
        let mut e = enc(7, b"hi");
        e[i] = v;
        let n = e.len() - 4;
        let crc = graven::crc32c(&e[..n]);
        e[n..].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(decode(&e), Err(Error::BadHeader), "byte {i}");
    }
}

proptest! {
    #[test]
    fn decode_never_panics(v in proptest::collection::vec(any::<u8>(), 0..256)) {
        let _ = decode(&v);
    }

    #[test]
    fn roundtrip_any(id in any::<u64>(), p in proptest::collection::vec(any::<u8>(), 0..512)) {
        let e = enc(id, &p);
        let env = decode(&e).unwrap();
        prop_assert_eq!(env.schema_id, id);
        prop_assert_eq!(env.payload, &p[..]);
        prop_assert_eq!(env.len(), e.len());
    }

    #[test]
    fn decode_mutated_never_ok(
        p in proptest::collection::vec(any::<u8>(), 0..64),
        muts in proptest::collection::vec((any::<proptest::sample::Index>(), any::<u8>()), 1..4),
    ) {
        let e = enc(7, &p);
        let mut m = e.clone();
        for (idx, val) in &muts {
            let i = idx.index(m.len());
            m[i] = *val;
        }
        if let Ok(env) = decode(&m) {
            // Only acceptable if the mutation restored the original bytes.
            prop_assert_eq!(env.payload, &p[..]);
            prop_assert_eq!(&m[..env.len()], &e[..]);
        }
    }
}

// ---- Builder envelope output and `envelopes()` iteration ----

#[test]
fn envelopes_over_encoded_and_empty() {
    let mut out = BytesMut::new();
    encode(1, b"a", &mut out).unwrap();
    encode(2, b"", &mut out).unwrap();
    let got: Vec<_> = graven::envelope::envelopes(&out).map(|e| e.map(|e| (e.schema_id, e.payload))).collect();
    assert_eq!(got, [Ok((1, &b"a"[..])), Ok((2, &b""[..]))]);
    assert_eq!(graven::envelope::envelopes(&[]).count(), 0);
}

// `SimplePool` only exists with `std`.
#[cfg(feature = "std")]
mod builder {
    use super::*;
    use graven::envelope::envelopes;
    use graven::{Bit, Builder, SimplePool};

    fn raw_payload(v: u32) -> Vec<u8> {
        let pool = SimplePool::new(256);
        let mut b = Builder::new(&pool);
        b.root(6).set::<u32>(1, Bit { byte: 0, shift: 0 }, v);
        b.finish().unwrap().to_vec()
    }

    fn batch_of_three() -> (Vec<u8>, [Vec<u8>; 3]) {
        let pool = SimplePool::new(1024);
        let mut b = Builder::new_enveloped(&pool);
        for v in [10u32, 20, 30] {
            b.root(6).set::<u32>(1, Bit { byte: 0, shift: 0 }, v);
            b.seal_envelope(9).unwrap();
        }
        let batch = b.take_frames().to_vec();
        (batch, [raw_payload(10), raw_payload(20), raw_payload(30)])
    }

    #[test]
    fn builder_envelope_equals_encode() {
        let pool = SimplePool::new(256);
        let mut b = Builder::new_enveloped(&pool);
        b.root(6).set::<u32>(1, Bit { byte: 0, shift: 0 }, 0xdead_beef);
        let got = b.flush_envelope(9).unwrap();
        assert_eq!(&got[..], &enc(9, &raw_payload(0xdead_beef))[..]);
        // The builder keeps working after a flush.
        b.root(6).set::<u32>(1, Bit { byte: 0, shift: 0 }, 1);
        let got = b.flush_envelope(9).unwrap();
        assert_eq!(&got[..], &enc(9, &raw_payload(1))[..]);
    }

    #[test]
    fn batch_of_envelopes() {
        let (batch, payloads) = batch_of_three();
        let items: Vec<_> = envelopes(&batch).collect();
        assert_eq!(items.len(), 3);
        for (item, p) in items.iter().zip(&payloads) {
            let env = item.as_ref().unwrap();
            assert_eq!(env.schema_id, 9);
            assert_eq!(env.payload, &p[..]);
        }
    }

    #[test]
    fn envelopes_stop_at_corruption() {
        let (mut batch, _) = batch_of_three();
        let first = decode(&batch).unwrap().len();
        batch[first + HEADER_LEN] ^= 1; // first payload byte of the 2nd envelope
        let items: Vec<_> = envelopes(&batch).collect();
        assert_eq!(items.len(), 2);
        assert!(items[0].is_ok());
        assert_eq!(items[1], Err(Error::ChecksumMismatch));
    }

    #[test]
    fn envelopes_truncated_tail() {
        let (batch, _) = batch_of_three();
        let one = decode(&batch).unwrap().len();
        let cut = &batch[..2 * one + one / 2];
        let items: Vec<_> = envelopes(cut).collect();
        assert_eq!(items.len(), 3);
        assert!(items[0].is_ok() && items[1].is_ok());
        assert!(matches!(items[2], Err(Error::Incomplete { .. })), "{:?}", items[2]);
    }

    #[test]
    fn envelopes_empty_buffer() {
        assert_eq!(envelopes(&[]).count(), 0);
    }

    #[test]
    #[should_panic(expected = "builder mode mismatch")]
    fn seal_on_enveloped_panics() {
        let pool = SimplePool::new(256);
        let mut b = Builder::new_enveloped(&pool);
        b.root(6);
        let _ = b.seal();
    }

    #[test]
    #[should_panic(expected = "builder mode mismatch")]
    fn seal_envelope_on_normal_panics() {
        let pool = SimplePool::new(256);
        let mut b = Builder::new(&pool);
        b.root(6);
        let _ = b.seal_envelope(9);
    }

    #[test]
    #[should_panic(expected = "builder mode mismatch")]
    fn flush_envelope_on_normal_panics() {
        let pool = SimplePool::new(256);
        let mut b = Builder::new(&pool);
        b.root(6);
        let _ = b.flush_envelope(9);
    }

    #[test]
    fn flush_on_enveloped_returns_payload() {
        let pool = SimplePool::new(256);
        let mut b = Builder::new_enveloped(&pool);
        b.root(6).set::<u32>(1, Bit { byte: 0, shift: 0 }, 77);
        assert_eq!(b.finished().unwrap(), &raw_payload(77)[..]);
        let got = b.flush().unwrap();
        assert_eq!(&got[..], &raw_payload(77)[..]);
    }
}
