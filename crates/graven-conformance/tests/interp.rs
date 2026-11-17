use proptest::prelude::*;
use std::collections::BTreeMap;
use graven::{Builder, Error, SimplePool, TableRef};
use graven_conformance::{conformance_extra, conformance_holder, 
    StructDef, TableDef, Ty, Value, bank_transaction, conformance_leaf, conformance_root, conformance_root_v2,
    game_frame, layout, read, validate, write,
};
use graven_layout::{FieldKind, compute_layout};

// ---------------------------------------------------------------- helpers

fn table(pairs: Vec<(&str, Value)>) -> Value {
    Value::Table(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn encode(v: &Value, def: &TableDef) -> Vec<u8> {
    let pool = SimplePool::new(256);
    let mut b = Builder::new(&pool);
    write(v, def, &mut b).unwrap();
    b.finished().unwrap().to_vec()
}

fn minimal_root() -> Vec<(&'static str, Value)> {
    vec![("a_u8", Value::UInt(1)), ("flag2", Value::Bool(false))]
}

fn with_default(mut v: Value) -> Value {
    if let Value::Table(m) = &mut v {
        m.entry("a_u32".into()).or_insert(Value::UInt(7));
    }
    v
}

// ----------------------------------------------------------------- layout

#[test]
fn layout_matches_runtime() {
    assert_eq!(
        layout(conformance_leaf()),
        compute_layout(&[FieldKind::Scalar { size: 4 }, FieldKind::Ptr, FieldKind::Ptr])
    );
    // Root: reserved `old` keeps its u32 slot; RootV2 extends Root without moving anything.
    let r = layout(conformance_root());
    let r2 = layout(conformance_root_v2());
    assert_eq!(r.fields.len(), 22);
    assert_eq!(&r2.fields[..22], &r.fields[..]);
    assert!(r2.fixed_len > r.fixed_len);
    let point = conformance_root().index_of("point").unwrap();
    let choice = conformance_root().index_of("choice").unwrap();
    let flag = conformance_root().index_of("flag").unwrap();
    assert!(r.fields[point].presence.is_some());
    assert_eq!(r.fields[flag].slot, None);
    assert_eq!(r.fields[choice + 1].slot.unwrap() - r.fields[choice].slot.unwrap(), 6);
}

// -------------------------------------------------------------- roundtrip

fn opt<T: std::fmt::Debug + Clone>(s: impl Strategy<Value = T>) -> impl Strategy<Value = Option<T>> {
    prop::option::of(s)
}

fn leaf_value() -> impl Strategy<Value = Value> {
    let flat = (any::<u32>(), opt(".{0,8}")).prop_map(|(n, s)| leaf(n, s, None));
    // `next` nests up to 3 levels.
    flat.prop_recursive(3, 4, 1, |inner| {
        (any::<u32>(), opt(".{0,8}"), opt(inner)).prop_map(|(n, s, next)| leaf(n, s, next))
    })
}

fn leaf(n: u32, s: Option<String>, next: Option<Value>) -> Value {
    let mut m = BTreeMap::new();
    m.insert("n".to_string(), Value::UInt(n as u128));
    if let Some(s) = s {
        m.insert("s".to_string(), Value::Str(s));
    }
    if let Some(next) = next {
        m.insert("next".to_string(), next);
    }
    Value::Table(m)
}

fn point_value() -> impl Strategy<Value = Value> {
    (any::<i32>(), any::<i32>())
        .prop_map(|(x, y)| table(vec![("x", Value::Int(x as i128)), ("y", Value::Int(y as i128))]))
}

fn choice_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        leaf_value().prop_map(|l| Value::Union { tag: 1, value: Box::new(l) }),
        (2u16..).prop_map(|tag| Value::Union { tag, value: Box::new(Value::Table(BTreeMap::new())) }),
    ]
}

fn root_value() -> impl Strategy<Value = Value> {
    let scalars = (
        any::<u8>(),
        opt(any::<i16>()),
        opt(any::<u32>()),
        opt(any::<i64>()),
        opt(any::<u128>()),
        opt(any::<i128>()),
        opt(any::<u32>().prop_map(f32::from_bits)),
        opt(any::<u64>().prop_map(f64::from_bits)),
        opt(any::<bool>()),
        any::<bool>(),
        opt(any::<u8>()),
        opt(point_value()),
    );
    let vars = (
        opt(".{0,12}"),
        opt(prop::collection::vec(any::<u8>(), 0..12)),
        opt(prop::collection::vec(any::<u16>(), 0..6)),
        opt(prop::collection::vec(point_value(), 0..4)),
        opt(prop::collection::vec(leaf_value(), 0..4)),
        opt(prop::collection::vec(".{0,6}", 0..4)),
        opt(prop::collection::vec(prop::collection::vec(any::<u8>(), 0..6), 0..4)),
        opt(leaf_value()),
        opt(choice_value()),
    );
    (scalars, vars).prop_map(|(s, v)| {
        let mut m = BTreeMap::new();
        let mut put = |k: &str, x: Option<Value>| {
            if let Some(x) = x {
                m.insert(k.to_string(), x);
            }
        };
        put("a_u8", Some(Value::UInt(s.0 as u128)));
        put("a_i16", s.1.map(|x| Value::Int(x as i128)));
        put("a_u32", s.2.map(|x| Value::UInt(x as u128)));
        put("a_i64", s.3.map(|x| Value::Int(x as i128)));
        put("a_u128", s.4.map(Value::UInt));
        put("a_i128", s.5.map(Value::Int));
        put("a_f32", s.6.map(Value::F32));
        put("a_f64", s.7.map(Value::F64));
        put("flag", s.8.map(Value::Bool));
        put("flag2", Some(Value::Bool(s.9)));
        put("color", s.10.map(|x| Value::UInt(x as u128)));
        put("point", s.11);
        put("name", v.0.map(Value::Str));
        put("blob", v.1.map(Value::Bytes));
        put("nums", v.2.map(|l| Value::List(l.into_iter().map(|x| Value::UInt(x as u128)).collect())));
        put("points", v.3.map(Value::List));
        put("leaves", v.4.map(Value::List));
        put("names", v.5.map(|l| Value::List(l.into_iter().map(Value::Str).collect())));
        put("blobs", v.6.map(|l| Value::List(l.into_iter().map(Value::Bytes).collect())));
        put("child", v.7);
        put("choice", v.8);
        Value::Table(m)
    })
}

proptest! {
    #[test]
    fn roundtrip_all_fields(v in root_value()) {
        let buf = encode(&v, conformance_root());
        prop_assert_eq!(read(&buf, conformance_root()).unwrap(), with_default(v.clone()));
        // Same bytes read by the V2 schema: `extra` is simply absent.
        prop_assert_eq!(read(&buf, conformance_root_v2()).unwrap(), with_default(v));
    }

    #[test]
    fn random_bytes_never_panic(buf in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = read(&buf, conformance_root());
        let _ = read(&buf, conformance_root_v2());
        let _ = read(&buf, bank_transaction());
    }

    #[test]
    fn mutated_messages_never_panic(
        v in root_value(),
        flips in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
    ) {
        let mut buf = encode(&v, conformance_root());
        for (i, x) in flips {
            let i = i.index(buf.len());
            buf[i] = x;
        }
        let _ = read(&buf, conformance_root());
        let _ = read(&buf, conformance_root_v2());
    }

    #[test]
    fn json_roundtrip(v in root_value()) {
        let s = serde_json::to_string(&v).unwrap();
        let back: Value = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(back, v);
    }
}

// ------------------------------------- parity with the generated `bank` code

#[test]
fn matches_generated_bank() {
    use graven_schemas::bank::{Currency, Transaction, TransactionW};
    let pool = SimplePool::new(256);
    let mut b = Builder::new(&pool);
    {
        let mut t = TransactionW::root(&mut b);
        // Fields in ID order, so the interpreter (which writes in ID order) can be
        // compared byte for byte below.
        t.id(77).ts(1_700_000_000).currency(Currency::Unknown(9)).from_shard(3).to_shard(u32::MAX);
        {
            let mut l = t.entries(3);
            l.push().account(1).amount(-500).memo("débito");
            l.push().account(2).amount(i64::MIN);
            l.push().account(u64::MAX).amount(0).memo("");
        }
        t.note("pix");
    }
    let buf = b.finished().unwrap().to_vec();

    // Interpreter reads what the generated code wrote, value by value.
    let v = read(&buf, bank_transaction()).unwrap();
    let h = Transaction::verified(&buf).unwrap();
    let mut entries = Vec::new();
    for e in h.entries().iter() {
        let mut m = vec![("account", Value::UInt(e.account() as u128)), ("amount", Value::Int(e.amount() as i128))];
        if let Some(s) = e.memo() {
            m.push(("memo", Value::Str(s.into())));
        }
        entries.push(table(m));
    }
    let want = table(vec![
        ("id", Value::UInt(h.id() as u128)),
        ("ts", Value::UInt(h.ts() as u128)),
        ("currency", Value::UInt(h.currency().to_raw() as u128)),
        ("from_shard", Value::UInt(h.from_shard() as u128)),
        ("to_shard", Value::UInt(h.to_shard() as u128)),
        ("entries", Value::List(entries)),
        ("note", Value::Str(h.note().unwrap().into())),
    ]);
    assert_eq!(v, want);

    // And the interpreter writes the same bytes the generated writer does.
    let mine = encode(&v, bank_transaction());
    assert!(Transaction::verified(&mine).is_ok());
    assert_eq!(mine, buf);
}

// ------------------------------------------------------ reader semantics

#[test]
fn missing_required() {
    let v = table(vec![("flag2", Value::Bool(true))]);
    assert_eq!(read(&encode(&v, conformance_root()), conformance_root()), Err(Error::Missing { field: "a_u8" }));
    let v = table(vec![("a_u8", Value::UInt(0))]);
    assert_eq!(read(&encode(&v, conformance_root()), conformance_root()), Err(Error::Missing { field: "flag2" }));
    // Required field of a nested table.
    let mut m = minimal_root();
    m.push(("child", table(vec![("s", Value::Str("x".into()))])));
    assert_eq!(read(&encode(&table(m), conformance_root()), conformance_root()), Err(Error::Missing { field: "n" }));
}

#[test]
fn absent_optional_and_default() {
    let buf = encode(&table(minimal_root()), conformance_root());
    let got = read(&buf, conformance_root()).unwrap();
    let mut want = minimal_root();
    want.push(("a_u32", Value::UInt(7)));
    assert_eq!(got, table(want));
    // A present 0 is distinct from absent (and from the default).
    let mut m = minimal_root();
    m.push(("a_u32", Value::UInt(0)));
    m.push(("a_i16", Value::Int(0)));
    let got = read(&encode(&table(m.clone()), conformance_root()), conformance_root()).unwrap();
    assert_eq!(got, table(m));
}

#[test]
fn partial_optional_slot_is_truncated() {
    // Root fixed part: [bits][a_u8 @1][a_i16 @2..4]...; fixed_len 3 cuts a_i16 in half.
    let buf = [3u8, 0, 0b01, 5, 0];
    assert_eq!(read(&buf, conformance_root()), Err(Error::Truncated));
}

#[test]
fn short_fixed_len_reads_absent() {
    // Old writer that only knew a_u8 and flag2? Not expressible; instead a writer whose
    // fixed part stops right after a_u8: everything else absent ⇒ Missing flag2.
    let buf = [2u8, 0, 0b01, 5];
    assert_eq!(read(&buf, conformance_root()), Err(Error::Missing { field: "flag2" }));
}

#[test]
fn unknown_enum_and_union_tag() {
    let mut m = minimal_root();
    m.push(("color", Value::UInt(200)));
    m.push(("choice", Value::Union { tag: 42, value: Box::new(Value::Table(BTreeMap::new())) }));
    let got = read(&encode(&table(m.clone()), conformance_root()), conformance_root()).unwrap();
    m.push(("a_u32", Value::UInt(7)));
    assert_eq!(got, table(m));
}

#[test]
fn unknown_union_tag_is_not_descended() {
    // Write a known variant whose table is missing its required field, then flip the tag:
    // the reader must not visit the payload.
    let mut m = minimal_root();
    m.push(("choice", Value::Union { tag: 1, value: Box::new(table(vec![])) }));
    let mut buf = encode(&table(m), conformance_root());
    assert_eq!(read(&buf, conformance_root()), Err(Error::Missing { field: "n" }));
    let slot = layout(conformance_root()).fields[conformance_root().index_of("choice").unwrap()].slot.unwrap();
    let tag_at = 2 + slot as usize;
    buf[tag_at..tag_at + 2].copy_from_slice(&7u16.to_le_bytes());
    let got = read(&buf, conformance_root()).unwrap();
    let Value::Table(got) = got else { panic!() };
    assert_eq!(got["choice"], Value::Union { tag: 7, value: Box::new(Value::Table(BTreeMap::new())) });
}

#[test]
fn invalid_utf8_is_rejected() {
    let mut m = minimal_root();
    m.push(("name", Value::Str("ok".into())));
    let mut buf = encode(&table(m), conformance_root());
    let n = buf.len();
    buf[n - 1] = 0xFF;
    assert_eq!(read(&buf, conformance_root()), Err(Error::InvalidUtf8));
}

#[test]
fn evolution_v2_extra_field() {
    let mut m = minimal_root();
    m.push(("extra", Value::UInt(u64::MAX as u128)));
    let buf = encode(&table(m.clone()), conformance_root_v2());
    // Old reader ignores the bytes it does not know.
    assert_eq!(read(&buf, conformance_root()).unwrap(), with_default(table(minimal_root())));
    assert_eq!(read(&buf, conformance_root_v2()).unwrap(), with_default(table(m)));
}

#[test]
#[should_panic(expected = "reserved")]
fn writing_reserved_field_panics() {
    let mut m = minimal_root();
    m.push(("old", Value::UInt(1)));
    encode(&table(m), conformance_root());
}

// ------------------------------------------------------------ verify limits

#[test]
fn self_referencing_child_is_depth_exceeded() {
    // Root.child → Leaf whose `next` points back at the Leaf itself.
    let mut m = minimal_root();
    m.push(("child", leaf(1, None, None)));
    let mut buf = encode(&table(m), conformance_root());
    assert!(read(&buf, conformance_root()).is_ok());
    let child_slot = layout(conformance_root()).fields[conformance_root().index_of("child").unwrap()].slot.unwrap();
    let next_slot = layout(conformance_leaf()).fields[conformance_leaf().index_of("next").unwrap()].slot.unwrap();
    let leaf_ref = TableRef::at(&buf, 0).unwrap().table(child_slot).unwrap().unwrap();
    let header = leaf_ref.fixed_start() - 2;
    let at = leaf_ref.fixed_start() + next_slot as usize;
    buf[at..at + 4].copy_from_slice(&((header as i64 - at as i64) as i32).to_le_bytes());
    assert_eq!(read(&buf, conformance_root()), Err(Error::DepthExceeded));
}

#[test]
fn many_pointers_to_one_string_is_budget_exceeded() {
    const N: usize = 20;
    let mut names = vec![Value::Str("a".repeat(200_000))];
    names.extend((1..N).map(|_| Value::Str(String::new())));
    let mut m = minimal_root();
    m.push(("names", Value::List(names)));
    let mut buf = encode(&table(m), conformance_root());
    assert!(read(&buf, conformance_root()).is_ok());

    let slot = layout(conformance_root()).fields[conformance_root().index_of("names").unwrap()].slot.unwrap();
    let p = TableRef::at(&buf, 0).unwrap().ptr(slot).unwrap().unwrap();
    let s0 = p + 4;
    let target = s0 as i64 + i32::from_le_bytes(buf[s0..s0 + 4].try_into().unwrap()) as i64;
    for i in 1..N {
        let si = p + 4 + 4 * i;
        buf[si..si + 4].copy_from_slice(&((target - si as i64) as i32).to_le_bytes());
    }
    assert_eq!(read(&buf, conformance_root()), Err(Error::BudgetExceeded));
}

// ------------------------------------------------------------------- JSON

#[test]
fn json_representation_is_exact() {
    let v = table(vec![
        ("f", Value::F32(1.5)),
        ("d", Value::F64(-0.0)),
        ("u", Value::UInt(u128::MAX)),
        ("i", Value::Int(i128::MIN)),
        ("b", Value::Bytes(vec![0, 0xAB])),
        ("s", Value::Str("x".into())),
        ("t", Value::Bool(true)),
        ("l", Value::List(vec![])),
        ("c", Value::Union { tag: 1, value: Box::new(table(vec![])) }),
    ]);
    let j = serde_json::to_value(&v).unwrap();
    let t = &j["Table"];
    assert_eq!(t["f"], serde_json::json!({"F32": "f32:0x3fc00000"}));
    assert_eq!(t["d"], serde_json::json!({"F64": "f64:0x8000000000000000"}));
    assert_eq!(t["u"], serde_json::json!({"UInt": u128::MAX.to_string()}));
    assert_eq!(t["i"], serde_json::json!({"Int": i128::MIN.to_string()}));
    assert_eq!(t["b"], serde_json::json!({"Bytes": "00ab"}));
    assert_eq!(t["s"], serde_json::json!({"Str": "x"}));
    assert_eq!(t["t"], serde_json::json!({"Bool": true}));
    assert_eq!(t["l"], serde_json::json!({"List": []}));
    assert_eq!(t["c"], serde_json::json!({"Union": {"tag": 1, "value": {"Table": {}}}}));
    // NaN payloads survive (bitwise equality).
    let nan = Value::F32(f32::from_bits(0x7fc0_0001));
    let back: Value = serde_json::from_str(&serde_json::to_string(&nan).unwrap()).unwrap();
    assert_eq!(back, nan);
    assert_ne!(Value::F64(0.0), Value::F64(-0.0));
}

#[test]
fn bytes_list_utf8_and_binary() {
    for blobs in [vec![b"ab".to_vec(), vec![]], vec![vec![0xFF, 0], vec![], vec![1, 2, 3]]] {
        let mut m = minimal_root();
        m.push(("blobs", Value::List(blobs.iter().cloned().map(Value::Bytes).collect())));
        let buf = encode(&table(m.clone()), conformance_root());
        assert_eq!(read(&buf, conformance_root()).unwrap(), with_default(table(m)));
        // The runtime's own StrList reads it too.
        let slot = layout(conformance_root()).fields[conformance_root().index_of("blobs").unwrap()].slot.unwrap();
        let l = TableRef::at(&buf, 0).unwrap().strs(slot).unwrap().unwrap();
        let got: Vec<Vec<u8>> = (0..l.len()).map(|i| l.get_bytes(i).unwrap().to_vec()).collect();
        assert_eq!(got, blobs);
    }
}

// -------------------------------------------------------------- validation

#[test]
fn all_models_validate() {
    for def in [conformance_root(), conformance_root_v2(), conformance_leaf(), conformance_extra(), conformance_holder(), bank_transaction(), game_frame()] {
        validate(def).unwrap_or_else(|e| panic!("{}: {e}", def.name));
    }
}

#[test]
fn validate_rejects_bad_models() {
    use graven_conformance::{Field, Presence};
    // `static` instead of `Box::leak`: Miri reports a leak.
    static BAD_STRUCT: std::sync::LazyLock<StructDef> =
        std::sync::LazyLock::new(|| StructDef { name: "S", fields: vec![("a", Ty::I32)], size: 5 });
    let bad_struct: &'static StructDef = &BAD_STRUCT;
    let mk = |fields: Vec<Field>| TableDef { name: "T", fields };
    let fld = |id, ty| Field { id, name: "f", ty, presence: Presence::Optional };
    assert!(validate(&mk(vec![fld(0, Ty::Struct(bad_struct))])).is_err());
    assert!(validate(&mk(vec![fld(0, Ty::Enum { base: Box::new(Ty::U64) })])).is_err());
    assert!(validate(&mk(vec![fld(0, Ty::U8), fld(2, Ty::U8)])).is_err());
    assert!(validate(&mk(vec![fld(1, Ty::U8), fld(0, Ty::U8)])).is_err());
    assert!(validate(&mk(vec![fld(0, Ty::U8), fld(1, Ty::Enum { base: Box::new(Ty::U16) })])).is_ok());
}
