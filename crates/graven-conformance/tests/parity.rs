//! The interpreter against the compiler-generated code in `graven-schemas`.

use proptest::prelude::*;
use std::collections::BTreeMap;
use graven::{Builder, SimplePool, VerifyCtx};
use graven_conformance::{Value, bank_transaction, game_frame, read, write};
use graven_schemas::bank::{Currency, Transaction, TransactionW};
use graven_schemas::game::{Frame, FrameW, Vec3};

fn table(pairs: Vec<(&str, Value)>) -> Value {
    Value::Table(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>())
}

fn finish(b: Builder<'_>) -> Vec<u8> {
    b.finish().unwrap().to_vec()
}

// ---------------------------------------------------- generated ⇒ Value

fn bank_value(t: &Transaction<'_>) -> Value {
    let mut entries = Vec::new();
    for e in t.entries().unwrap().iter() {
        let e = e.unwrap();
        let mut m = vec![("account", Value::UInt(e.account() as u128)), ("amount", Value::Int(e.amount() as i128))];
        if let Some(s) = e.memo().unwrap() {
            m.push(("memo", Value::Str(s.into())));
        }
        entries.push(table(m));
    }
    let mut m = vec![
        ("id", Value::UInt(t.id() as u128)),
        ("ts", Value::UInt(t.ts() as u128)),
        ("currency", Value::UInt(t.currency().to_raw() as u128)),
        ("from_shard", Value::UInt(t.from_shard() as u128)),
        ("to_shard", Value::UInt(t.to_shard() as u128)),
        ("entries", Value::List(entries)),
    ];
    if let Some(s) = t.note().unwrap() {
        m.push(("note", Value::Str(s.into())));
    }
    table(m)
}

fn vec3(v: Vec3) -> Value {
    table(vec![("x", Value::F32(v.x)), ("y", Value::F32(v.y)), ("z", Value::F32(v.z))])
}

fn game_value(f: &Frame<'_>) -> Value {
    let mut entities = Vec::new();
    for e in f.entities().unwrap().iter() {
        let e = e.unwrap();
        entities.push(table(vec![
            ("id", Value::UInt(e.id() as u128)),
            ("pos", vec3(e.pos())),
            ("vel", vec3(e.vel())),
            ("hp", Value::UInt(e.hp() as u128)),
            ("alive", Value::Bool(e.alive())),
        ]));
    }
    table(vec![("tick", Value::UInt(f.tick() as u128)), ("entities", Value::List(entities))])
}

/// Hand-written `read()` + `verify()` (default limits), then accessors ⇒ Value.
fn bank_hand(buf: &[u8]) -> Option<Value> {
    let t = Transaction::read(buf).ok()?;
    t.verify(&mut VerifyCtx::new(buf.len())).ok()?;
    Some(bank_value(&t))
}

fn game_hand(buf: &[u8]) -> Option<Value> {
    let f = Frame::read(buf).ok()?;
    f.verify(&mut VerifyCtx::new(buf.len())).ok()?;
    Some(game_value(&f))
}

// ------------------------------------------------------------ messages

type Ent = (u64, i64, Option<String>);

fn bank_msg(pool: &SimplePool, hdr: (u64, u64, u16, u32, u32), entries: &[Ent], note: Option<&str>) -> Vec<u8> {
    let mut b = Builder::new(pool);
    {
        let mut t = TransactionW::root(&mut b);
        // ID order, so the interpreter's output can be compared byte for byte.
        t.id(hdr.0).ts(hdr.1).currency(Currency::from_raw(hdr.2)).from_shard(hdr.3).to_shard(hdr.4);
        {
            let mut l = t.entries(entries.len() as u32);
            for (a, m, memo) in entries {
                let mut e = l.push();
                e.account(*a).amount(*m);
                if let Some(s) = memo {
                    e.memo(s);
                }
            }
        }
        if let Some(n) = note {
            t.note(n);
        }
    }
    finish(b)
}

type Ent3 = (u32, [u32; 3], [u32; 3], u16, bool);

fn game_msg(pool: &SimplePool, tick: u64, ents: &[Ent3]) -> Vec<u8> {
    let v = |a: [u32; 3]| Vec3 { x: f32::from_bits(a[0]), y: f32::from_bits(a[1]), z: f32::from_bits(a[2]) };
    let mut b = Builder::new(pool);
    {
        let mut f = FrameW::root(&mut b);
        f.tick(tick);
        let mut l = f.entities(ents.len() as u32);
        for &(id, pos, vel, hp, alive) in ents {
            l.push().id(id).pos(v(pos)).vel(v(vel)).hp(hp).alive(alive);
        }
    }
    finish(b)
}

fn bank_strategy() -> impl Strategy<Value = Vec<u8>> {
    (
        (any::<u64>(), any::<u64>(), any::<u16>(), any::<u32>(), any::<u32>()),
        prop::collection::vec((any::<u64>(), any::<i64>(), prop::option::of(".{0,6}")), 0..4),
        prop::option::of(".{0,6}"),
    )
        .prop_map(|(h, e, n)| bank_msg(&SimplePool::new(256), h, &e, n.as_deref()))
}

fn game_strategy() -> impl Strategy<Value = Vec<u8>> {
    (
        any::<u64>(),
        prop::collection::vec(
            (any::<u32>(), any::<[u32; 3]>(), any::<[u32; 3]>(), any::<u16>(), any::<bool>()),
            0..4,
        ),
    )
        .prop_map(|(t, e)| game_msg(&SimplePool::new(256), t, &e))
}

// --------------------------------------------------------------- tests

#[test]
fn game_matches_generated() {
    let pool = SimplePool::new(256);
    let nan = 0x7fc0_0001;
    let ents = [
        (1, [1.5f32.to_bits(), (-0.0f32).to_bits(), nan], [0, 0, 0], 100, true),
        (u32::MAX, [f32::INFINITY.to_bits(), 1, 2], [3, 4, 5], 0, false),
    ];
    let buf = game_msg(&pool, u64::MAX, &ents);
    let v = read(&buf, game_frame()).unwrap();
    assert_eq!(Some(v.clone()), game_hand(&buf));
    let Value::Table(m) = &v else { panic!() };
    let Value::List(es) = &m["entities"] else { panic!() };
    assert_eq!(es.len(), 2);
    // Re-encoding gives the generated writer's bytes.
    let mut b = Builder::new(&pool);
    write(&v, game_frame(), &mut b).unwrap();
    assert_eq!(finish(b), buf);
}

#[test]
fn bank_reencodes_byte_identically() {
    let pool = SimplePool::new(256);
    let buf = bank_msg(&pool, (1, 2, 7, 3, 4), &[(5, -6, Some("é".into())), (7, 8, None)], Some("n"));
    let v = read(&buf, bank_transaction()).unwrap();
    assert_eq!(Some(v.clone()), bank_hand(&buf));
    let mut b = Builder::new(&pool);
    write(&v, bank_transaction(), &mut b).unwrap();
    assert_eq!(finish(b), buf);
}

/// One random byte replaced and, optionally, the buffer truncated.
fn mutate(mut buf: Vec<u8>, i: prop::sample::Index, x: u8, cut: Option<prop::sample::Index>) -> Vec<u8> {
    let at = i.index(buf.len());
    buf[at] = x;
    if let Some(c) = cut {
        buf.truncate(c.index(buf.len() + 1));
    }
    buf
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(if cfg!(miri) { 16 } else { 2000 }))]

    #[test]
    fn bank_valid_messages_agree(buf in bank_strategy()) {
        let v = read(&buf, bank_transaction()).unwrap();
        prop_assert_eq!(Some(v.clone()), bank_hand(&buf));
        let pool = SimplePool::new(256);
        let mut b = Builder::new(&pool);
        write(&v, bank_transaction(), &mut b).unwrap();
        prop_assert_eq!(finish(b), buf);
    }

    #[test]
    fn game_valid_messages_agree(buf in game_strategy()) {
        let v = read(&buf, game_frame()).unwrap();
        prop_assert_eq!(Some(v.clone()), game_hand(&buf));
        let pool = SimplePool::new(256);
        let mut b = Builder::new(&pool);
        write(&v, game_frame(), &mut b).unwrap();
        prop_assert_eq!(finish(b), buf);
    }

    #[test]
    fn bank_mutated_differential(
        buf in bank_strategy(), i in any::<prop::sample::Index>(), x in any::<u8>(),
        cut in prop::option::weighted(0.2, any::<prop::sample::Index>()),
    ) {
        let buf = mutate(buf, i, x, cut);
        let mine = read(&buf, bank_transaction());
        let hand = bank_hand(&buf);
        prop_assert_eq!(mine.is_ok(), hand.is_some(), "interp {:?} vs generated {:?}", mine, hand);
        if let Ok(v) = mine {
            prop_assert_eq!(Some(v), hand);
        }
    }

    #[test]
    fn game_mutated_differential(
        buf in game_strategy(), i in any::<prop::sample::Index>(), x in any::<u8>(),
        cut in prop::option::weighted(0.2, any::<prop::sample::Index>()),
    ) {
        let buf = mutate(buf, i, x, cut);
        let mine = read(&buf, game_frame());
        let hand = game_hand(&buf);
        prop_assert_eq!(mine.is_ok(), hand.is_some(), "interp {:?} vs generated {:?}", mine, hand);
        if let Ok(v) = mine {
            prop_assert_eq!(Some(v), hand);
        }
    }
}
