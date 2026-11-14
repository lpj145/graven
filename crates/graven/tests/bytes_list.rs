//! `TableW::set_bytes_list` and its relation to `set_strs`.
use graven::{Builder, SimplePool, TableRef};

/// `set_strs(0, ["a", "bc"])` on a root with one pointer slot, as produced before
/// `set_bytes_list` existed (frozen: `set_strs` output must never change).
const STRS_GOLDEN: [u8; 29] = [
    4, 0, 4, 0, 0, 0, // root: fixed_len 4, ptr → list at 6
    2, 0, 0, 0, 8, 0, 0, 0, 9, 0, 0, 0, // count 2, rel → 18, rel → 23
    1, 0, 0, 0, b'a', // "a"
    2, 0, 0, 0, b'b', b'c', // "bc"
];

fn build(f: impl FnOnce(&mut graven::TableW<'_>)) -> Vec<u8> {
    let pool = SimplePool::new(64);
    let mut b = Builder::new(&pool);
    f(&mut b.root(4));
    b.finished().unwrap().to_vec()
}

#[test]
fn set_strs_output_is_unchanged() {
    let got = build(|t| t.set_strs(0, ["a", "bc"].into_iter()));
    assert_eq!(got, STRS_GOLDEN.as_slice());
}

#[test]
fn set_bytes_list_matches_set_strs_layout() {
    let got = build(|t| t.set_bytes_list(0, [&b"a"[..], &b"bc"[..]].into_iter()));
    assert_eq!(got, STRS_GOLDEN.as_slice());
}

#[test]
fn set_bytes_list_non_utf8_reads_back() {
    let blobs: [&[u8]; 4] = [&[0xFF, 0x00], &[], &[0xC3], &[1, 2, 3]];
    let buf = build(|t| t.set_bytes_list(0, blobs.into_iter()));
    let l = TableRef::at(&buf, 0).unwrap().strs(0).unwrap().unwrap();
    assert_eq!(l.len(), 4);
    for (i, want) in blobs.iter().enumerate() {
        assert_eq!(l.get_bytes(i).unwrap(), *want);
    }
    assert!(l.get(0).is_err()); // not UTF-8 as a string
}
