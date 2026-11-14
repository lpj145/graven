//! Codegen edge cases (`schemas/edge.graven`): everything written through the generated
//! writer reads back through the checked reader and the verified view; defaults, required
//! pointers, nested all-scalar tables (`table_fixed` / `union_fixed`), `[bool]`, `[enum]`,
//! `[struct]`, a keyword field name and reserved fields.

use graven::{Builder, Error, SimplePool, TableRef};
use graven_codegen_tests::edge::*;

fn px(r: u8) -> Pixel {
    Pixel { r, on: r % 2 == 1, mode: Mode::from_raw(r as u16), tiny: Tiny::from_raw(r) }
}

fn finish(f: impl FnOnce(&mut EdgeW<'_>)) -> Vec<u8> {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new(&pool);
    f(&mut EdgeW::root(&mut b));
    b.finished().unwrap().to_vec()
}

/// Only the required fields.
fn required(w: &mut EdgeW<'_>, pick_big: bool) {
    w.r#type(3).req_str("req").req_bytes(&[1, 2]).req_nums(&[-1, i64::MAX]);
    w.req_names(["a", "", "ç"]);
    w.req_blobs([&[0u8][..], &[]]);
    {
        let mut l = w.req_small(2);
        l.push().v(10).ok(true);
        l.push().v(11);
    }
    {
        let mut l = w.req_big(1);
        l.push().s("big").b(&[9]);
    }
    w.req_tab().v(77).ok(false);
    if pick_big {
        w.req_pick_big().s("bb").b(&[]);
    } else {
        w.req_pick_small().v(5);
    }
    w.pair(Pair { a: px(1), w: -0.25 });
}

#[test]
fn defaults_and_required_roundtrip() {
    let buf = finish(|w| required(w, false));
    let v = Edge::verified(&buf).unwrap();
    let r = v.reader();
    // Defaults when absent.
    assert_eq!((r.d_i8(), r.d_i64(), r.d_f32(), r.d_f64(), r.d_bool(), r.d_mode()), (-128, i64::MIN, -1.5, 2.0, true, Mode::Auto));
    assert_eq!((v.d_i8(), v.d_mode()), (-128, Mode::Auto));
    // Required fields.
    assert_eq!(v.r#type(), 3);
    assert_eq!(v.req_str().unwrap(), "req");
    assert_eq!(r.req_str().unwrap(), "req");
    assert_eq!(v.req_bytes().unwrap(), &[1, 2]);
    assert_eq!(v.req_nums().unwrap().iter().collect::<Vec<_>>(), [-1, i64::MAX]);
    let names = v.req_names().unwrap();
    assert_eq!((0..names.len()).map(|i| names.get(i).unwrap()).collect::<Vec<_>>(), ["a", "", "ç"]);
    let names = r.req_names().unwrap();
    assert_eq!(names.get(2).unwrap(), "ç");
    let blobs = v.req_blobs().unwrap();
    assert_eq!((blobs.get_bytes(0).unwrap(), blobs.get_bytes(1).unwrap()), (&[0u8][..], &[][..]));
    let small: Vec<(u32, Option<bool>)> = v.req_small().unwrap().iter().map(|e| e.unwrap()).map(|e| (e.v(), e.ok())).collect();
    assert_eq!(small, [(10, Some(true)), (11, None)]);
    let big = v.req_big().unwrap().get(0).unwrap();
    assert_eq!((big.s().unwrap(), big.b().unwrap()), ("big", &[9u8][..]));
    let t = v.req_tab().unwrap();
    assert_eq!((t.v(), t.ok()), (77, Some(false)));
    assert_eq!(r.req_tab().unwrap().v(), 77);
    assert!(matches!(v.req_pick().unwrap(), PickVerified::Small(s) if s.v() == 5));
    assert!(matches!(r.req_pick().unwrap(), Pick::Small(s) if s.v() == 5));
    assert_eq!(v.pair(), Pair { a: px(1), w: -0.25 });
    // Optional pointers absent.
    assert!(v.opt_tab().unwrap().is_none());
    assert!(v.opt_pick().unwrap().is_none());
    assert!(v.bools().unwrap().is_none());
    assert_eq!(v.pixel(), None);
    assert_eq!(v.last(), None);

    let buf = finish(|w| required(w, true));
    let v = Edge::verified(&buf).unwrap();
    assert!(matches!(v.req_pick().unwrap(), PickVerified::Big(b) if b.s().unwrap() == "bb"));
}

#[test]
fn optional_and_lists_roundtrip() {
    let buf = finish(|w| {
        required(w, false);
        w.d_i8(5).d_i64(-7).d_f32(0.5).d_f64(-0.0).d_bool(false).d_mode(Mode::Unknown(300));
        w.pixel(px(2));
        w.bools(&[true, false, true]);
        w.modes(&[Mode::Off, Mode::On, Mode::Unknown(9)]);
        {
            let mut l = w.pixels_streaming();
            l.push(px(3));
            l.push(px(4));
        }
        w.opt_tab().s("opt").b(&[1]);
        w.opt_pick_big().s("p").b(&[2]);
        w.last(250);
    });
    let v = Edge::verified(&buf).unwrap();
    assert_eq!((v.d_i8(), v.d_i64(), v.d_f32(), v.d_bool(), v.d_mode()), (5, -7, 0.5, false, Mode::Unknown(300)));
    assert_eq!(v.d_f64().to_bits(), (-0.0f64).to_bits());
    assert_eq!(v.pixel(), Some(px(2)));
    assert_eq!(v.bools().unwrap().unwrap().iter().collect::<Vec<_>>(), [true, false, true]);
    assert_eq!(v.modes().unwrap().unwrap().iter().collect::<Vec<_>>(), [Mode::Off, Mode::On, Mode::Unknown(9)]);
    assert_eq!(v.pixels().unwrap().unwrap().iter().collect::<Vec<_>>(), [px(3), px(4)]);
    assert_eq!(v.opt_tab().unwrap().unwrap().s().unwrap(), "opt");
    assert!(matches!(v.opt_pick().unwrap(), Some(PickVerified::Big(b)) if b.b().unwrap() == [2]));
    assert_eq!(v.last(), Some(250));
}

#[test]
fn missing_required_pointer_and_union() {
    let buf = finish(|w| {
        w.r#type(1);
    });
    assert_eq!(Edge::read(&buf).err(), Some(Error::Missing { field: "req_str" }));
    // Everything but the union.
    let buf = finish(|w| {
        w.r#type(3).req_str("x").req_bytes(&[]).req_nums(&[]).req_names([]).req_blobs([]);
        w.req_small(0);
        w.req_big(0);
        w.req_tab().v(1);
        w.pair(Pair::default());
    });
    assert_eq!(Edge::verified(&buf).err(), Some(Error::Missing { field: "req_pick" }));
}

/// Nested all-scalar writes (`table_fixed` / `union_fixed`) give the same bytes as the
/// generic `TableW` path.
#[test]
fn nested_fixed_tables_match_tablew() {
    let gen_bytes = finish(|w| {
        w.req_tab().v(0xabcd).ok(true);
        w.req_pick_small().v(3);
    });
    let pool = SimplePool::new(4096);
    let mut b = Builder::new(&pool);
    {
        let mut t = b.root(edge::FIXED_LEN);
        {
            let mut s = t.table(edge::REQ_TAB, small::FIXED_LEN);
            s.set(small::V, small::V_BIT, 0xabcd_u32);
            s.set_bool(small::OK_BIT, true);
        }
        t.union(edge::REQ_PICK, 1, small::FIXED_LEN).set(small::V, small::V_BIT, 3u32);
    }
    assert_eq!(gen_bytes, b.finished().unwrap().to_vec());
    let root = TableRef::at(&gen_bytes, 0).unwrap();
    assert_eq!(root.union(edge::REQ_PICK).unwrap().unwrap().0, 1);
}

/// Reserved fields generate nothing, but their slots stay in the layout.
#[test]
fn reserved_slots_stay_in_layout() {
    let schema = include_str!("../schemas/edge.graven.lock");
    assert!(schema.contains("_reserved_23") && schema.contains("_reserved_24"));
    // `last @25` sits after the two reserved slots (string ptr + u64).
    const { assert!(edge::LAST > edge::OPT_PICK + 6 + 4 + 8 - 1) };
    // 12 non-reserved pointers + `slot + 2` of the two unions.
    assert_eq!(edge::PTR_SLOTS.len(), 14);
    assert!(edge::PTR_SLOTS.contains(&(edge::REQ_PICK + 2)) && edge::PTR_SLOTS.contains(&(edge::OPT_PICK + 2)));
}

/// `verified` (not only `read`) rejects absent required `string`/`bytes`, at the top and
/// in a nested table (union variant and optional): the codegen's unconditional charge
/// cannot skip the `Missing`.
#[test]
fn verified_rejects_missing_required_str_bytes() {
    let full = |w: &mut EdgeW<'_>| {
        w.r#type(1).req_nums(&[]).req_names([]).req_blobs([]);
        w.req_small(0);
        w.req_big(0);
        w.req_tab().v(1);
        w.pair(Pair::default());
    };
    let buf = finish(|w| {
        full(w);
        w.req_bytes(&[]);
        w.req_pick_small().v(1);
    });
    assert_eq!(Edge::verified(&buf).err(), Some(Error::Missing { field: "req_str" }));
    let buf = finish(|w| {
        full(w);
        w.req_str("x");
        w.req_pick_small().v(1);
    });
    assert_eq!(Edge::verified(&buf).err(), Some(Error::Missing { field: "req_bytes" }));
    // Nested: variant `Big` without `b`.
    let buf = finish(|w| {
        full(w);
        w.req_str("x").req_bytes(&[]);
        w.req_pick_big().s("s");
    });
    assert_eq!(Edge::verified(&buf).err(), Some(Error::Missing { field: "b" }));
    // Nested: `opt_tab` present without `s`.
    let buf = finish(|w| {
        full(w);
        w.req_str("x").req_bytes(&[]);
        w.req_pick_small().v(1);
        w.opt_tab().b(&[1]);
    });
    assert_eq!(Edge::verified(&buf).err(), Some(Error::Missing { field: "s" }));
}
