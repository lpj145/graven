//! Gate of the compiler-generated code (`graven_conformance::generated`, from
//! `schemas/conformance.graven` via `build.rs`) against the frozen vectors in
//! `testvectors/v1/` and under mutation.
//!
//! - `conformance_vectors_generated`: every `.bin` read with the generated `Root` /
//!   `RootV2` (`from_bytes` if enveloped, else `verified`) gives the `.json` result (value
//!   via [`to_value`], or the same error variant name).
//! - `generated_writer_reproduces_vectors`: for the valid vectors a typed writer can
//!   produce, the generated writer called in ID order reproduces the `.bin` byte for byte.
//! - `mutation_gate_generated`: mutated valid messages; whenever the generated `verified`
//!   accepts, every verified view (including `VStrList`, the union and `child.next`) reads
//!   exactly what the checked reader reads, and both equal the interpreter.
#![deny(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use graven::{Builder, Error, SimplePool};
use graven_conformance::generated as g;
use graven_conformance::vectors::{CaseJson, ExpectJson, error_name};
use graven_conformance::{Value, conformance_root, read};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testvectors/v1")
}

fn vectors() -> Vec<(String, Vec<u8>, CaseJson)> {
    let mut names: Vec<String> = fs::read_dir(dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter_map(|f| f.strip_suffix(".bin").map(str::to_string))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let bytes = fs::read(dir().join(format!("{n}.bin"))).unwrap();
            let json = fs::read_to_string(dir().join(format!("{n}.json"))).unwrap();
            let j: CaseJson = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{n}.json: {e}"));
            (n, bytes, j)
        })
        .collect()
}

// --------------------------------------------------------- views ⇒ Value

fn tbl(m: Vec<(&str, Value)>) -> Value {
    Value::Table(m.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>())
}

fn point(p: g::Point) -> Value {
    tbl(vec![("x", Value::Int(p.x as i128)), ("y", Value::Int(p.y as i128))])
}

fn leaf_verified(l: g::LeafVerified<'_>) -> Value {
    let mut m = vec![("n", Value::UInt(l.n() as u128))];
    if let Some(s) = l.s().unwrap() {
        m.push(("s", Value::Str(s.to_string())));
    }
    if let Some(next) = l.next().unwrap() {
        m.push(("next", leaf_verified(next)));
    }
    tbl(m)
}

fn leaf_checked(l: g::Leaf<'_>) -> Value {
    let mut m = vec![("n", Value::UInt(l.n() as u128))];
    if let Some(s) = l.s().unwrap() {
        m.push(("s", Value::Str(s.to_string())));
    }
    if let Some(next) = l.next().unwrap() {
        m.push(("next", leaf_checked(next)));
    }
    tbl(m)
}

/// The map of a `Root`/`RootV2` read through `$r` (a verified view or a checked reader);
/// `$leaf` turns a leaf into a `Value`, `$u` is the union enum (`ChoiceVerified` / `Choice`).
macro_rules! root_value {
    ($r:expr, $leaf:expr, $u:ident) => {{
        let r = $r;
        let mut m: Vec<(&str, Value)> = Vec::new();
        m.push(("a_u8", Value::UInt(r.a_u8() as u128)));
        if let Some(x) = r.a_i16() {
            m.push(("a_i16", Value::Int(x as i128)));
        }
        m.push(("a_u32", Value::UInt(r.a_u32() as u128)));
        if let Some(x) = r.a_i64() {
            m.push(("a_i64", Value::Int(x as i128)));
        }
        if let Some(x) = r.a_u128() {
            m.push(("a_u128", Value::UInt(x)));
        }
        if let Some(x) = r.a_i128() {
            m.push(("a_i128", Value::Int(x)));
        }
        if let Some(x) = r.a_f32() {
            m.push(("a_f32", Value::F32(x)));
        }
        if let Some(x) = r.a_f64() {
            m.push(("a_f64", Value::F64(x)));
        }
        if let Some(x) = r.flag() {
            m.push(("flag", Value::Bool(x)));
        }
        m.push(("flag2", Value::Bool(r.flag2())));
        if let Some(c) = r.color() {
            m.push(("color", Value::UInt(c.to_raw() as u128)));
        }
        if let Some(p) = r.point() {
            m.push(("point", point(p)));
        }
        if let Some(s) = r.name().unwrap() {
            m.push(("name", Value::Str(s.to_string())));
        }
        if let Some(b) = r.blob().unwrap() {
            m.push(("blob", Value::Bytes(b.to_vec())));
        }
        if let Some(l) = r.nums().unwrap() {
            m.push(("nums", Value::List(l.iter().map(|x| Value::UInt(x as u128)).collect())));
        }
        if let Some(l) = r.points().unwrap() {
            m.push(("points", Value::List(l.iter().map(point).collect())));
        }
        if let Some(l) = r.leaves().unwrap() {
            assert_eq!(l.iter().len(), l.len());
            assert!(l.get(l.len()).is_err());
            m.push(("leaves", Value::List(l.iter().map(|e| $leaf(e.unwrap())).collect())));
        }
        if let Some(l) = r.names().unwrap() {
            assert!(l.get(l.len()).is_err());
            m.push(("names", Value::List((0..l.len()).map(|i| Value::Str(l.get(i).unwrap().to_string())).collect())));
        }
        if let Some(l) = r.blobs().unwrap() {
            m.push(("blobs", Value::List((0..l.len()).map(|i| Value::Bytes(l.get_bytes(i).unwrap().to_vec())).collect())));
        }
        if let Some(c) = r.child().unwrap() {
            m.push(("child", $leaf(c)));
        }
        if let Some(u) = r.choice().unwrap() {
            let v = match u {
                g::$u::Leaf(l) => Value::Union { tag: 1, value: Box::new($leaf(l)) },
                g::$u::Unknown(tag) => Value::Union { tag, value: Box::new(tbl(vec![])) },
            };
            m.push(("choice", v));
        }
        m
    }};
}

/// `RootVerified` ⇒ the interpreter's `Value` (crate docs, "Resulting map").
fn to_value(r: &g::RootVerified<'_>) -> Value {
    tbl(root_value!(r, leaf_verified, ChoiceVerified))
}

fn to_value_checked(r: &g::Root<'_>) -> Value {
    tbl(root_value!(r, leaf_checked, Choice))
}

fn to_value_v2(r: &g::RootV2Verified<'_>) -> Value {
    let mut m = root_value!(r, leaf_verified, ChoiceVerified);
    if let Some(x) = r.extra() {
        m.push(("extra", Value::UInt(x as u128)));
    }
    tbl(m)
}

/// `Extra` (verified view or checked reader) ⇒ `Value`. `$u` is the union enum.
macro_rules! extra_value {
    ($r:expr, $leaf:expr, $u:ident) => {{
        let r = $r;
        let mut m: Vec<(&str, Value)> = Vec::new();
        if let Some(f) = r.flags() {
            m.push(("flags", tbl(vec![("on", Value::Bool(f.on)), ("c", Value::UInt(f.c.to_raw() as u128))])));
        }
        if let Some(l) = r.bools().unwrap() {
            m.push(("bools", Value::List(l.iter().map(Value::Bool).collect())));
        }
        if let Some(l) = r.colors().unwrap() {
            m.push(("colors", Value::List(l.iter().map(|c| Value::UInt(c.to_raw() as u128)).collect())));
        }
        if let Some(l) = r.names().unwrap() {
            m.push(("names", Value::List((0..l.len()).map(|i| Value::Str(l.get(i).unwrap().to_string())).collect())));
        }
        if let Some(l) = r.blobs().unwrap() {
            m.push(("blobs", Value::List((0..l.len()).map(|i| Value::Bytes(l.get_bytes(i).unwrap().to_vec())).collect())));
        }
        let pick = match r.pick().unwrap() {
            g::$u::Leaf(l) => Value::Union { tag: 1, value: Box::new($leaf(l)) },
            g::$u::Unknown(tag) => Value::Union { tag, value: Box::new(tbl(vec![])) },
        };
        m.push(("pick", pick));
        if let Some(l) = r.leaves().unwrap() {
            m.push(("leaves", Value::List(l.iter().map(|e| $leaf(e.unwrap())).collect())));
        }
        if r.e().is_some() {
            m.push(("e", tbl(vec![])));
        }
        if let Some(s) = r.tail().unwrap() {
            m.push(("tail", Value::Str(s.to_string())));
        }
        tbl(m)
    }};
}

fn extra_verified(r: &g::ExtraVerified<'_>) -> Value {
    extra_value!(r, leaf_verified, ChoiceVerified)
}

fn extra_checked(r: &g::Extra<'_>) -> Value {
    extra_value!(r, leaf_checked, Choice)
}

fn holder_verified(r: &g::HolderVerified<'_>) -> Value {
    let l = r.items().unwrap();
    tbl(vec![("items", Value::List(l.iter().map(|e| leaf_verified(e.unwrap())).collect()))])
}

fn holder_checked(r: &g::Holder<'_>) -> Value {
    let l = r.items().unwrap();
    tbl(vec![("items", Value::List(l.iter().map(|e| leaf_checked(e.unwrap())).collect()))])
}

/// Generated equivalent of `vectors::evaluate`.
fn evaluate(bytes: &[u8], schema: &str, enveloped: bool) -> Result<Value, Error> {
    match (schema, enveloped) {
        ("conformance.v1.Root", false) => g::Root::verified(bytes).map(|v| {
            assert_eq!(to_value_checked(&v.reader()), to_value(&v), "view ≠ reader");
            to_value(&v)
        }),
        ("conformance.v1.Root", true) => g::Root::from_bytes(bytes).map(|v| to_value(&v)),
        ("conformance.v1.RootV2", false) => g::RootV2::verified(bytes).map(|v| to_value_v2(&v)),
        ("conformance.v1.RootV2", true) => g::RootV2::from_bytes(bytes).map(|v| to_value_v2(&v)),
        ("conformance.v1.Extra", false) => g::Extra::verified(bytes).map(|v| {
            assert_eq!(extra_checked(&v.reader()), extra_verified(&v), "view ≠ reader");
            extra_verified(&v)
        }),
        ("conformance.v1.Holder", false) => g::Holder::verified(bytes).map(|v| {
            assert_eq!(holder_checked(&v.reader()), holder_verified(&v), "view ≠ reader");
            holder_verified(&v)
        }),
        (s, _) => panic!("unknown reader schema `{s}`"),
    }
}

#[test]
fn conformance_vectors_generated() {
    let vs = vectors();
    // Every vector, of any reader table (not only `Root`).
    assert_eq!(vs.len(), graven_conformance::vectors::cases().len(), "vectors on disk ≠ cases()");
    let mut ok = 0;
    for (name, bytes, j) in &vs {
        let got = evaluate(bytes, &j.schema, j.enveloped);
        match (&j.expect, got) {
            (ExpectJson::Value(want), Ok(v)) => assert_eq!(&v, want, "{name}: value"),
            (ExpectJson::Error(want), Err(e)) => assert_eq!(&error_name(&e), want, "{name}: erro ({e:?})"),
            (want, got) => panic!("{name}: esperava {want:?}, veio {got:?}"),
        }
        ok += 1;
    }
    eprintln!("conformance_vectors_generated: {ok}/{} vectors", vs.len());
}

/// Review focus #1 and #5, by name.
#[test]
fn self_cycle_and_unknown_union_tag() {
    let b = fs::read(dir().join("self_cycle.bin")).unwrap();
    assert_eq!(g::Root::verified(&b).err(), Some(Error::DepthExceeded));
    let b = fs::read(dir().join("unknown_union_tag.bin")).unwrap();
    let v = g::Root::verified(&b).unwrap();
    assert!(matches!(v.choice().unwrap(), Some(g::ChoiceVerified::Unknown(5))));
    assert!(matches!(v.reader().choice().unwrap(), Some(g::Choice::Unknown(5))));
}

// ---------------------------------------------------- Value ⇒ generated writer

fn get<'v>(m: &'v BTreeMap<String, Value>, k: &str) -> Option<&'v Value> {
    m.get(k)
}

fn uint(v: &Value) -> u128 {
    match v {
        Value::UInt(x) => *x,
        o => panic!("expected UInt, got {o:?}"),
    }
}

fn int(v: &Value) -> i128 {
    match v {
        Value::Int(x) => *x,
        o => panic!("expected Int, got {o:?}"),
    }
}

fn map(v: &Value) -> &BTreeMap<String, Value> {
    match v {
        Value::Table(m) => m,
        o => panic!("expected Table, got {o:?}"),
    }
}

fn list(v: &Value) -> &[Value] {
    match v {
        Value::List(l) => l,
        o => panic!("expected List, got {o:?}"),
    }
}

fn to_point(v: &Value) -> g::Point {
    let m = map(v);
    g::Point { x: int(&m["x"]) as i32, y: int(&m["y"]) as i32 }
}

fn write_leaf(w: &mut g::LeafW<'_>, v: &Value) {
    let m = map(v);
    w.n(uint(&m["n"]) as u32);
    if let Some(Value::Str(s)) = get(m, "s") {
        w.s(s);
    }
    if let Some(next) = get(m, "next") {
        write_leaf(&mut w.next(), next);
    }
}

/// Root fields of `v`, in ID order, through the generated writer. `a_u32` equal to its
/// default (7) is taken as absent (that is how the vectors that omit it read back).
macro_rules! write_root_fields {
    ($w:expr, $v:expr, $streaming:expr) => {{
        let w = $w;
        let m = map($v);
        if let Some(x) = get(m, "a_u8") {
            w.a_u8(uint(x) as u8);
        }
        if let Some(x) = get(m, "a_i16") {
            w.a_i16(int(x) as i16);
        }
        if let Some(x) = get(m, "a_u32").filter(|x| uint(x) != 7) {
            w.a_u32(uint(x) as u32);
        }
        if let Some(x) = get(m, "a_i64") {
            w.a_i64(int(x) as i64);
        }
        if let Some(x) = get(m, "a_u128") {
            w.a_u128(uint(x));
        }
        if let Some(x) = get(m, "a_i128") {
            w.a_i128(int(x));
        }
        if let Some(Value::F32(x)) = get(m, "a_f32") {
            w.a_f32(*x);
        }
        if let Some(Value::F64(x)) = get(m, "a_f64") {
            w.a_f64(*x);
        }
        if let Some(Value::Bool(x)) = get(m, "flag") {
            w.flag(*x);
        }
        if let Some(Value::Bool(x)) = get(m, "flag2") {
            w.flag2(*x);
        }
        if let Some(x) = get(m, "color") {
            w.color(g::Color::from_raw(uint(x) as u8));
        }
        if let Some(x) = get(m, "point") {
            w.point(to_point(x));
        }
        if let Some(Value::Str(s)) = get(m, "name") {
            w.name(s);
        }
        if let Some(Value::Bytes(b)) = get(m, "blob") {
            w.blob(b);
        }
        if let Some(x) = get(m, "nums") {
            let v: Vec<u16> = list(x).iter().map(|e| uint(e) as u16).collect();
            w.nums(&v);
        }
        if let Some(x) = get(m, "points") {
            let v: Vec<g::Point> = list(x).iter().map(to_point).collect();
            w.points(&v);
        }
        if let Some(x) = get(m, "leaves") {
            if $streaming {
                let mut l = w.leaves_streaming();
                for e in list(x) {
                    write_leaf(&mut l.push(), e);
                }
                l.close();
            } else {
                let mut l = w.leaves(list(x).len() as u32);
                for e in list(x) {
                    write_leaf(&mut l.push(), e);
                }
            }
        }
        if let Some(x) = get(m, "names") {
            w.names(list(x).iter().map(|e| match e {
                Value::Str(s) => s.as_str(),
                o => panic!("{o:?}"),
            }));
        }
        if let Some(x) = get(m, "blobs") {
            w.blobs(list(x).iter().map(|e| match e {
                Value::Bytes(b) => b.as_slice(),
                o => panic!("{o:?}"),
            }));
        }
        if let Some(x) = get(m, "child") {
            write_leaf(&mut w.child(), x);
        }
        if let Some(Value::Union { tag, value }) = get(m, "choice") {
            assert_eq!(*tag, 1, "the typed writer only writes known tags");
            write_leaf(&mut w.choice_leaf(), value);
        }
        m
    }};
}

fn write_root(v: &Value, streaming: bool) -> Vec<u8> {
    let pool = SimplePool::new(1 << 16);
    let mut b = Builder::new(&pool);
    write_root_fields!(&mut g::RootW::root(&mut b), v, streaming);
    b.finished().unwrap().to_vec()
}

fn write_root_enveloped(v: &Value) -> Vec<u8> {
    let pool = SimplePool::new(1 << 16);
    let mut b = Builder::new_enveloped(&pool);
    g::RootW::to_bytes(&mut b, |w| {
        write_root_fields!(w, v, false);
    })
    .unwrap()
    .to_vec()
}

/// Valid vectors a typed writer of the current `Root` cannot produce: an unknown union
/// tag, a `RootV2` writer (`extra` is not in the read value), an old writer with
/// `old @21` (now reserved), and the §10.3 vectors, which are built byte by byte on
/// purpose (non-canonical bools, null list elements, cut `fixed_len`, other tables, two
/// envelopes).
const NOT_TYPED_WRITABLE: &[&str] = &[
    "unknown_union_tag",
    "v2_read_by_v1",
    "reserved_ignored",
    "bool_struct_and_lists",
    "strlist_null_elem",
    "union_tag_null_ptr_optional",
    "reserved_slot_partial",
    "zero_size_slot_at_end",
    "table_list_elem_wider",
    "table_list_elem_narrower",
    "env_concat",
];

#[test]
fn generated_writer_reproduces_vectors() {
    let mut reproduced = Vec::new();
    let mut valid = 0;
    for (name, bytes, j) in vectors() {
        let ExpectJson::Value(v) = &j.expect else { continue };
        valid += 1;
        if NOT_TYPED_WRITABLE.contains(&name.as_str()) {
            continue;
        }
        let got = if j.enveloped { write_root_enveloped(v) } else { write_root(v, name == "streaming_leaves") };
        assert!(got == bytes, "{name}: the generated writer does not reproduce the .bin");
        reproduced.push(name);
    }
    eprintln!("generated writer reproduces {}/{valid} valid vectors: {reproduced:?}", reproduced.len());
    assert_eq!(reproduced.len(), valid - NOT_TYPED_WRITABLE.len());
}

/// The extra `RootV2` field round-trips through the generated `RootV2W`, and a `Root`
/// reader ignores it (like `v2_read_by_v1`).
#[test]
fn root_v2_writer_roundtrip() {
    let want = fs::read(dir().join("v2_read_by_v1.bin")).unwrap();
    let pool = SimplePool::new(4096);
    let mut b = Builder::new(&pool);
    g::RootV2W::root(&mut b).a_u8(42).flag2(true).name("v1").extra(0x1122_3344_5566_7788);
    let got = b.finished().unwrap().to_vec();
    assert_eq!(got, want, "RootV2W reproduz v2_read_by_v1.bin");
    assert_eq!(g::RootV2::verified(&got).unwrap().extra(), Some(0x1122_3344_5566_7788));
    assert_eq!(g::Root::verified(&got).unwrap().name().unwrap(), Some("v1"));
}

// ------------------------------------------------------------- mutation gate

/// Deterministic xorshift64* PRNG.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn coin(&mut self) -> bool {
        self.below(2) == 0
    }
    fn string(&mut self) -> String {
        const CHARS: &[char] = &['a', 'Z', '0', ' ', 'é', 'ç', '€', '中', '𝄞', '\u{0}'];
        (0..self.below(10)).map(|_| CHARS[self.below(CHARS.len())]).collect()
    }
}

fn rand_leaf(rng: &mut Rng, depth: usize) -> Value {
    let mut m = vec![("n", Value::UInt(rng.next() as u32 as u128))];
    if rng.coin() {
        m.push(("s", Value::Str(rng.string())));
    }
    if depth > 0 && rng.coin() {
        m.push(("next", rand_leaf(rng, depth - 1)));
    }
    tbl(m)
}

/// A random valid `Root` value (each optional field present with probability ½).
fn rand_root(rng: &mut Rng) -> Value {
    let mut m = vec![("a_u8", Value::UInt(rng.next() as u8 as u128)), ("flag2", Value::Bool(rng.coin()))];
    let mut opt = |rng: &mut Rng, k: &'static str, f: &dyn Fn(&mut Rng) -> Value| {
        if rng.coin() {
            m.push((k, f(rng)));
        }
    };
    opt(rng, "a_i16", &|r| Value::Int(r.next() as i16 as i128));
    opt(rng, "a_u32", &|r| Value::UInt(r.next() as u32 as u128));
    opt(rng, "a_i64", &|r| Value::Int(r.next() as i64 as i128));
    opt(rng, "a_u128", &|r| Value::UInt(((r.next() as u128) << 64) | r.next() as u128));
    opt(rng, "a_i128", &|r| Value::Int((((r.next() as u128) << 64) | r.next() as u128) as i128));
    opt(rng, "a_f32", &|r| Value::F32(f32::from_bits(r.next() as u32)));
    opt(rng, "a_f64", &|r| Value::F64(f64::from_bits(r.next())));
    opt(rng, "flag", &|r| Value::Bool(r.coin()));
    opt(rng, "color", &|r| Value::UInt(r.below(5) as u128));
    opt(rng, "point", &|r| tbl(vec![("x", Value::Int(r.next() as i32 as i128)), ("y", Value::Int(r.next() as i32 as i128))]));
    opt(rng, "name", &|r| Value::Str(r.string()));
    opt(rng, "blob", &|r| Value::Bytes((0..r.below(8)).map(|_| r.next() as u8).collect()));
    opt(rng, "nums", &|r| Value::List((0..r.below(5)).map(|_| Value::UInt(r.next() as u16 as u128)).collect()));
    opt(rng, "points", &|r| {
        Value::List(
            (0..r.below(4))
                .map(|_| tbl(vec![("x", Value::Int(r.next() as i32 as i128)), ("y", Value::Int(r.next() as i32 as i128))]))
                .collect(),
        )
    });
    opt(rng, "leaves", &|r| Value::List((0..r.below(4)).map(|_| rand_leaf(r, 2)).collect()));
    opt(rng, "names", &|r| Value::List((0..r.below(4)).map(|_| Value::Str(r.string())).collect()));
    opt(rng, "blobs", &|r| Value::List((0..r.below(4)).map(|_| Value::Bytes((0..r.below(5)).map(|_| r.next() as u8).collect())).collect()));
    opt(rng, "child", &|r| rand_leaf(r, 3));
    opt(rng, "choice", &|r| Value::Union { tag: 1, value: Box::new(rand_leaf(r, 1)) });
    tbl(m)
}

/// All strings of a value (to target their payload bytes).
fn strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Str(s) => out.push(s.clone()),
        Value::List(l) => l.iter().for_each(|e| strings(e, out)),
        Value::Table(m) => m.values().for_each(|e| strings(e, out)),
        Value::Union { value, .. } => strings(value, out),
        _ => {}
    }
}

/// Acceptance gate of the codegen invariants on the generated `Root` (all views: strings,
/// `VStrList`, `VListR<LeafVerified>`, the union, `child.next`): mutated valid messages;
/// whenever the generated `verified` accepts, the verified view reads exactly what the
/// checked reader reads, and both match the interpreter (which must accept too).
#[test]
fn mutation_gate_generated() {
    // Under Miri, a smaller sample (same code exercised; Miri is ~1000× slower).
    const CASES: usize = if cfg!(miri) { 64 } else { 4096 };
    let root_fixed = g::root::FIXED_LEN as usize;
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut ok, mut ok_noncritical, mut ok_any) = (0usize, 0usize, 0usize);
    for case in 0..CASES {
        let v = rand_root(&mut rng);
        let mut buf = write_root(&v, rng.coin());
        let base = g::Root::verified(&buf).unwrap_or_else(|e| panic!("case {case}: base does not verify: {e:?}"));
        assert_eq!(to_value(&base), read(&buf, conformance_root()).unwrap(), "case {case}: base");

        let noncritical = case % 2 == 0;
        let mut targets: Vec<usize> = Vec::new();
        if noncritical {
            // The root's fixed part (scalars, bits, pointers) plus string payload bytes.
            targets.extend(2..2 + root_fixed);
            let mut ss = Vec::new();
            strings(&v, &mut ss);
            for s in ss.iter().filter(|s| !s.is_empty()) {
                if let Some(p) = buf.windows(s.len()).position(|w| w == s.as_bytes()) {
                    targets.extend(p..p + s.len());
                }
            }
        } else {
            targets.extend(0..buf.len());
        }
        for _ in 0..1 + rng.below(3) {
            let p = targets[rng.below(targets.len())];
            buf[p] = rng.next() as u8;
        }
        if !noncritical && rng.below(4) == 0 {
            let n = rng.below(buf.len() + 1);
            buf.truncate(n);
        }

        let interp = read(&buf, conformance_root());
        match g::Root::verified(&buf) {
            Ok(view) => {
                ok += 1;
                if noncritical {
                    ok_noncritical += 1;
                } else {
                    ok_any += 1;
                }
                let checked = g::Root::read(&buf).expect("verified Ok ⇒ checked read Ok");
                let a = to_value(&view);
                assert_eq!(a, to_value_checked(&checked), "case {case}: view ≠ checked reader");
                assert_eq!(Ok(&a), interp.as_ref(), "case {case}: generated ≠ interpreter");
            }
            Err(e) => {
                assert!(interp.is_err(), "case {case}: generated refused ({e:?}) but the interpreter accepted");
            }
        }
    }
    eprintln!("mutation gate (generated): {CASES} cases, verified Ok = {ok} (non-critical: {ok_noncritical}, anywhere: {ok_any})");
    assert!(ok > 0, "nenhum mutante verificou Ok");
    assert!(ok_noncritical > CASES / 8, "too few non-critical mutants Ok: {ok_noncritical}");
    assert!(ok_any > 0, "no 'anywhere' mutant Ok");
}

/// Mutation gate over the valid `Extra`/`Holder` vectors (§10.3): every byte, with
/// three masks. Generated code and interpreter accept/refuse together; when they accept, the
/// verified view reads the same as the checked reader and the interpreter.
#[test]
fn mutation_gate_extra_holder() {
    let mut checked = 0usize;
    for (name, bytes, j) in vectors() {
        if j.enveloped || !matches!(j.expect, ExpectJson::Value(_)) {
            continue;
        }
        let def = match j.schema.as_str() {
            "conformance.v1.Extra" => graven_conformance::conformance_extra(),
            "conformance.v1.Holder" => graven_conformance::conformance_holder(),
            _ => continue,
        };
        // Under Miri, one byte in 5 (same paths, smaller sample).
        for i in (0..bytes.len()).step_by(if cfg!(miri) { 5 } else { 1 }) {
            for mask in [0x01u8, 0x80, 0xff] {
                let mut b = bytes.clone();
                b[i] ^= mask;
                let interp = read(&b, def);
                let generated = evaluate(&b, &j.schema, false);
                match (&interp, &generated) {
                    (Ok(a), Ok(g)) => assert_eq!(a, g, "{name}: byte {i} ^ {mask:#x}"),
                    (Err(_), Err(_)) => {}
                    _ => panic!("{name}: byte {i} ^ {mask:#x}: interp {interp:?} × generated {generated:?}"),
                }
                checked += 1;
            }
        }
    }
    assert!(checked > if cfg!(miri) { 200 } else { 1000 }, "too few mutations: {checked}");
}
