//! Proposal 0006: `#[align(to = N)]`, record alignment, `ScalarList::as_slice`.

use graven::envelope;
use graven::{Builder, LocalPool};
use graven_codegen_tests::aligned::{Batch, BatchW, Samples, SamplesW};

/// Offset of `data` inside `whole`.
fn offset(whole: &[u8], data: &[u8]) -> usize {
    data.as_ptr() as usize - whole.as_ptr() as usize
}

/// Copies `bytes` to an address that is a multiple of 4096 (as a page of a mapped file).
fn page_aligned(bytes: &[u8]) -> (Vec<u8>, usize) {
    let mut v = vec![0u8; bytes.len() + 4096];
    let at = (4096 - v.as_ptr() as usize % 4096) % 4096;
    v[at..at + bytes.len()].copy_from_slice(bytes);
    (v, at)
}

fn write(b: &mut Builder<'_>) -> graven::Bytes {
    SamplesW::to_bytes(b, |w| {
        w.name("odd-length name");
        w.values(&[1.0, 2.5, -3.0]);
        w.ids(&[7, 8]);
        w.blob(b"abc");
        w.page(&[1, 2, 3, 4]);
    })
    .unwrap()
}

#[test]
fn lists_and_bytes_are_aligned_from_the_envelope_start() {
    let mut b = Builder::new_enveloped(&LocalPool);
    let env = write(&mut b);
    let (buf, at) = page_aligned(&env);
    let env = &buf[at..at + env.len()];
    let s = Samples::from_bytes(env).unwrap();
    let values = s.values();
    assert_eq!(offset(env, values.as_bytes()) % 16, 0);
    assert_eq!(values.as_slice(), Some(&[1.0f32, 2.5, -3.0][..]));
    let ids = s.ids().unwrap();
    assert_eq!(offset(env, ids.as_bytes()) % 8, 0);
    assert_eq!(ids.as_slice(), Some(&[7u64, 8][..]));
    assert_eq!(offset(env, s.blob().unwrap()) % 64, 0);
    assert_eq!(offset(env, s.page().unwrap().as_bytes()) % 4096, 0);
    assert_eq!(s.name(), "odd-length name");
}

#[test]
fn lists_are_aligned_from_the_message_start_without_an_envelope() {
    let mut b = Builder::new(&LocalPool);
    {
        let mut w = SamplesW::root(&mut b);
        w.name("x").values(&[1.0]).ids(&[1, 2, 3]);
    }
    let msg = b.finished().unwrap().to_vec();
    let (buf, at) = page_aligned(&msg);
    let msg = &buf[at..at + msg.len()];
    let s = Samples::verified(msg).unwrap();
    assert_eq!(offset(msg, s.values().as_bytes()) % 16, 0);
    assert_eq!(offset(msg, s.ids().unwrap().as_bytes()) % 8, 0);
}

#[test]
fn as_slice_falls_back_when_not_aligned() {
    let mut b = Builder::new_enveloped(&LocalPool);
    let env = write(&mut b);
    // The same envelope one byte further: still valid, the list is no longer aligned.
    let (mut buf, at) = page_aligned(&[&[0u8][..], &env[..]].concat());
    let env = &mut buf[at + 1..at + 1 + env.len()];
    let s = Samples::from_bytes(env).unwrap();
    assert_eq!(s.values().as_slice(), None);
    assert_eq!(s.values().iter().collect::<Vec<_>>(), vec![1.0, 2.5, -3.0]);
}

#[test]
fn aligned_lists_inside_list_elements() {
    let mut b = Builder::new_enveloped(&LocalPool);
    let env = BatchW::to_bytes(&mut b, |w| {
        let mut l = w.items(3);
        for i in 0..3u32 {
            l.push().name(&"n".repeat(i as usize + 1)).values(&[i as f32; 3]);
        }
    })
    .unwrap();
    let (buf, at) = page_aligned(&env);
    let env = &buf[at..at + env.len()];
    let batch = Batch::from_bytes(env).unwrap();
    for (i, s) in batch.items().iter().enumerate() {
        assert_eq!(offset(env, s.values().as_bytes()) % 16, 0, "item {i}");
        assert_eq!(s.values().as_slice(), Some(&[i as f32; 3][..]));
    }
    // Streaming elements too.
    let env = BatchW::to_bytes(&mut b, |w| {
        let mut l = w.items_streaming();
        for i in 0..3u32 {
            l.push().name(&"m".repeat(i as usize + 2)).values(&[1.0; 5]);
        }
    })
    .unwrap();
    let (buf, at) = page_aligned(&env);
    let env = &buf[at..at + env.len()];
    for s in Batch::from_bytes(env).unwrap().items().iter() {
        assert_eq!(offset(env, s.values().as_bytes()) % 16, 0);
    }
}

#[test]
fn record_align_pads_every_envelope_to_a_multiple() {
    let mut b = Builder::new_enveloped(&LocalPool).with_record_align(64);
    for n in [1usize, 50, 63, 64, 65, 200] {
        let env = SamplesW::to_bytes(&mut b, |w| {
            w.name(&"x".repeat(n)).values(&[0.5]);
        })
        .unwrap();
        assert_eq!(env.len() % 64, 0, "name of {n} bytes: envelope of {} bytes", env.len());
        let s = Samples::from_bytes(&env).unwrap();
        assert_eq!(s.name().len(), n);
    }
    // A batch of sealed envelopes: every one starts at a multiple of 4096, and the stream of
    // envelopes splits as usual.
    let mut b = Builder::new_enveloped(&LocalPool).with_record_align(4096);
    for n in 0..5usize {
        SamplesW::seal(&mut b, |w| {
            w.name(&"y".repeat(n * 700)).values(&[n as f32]);
        })
        .unwrap();
    }
    let file = b.take_frames();
    let mut at = 0;
    let mut count = 0;
    while at < file.len() {
        assert_eq!(at % 4096, 0);
        let total = envelope::decode(&file[at..]).unwrap().len();
        let s = Samples::from_bytes(&file[at..at + total]).unwrap();
        assert_eq!(s.values().get(0), Some(count as f32));
        at += total;
        count += 1;
    }
    assert_eq!(count, 5);
}

#[test]
fn rebuild_keeps_alignment() {
    let mut b = Builder::new_enveloped(&LocalPool);
    let old = write(&mut b);
    let ov = Samples::from_bytes(&old).unwrap();
    let new = SamplesW::rebuild(&mut b, &ov, |w| {
        w.name("a much longer name than before, to move everything after it");
    })
    .unwrap();
    let (buf, at) = page_aligned(&new);
    let new = &buf[at..at + new.len()];
    let nv = Samples::from_bytes(new).unwrap();
    assert_eq!(offset(new, nv.values().as_bytes()) % 16, 0);
    assert_eq!(offset(new, nv.blob().unwrap()) % 64, 0);
    assert_eq!(nv.values().as_slice(), Some(&[1.0f32, 2.5, -3.0][..]));
    assert_eq!(nv.blob(), ov.blob());
}

/// The bytes of one aligned message, pinned here and in `ts/tests/rebuild_align.test.ts`:
/// the Rust and JavaScript writers place the padding identically.
pub const PINNED_HEX: &str = "4752564e01000000fefb695008dd59a16f000000140014000000320000003e0000005a000000000000000f0000006f64642d6c656e677468206e616d65000000000000000000000000000000030000000000803f00002040000040c002000000070000000000000008000000000000000000000000000000000000000300000061626380282d5d";

#[test]
fn rust_and_javascript_write_the_same_aligned_bytes() {
    let mut b = Builder::new_enveloped(&LocalPool);
    let env = SamplesW::to_bytes(&mut b, |w| {
        w.name("odd-length name").values(&[1.0, 2.5, -3.0]).ids(&[7, 8]).blob(b"abc");
    })
    .unwrap();
    let hex: String = env.iter().map(|x| format!("{x:02x}")).collect();
    assert_eq!(hex, PINNED_HEX);
}
