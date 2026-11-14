//! Support for generated code (`graven::rt`).
use proptest::prelude::*;
use graven::rt::{
    Req, ReqMask, VStrList, check_required, check_required_fast, mask_for, verify_bytes_list, verify_strs,
};
use graven::Bit;
use graven::{Builder, Error, Le, SimplePool, TableRef, VerifyCtx};

/// Root table `fixed_len = 6` holding one union slot `[u16 tag][i32 ptr]` at offset 0,
/// followed by an empty child table `[u16 0]` at position 8.
fn union_table(tag: u16, ptr: i32) -> Vec<u8> {
    let mut v = vec![6, 0];
    v.extend_from_slice(&tag.to_le_bytes());
    v.extend_from_slice(&ptr.to_le_bytes());
    v.extend_from_slice(&[0, 0]);
    v
}

#[test]
fn req_union_missing_when_ptr_zero() {
    let reqs = [Req::Union { name: "u", off: 0 }];
    let check = |buf: &[u8]| check_required(&TableRef::at(buf, 0).unwrap(), &reqs);

    // tag ≠ 0 but ptr = 0 ⇒ absent ⇒ Missing (spec §4.3).
    assert_eq!(check(&union_table(1, 0)), Err(Error::Missing { field: "u" }));
    // tag = 0 ⇒ absent.
    assert_eq!(check(&union_table(0, 4)), Err(Error::Missing { field: "u" }));
    // ptr (relative to its own slot at 4) ⇒ 8, the child table.
    assert_eq!(check(&union_table(1, 4)), Ok(()));
    // Slot outside fixed_len ⇒ absent; partially covered ⇒ Truncated.
    assert_eq!(check(&[0, 0]), Err(Error::Missing { field: "u" }));
    assert_eq!(check(&[3, 0, 1, 0, 4]), Err(Error::Truncated));
}

#[test]
fn vstrlist_reads_without_validation_after_verify() {
    let pool = SimplePool::new(256);
    let mut b = Builder::new(&pool);
    let words = ["", "x", "olá", "𝄞 graven"];
    b.root(4).set_strs(0, words.iter().copied());
    let buf = b.finish().unwrap().to_vec();

    let checked = TableRef::at(&buf, 0).unwrap().strs(0).unwrap().unwrap();
    // "verify": every element validated by the checked `StrList::get`.
    for i in 0..checked.len() {
        checked.get(i).unwrap();
    }
    // SAFETY: every element of this list was just validated as UTF-8 above.
    let v = unsafe { VStrList::new(Some(checked)) };
    assert_eq!(v.len(), words.len());
    assert!(!v.is_empty());
    for (i, w) in words.iter().enumerate() {
        assert_eq!(v.get(i), checked.get(i));
        assert_eq!(v.get(i), Ok(*w));
        assert_eq!(v.get_bytes(i), checked.get_bytes(i));
    }
    assert_eq!(v.iter().len(), words.len());
    assert_eq!(v.iter().collect::<Result<Vec<_>, _>>().unwrap(), words);
    assert_eq!(v.get(words.len()), Err(Error::OutOfBounds));

    // SAFETY: an absent list has no strings.
    let none = unsafe { VStrList::new(None) };
    assert_eq!(none.len(), 0);
    assert!(none.is_empty());
    assert_eq!(none.get(0), Err(Error::OutOfBounds));
    assert_eq!(none.iter().count(), 0);
}

#[test]
fn bool_le_follows_spec() {
    // Spec §5.3 / §6.3: 1 byte; writer writes 00/01; reader: 00 ⇒ false, anything else ⇒ true.
    assert_eq!(<bool as Le>::SIZE, 1);
    assert!(!bool::read_le(&[0]));
    assert!(bool::read_le(&[1]));
    assert!(bool::read_le(&[2]));
    assert!(bool::read_le(&[0xFF]));
    let mut b = [0xAAu8];
    true.write_le(&mut b);
    assert_eq!(b, [1]);
    false.write_le(&mut b);
    assert_eq!(b, [0]);
}

const UREQ: &[Req] = &[
    Req::Scalar { name: "a", off: 1, size: 1, bit: Bit { byte: 0, shift: 0 } },
    Req::Union { name: "u", off: 2 },
];
const UMASK: ReqMask = ReqMask { len: 8, bits: &[(0, mask_for(UREQ, 0))], ptrs: &[], unions: &[2] };

proptest! {
    #![proptest_config(ProptestConfig::with_cases(if cfg!(miri) { 64 } else { 4096 }))]
    /// The mask fast path must agree with the per-field check for required unions.
    #[test]
    fn union_fast_path_agrees(
        len in 0u16..10,
        body in proptest::collection::vec(any::<u8>(), 10),
        sparse in any::<bool>(),
    ) {
        let mut buf = len.to_le_bytes().to_vec();
        buf.extend(body.iter().map(|b| if sparse && b % 3 != 0 { 0 } else { *b }));
        if let Ok(t) = TableRef::at(&buf, 0) {
            prop_assert_eq!(check_required_fast(&t, &UMASK, UREQ), check_required(&t, UREQ));
        }
    }
}

fn list(buf: &[u8]) -> Option<graven::StrList<'_>> {
    TableRef::at(buf, 0).unwrap().strs(0).unwrap()
}

fn strs_msg(words: &[&str]) -> Vec<u8> {
    let pool = SimplePool::new(256);
    let mut b = Builder::new(&pool);
    b.root(4).set_strs(0, words.iter().copied());
    b.finish().unwrap().to_vec()
}

#[test]
fn verify_strs_charges_spec_and_rejects_bad_utf8() {
    let words = ["", "x", "olá", "BAD!"];
    let mut buf = strs_msg(&words);

    // Spec §7.3: 4 + 4 × count, plus 4 + len per element.
    let total = 4 + 4 * 4 + 4 + (4 + 1) + (4 + 4) + (4 + 4);
    let mut ctx = VerifyCtx::with_limits(64, 1000);
    verify_strs(list(&buf), &mut ctx).unwrap();
    assert_eq!(1000 - ctx.budget(), total);
    let mut ctx = VerifyCtx::with_limits(64, 1000);
    verify_bytes_list(list(&buf), &mut ctx).unwrap();
    assert_eq!(1000 - ctx.budget(), total);
    assert_eq!(verify_strs(list(&buf), &mut VerifyCtx::with_limits(64, total - 1)), Err(Error::BudgetExceeded));
    assert_eq!(verify_strs(None, &mut VerifyCtx::with_limits(64, 0)), Ok(()));

    // A list that passed verify_strs reads the same through VStrList and StrList.
    let l = list(&buf).unwrap();
    // SAFETY: `verify_strs` succeeded on this very list just above.
    let v = unsafe { VStrList::new(Some(l)) };
    assert_eq!(l.len(), words.len());
    for (i, w) in words.iter().enumerate() {
        assert_eq!(v.get(i), l.get(i));
        assert_eq!(v.get(i), Ok(*w));
    }

    // One element with invalid UTF-8 ⇒ InvalidUtf8 (bytes list still accepts it).
    let pos = buf.windows(4).position(|w| w == b"BAD!").unwrap();
    buf[pos] = 0xFF;
    let mut ctx = VerifyCtx::new(buf.len());
    assert_eq!(verify_strs(list(&buf), &mut ctx), Err(Error::InvalidUtf8));
    assert_eq!(verify_bytes_list(list(&buf), &mut VerifyCtx::new(buf.len())), Ok(()));
}

/// `table_fixed` / `union_fixed` (nested all-scalar tables in generated writers) write the
/// same bytes as `table` / `union` + `set`.
#[test]
fn table_fixed_matches_table() {
    let pool = SimplePool::new(256);
    let bit = Bit { byte: 0, shift: 0 };
    let build = |fixed: bool| {
        let mut b = Builder::new(&pool);
        {
            let mut r = b.root(10);
            if fixed {
                r.table_fixed::<5>(0).set(1, bit, 0xdead_beef_u32);
                r.union_fixed::<5>(4, 3).set(1, bit, 7u32);
            } else {
                r.table(0, 5).set(1, bit, 0xdead_beef_u32);
                r.union(4, 3, 5).set(1, bit, 7u32);
            }
        }
        b.finished().unwrap().to_vec()
    };
    let a = build(true);
    assert_eq!(a, build(false));
    let t = TableRef::at(&a, 0).unwrap();
    assert_eq!(t.table(0).unwrap().unwrap().raw::<u32>(1), 0xdead_beef);
    let (tag, u) = t.union(4).unwrap().unwrap();
    assert_eq!((tag, u.raw::<u32>(1)), (3, 7));
}
