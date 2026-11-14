//! Writer limits (programmer-error panics) and the error variants of the envelope layer.
use graven::{Builder, Error, SimplePool};

#[test]
fn error_display_new_variants() {
    assert!(Error::Incomplete { needed: 24 }.to_string().contains("24"));
    let s = Error::SchemaMismatch { expected: 1, found: 2 }.to_string();
    assert!(s.contains("0x1") && s.contains("0x2"), "{s}");
    assert!(!Error::BadMagic.to_string().is_empty());
    assert!(Error::UnsupportedVersion(7).to_string().contains('7'));
    assert!(!Error::BadHeader.to_string().is_empty());
    assert!(!Error::ChecksumMismatch.to_string().is_empty());
}

#[test]
#[should_panic(expected = "list too large")]
fn list_over_2gib_panics() {
    let pool = SimplePool::new(64);
    let mut b = Builder::new(&pool);
    let mut t = b.root(4);
    let _ = t.tables(0, u32::MAX, u16::MAX);
}

#[test]
#[should_panic(expected = "list too large")]
fn fixed_list_over_2gib_panics() {
    let pool = SimplePool::new(64);
    let mut b = Builder::new(&pool);
    let mut t = b.root(4);
    let _ = t.tables_fixed::<65535>(0, u32::MAX);
}

#[test]
#[should_panic]
fn push_beyond_n_panics() {
    let pool = SimplePool::new(64);
    let mut b = Builder::new(&pool);
    let mut t = b.root(4);
    let mut l = t.tables(0, 1, 4);
    l.push();
    l.push();
}

#[test]
#[should_panic(expected = "sealed frames pending")]
fn flush_with_pending_frames_panics() {
    let pool = SimplePool::new(64);
    let mut b = Builder::new(&pool);
    b.root(4);
    b.seal().unwrap();
    let _ = b.flush();
}
