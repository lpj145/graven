#![no_main]
use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use graven::envelope::{HEADER_LEN, TRAILER_LEN, decode, encode, envelopes};

fuzz_target!(|data: &[u8]| {
    // decode never panics; what it consumes fits in the buffer.
    if let Ok(env) = decode(data) {
        assert!(env.len() <= data.len());
        assert_eq!(env.len(), HEADER_LEN + env.payload.len() + TRAILER_LEN);
        // And it is stable: re-encoding the payload with the same id gives the same bytes.
        let mut out = BytesMut::new();
        encode(env.schema_id, env.payload, &mut out).unwrap();
        assert_eq!(&out[..], &data[..env.len()]);
    }
    // The iterator stops at the first error and never passes the end.
    let mut seen_err = false;
    let mut total = 0usize;
    for r in envelopes(data) {
        assert!(!seen_err, "iterator continued after an error");
        match r {
            Ok(e) => total += e.len(),
            Err(_) => seen_err = true,
        }
    }
    assert!(total <= data.len());
    // Round trip with an arbitrary payload.
    let mut out = BytesMut::new();
    if encode(0x0123_4567_89ab_cdef, data, &mut out).is_ok() {
        let env = decode(&out).unwrap();
        assert_eq!(env.payload, data);
        assert_eq!(env.schema_id, 0x0123_4567_89ab_cdef);
    }
});
