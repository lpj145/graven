//! The frozen conformance vectors of format v1 (`testvectors/v1/` at the repo root).
//!
//! [`cases`] builds every vector deterministically; the `gen-vectors` binary writes them
//! to disk and `tests/conformance.rs` checks the files. A frozen vector never changes: a
//! change to the bytes or to the expectation is a new format version.
//!
//! Ordinary valid cases are written with [`crate::write`]. Cases a value-driven writer
//! cannot produce (streaming list, unknown union payload, a slot filled by an old writer)
//! are written directly with `TableW`. Every error case contains exactly **one** defect
//! (error precedence between several defects is not normative): it is a valid base
//! message plus one targeted mutation, and the unit tests check that the base reads Ok.

use crate::interp::{Value, bit, layout, slot};
use crate::model::{
    Presence, TableDef, conformance_extra, conformance_holder, conformance_leaf, conformance_root, conformance_root_v2,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use graven::envelope::{self, schema_id};
use graven::{Builder, Error, SimplePool, TableW};

pub const ROOT: &str = "conformance.v1.Root";
pub const ROOT_V2: &str = "conformance.v1.RootV2";
pub const EXTRA: &str = "conformance.v1.Extra";
pub const HOLDER: &str = "conformance.v1.Holder";
const LEAF: &str = "conformance.v1.Leaf";

/// Expected outcome of reading a vector.
#[derive(Debug, Clone, PartialEq)]
pub enum Expect {
    /// Reads successfully as this value.
    Value(Value),
    /// Fails with this `graven::Error` variant (name only, e.g. `"Missing"`).
    Error(&'static str),
}

/// One conformance vector.
#[derive(Debug, Clone)]
pub struct Case {
    /// File stem: `<name>.bin` / `<name>.json`.
    pub name: &'static str,
    /// Qualified name of the **reader** schema.
    pub schema: &'static str,
    /// `bytes` is an envelope (`graven::envelope`) whose payload is the message.
    pub enveloped: bool,
    pub bytes: Vec<u8>,
    pub expect: Expect,
}

/// The `.json` file of a vector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseJson {
    pub schema: String,
    pub enveloped: bool,
    pub expect: ExpectJson,
}

/// `{"value": <Value>}` or `{"error": "<variant>"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExpectJson {
    Value(Value),
    Error(String),
}

impl Case {
    pub fn json(&self) -> CaseJson {
        CaseJson {
            schema: self.schema.to_string(),
            enveloped: self.enveloped,
            expect: match &self.expect {
                Expect::Value(v) => ExpectJson::Value(v.clone()),
                Expect::Error(e) => ExpectJson::Error(e.to_string()),
            },
        }
    }

    /// Pretty-printed `.json` contents (with a trailing newline).
    pub fn json_text(&self) -> String {
        let mut s = serde_json::to_string_pretty(&self.json()).expect("CaseJson serializes");
        s.push('\n');
        s
    }
}

/// Reader schema by qualified name.
pub fn reader_schema(name: &str) -> Option<&'static TableDef> {
    match name {
        ROOT => Some(conformance_root()),
        ROOT_V2 => Some(conformance_root_v2()),
        EXTRA => Some(conformance_extra()),
        HOLDER => Some(conformance_holder()),
        _ => None,
    }
}

/// Dynamic equivalent of a generated `from_bytes`: envelope (if any) → schema id check
/// (`SchemaMismatch`) → full read + verify with the reader schema.
pub fn evaluate(bytes: &[u8], schema: &str, enveloped: bool) -> Result<Value, Error> {
    let def = reader_schema(schema).unwrap_or_else(|| panic!("unknown reader schema `{schema}`"));
    if !enveloped {
        return crate::read(bytes, def);
    }
    let env = envelope::decode(bytes)?;
    let expected = schema_id(schema);
    if env.schema_id != expected {
        return Err(Error::SchemaMismatch { expected, found: env.schema_id });
    }
    crate::read(env.payload, def)
}

/// `Missing { field: "a_u8" }` ⇒ `"Missing"`.
pub fn error_name(e: &Error) -> String {
    let s = format!("{e:?}");
    s.split(['{', '(']).next().unwrap_or("").trim().to_string()
}

/// Every vector, in a fixed order.
pub fn cases() -> Vec<Case> {
    built().into_iter().map(|b| b.case).collect()
}

/// A case plus, for error cases, the valid base it was mutated from.
pub(crate) struct Built {
    pub case: Case,
    /// Read by the unit tests (`error_bases_read_ok`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub base: Option<Vec<u8>>,
}

// ------------------------------------------------------------------ values

fn tbl(pairs: Vec<(&str, Value)>) -> Value {
    Value::Table(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn u(x: u128) -> Value {
    Value::UInt(x)
}

fn i(x: i128) -> Value {
    Value::Int(x)
}

fn s(x: &str) -> Value {
    Value::Str(x.to_string())
}

fn by(x: &[u8]) -> Value {
    Value::Bytes(x.to_vec())
}

fn point(x: i32, y: i32) -> Value {
    tbl(vec![("x", i(x as i128)), ("y", i(y as i128))])
}

fn leaf(n: u32, st: Option<&str>, next: Option<Value>) -> Value {
    let mut m = BTreeMap::new();
    m.insert("n".to_string(), u(n as u128));
    if let Some(st) = st {
        m.insert("s".to_string(), s(st));
    }
    if let Some(next) = next {
        m.insert("next".to_string(), next);
    }
    Value::Table(m)
}

/// Adds/overrides fields of a table value.
fn with(base: Value, pairs: Vec<(&str, Value)>) -> Value {
    let Value::Table(mut m) = base else { unreachable!() };
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    Value::Table(m)
}

/// What the reader yields for `v` written as is: the absent `a_u32` reads as its default 7.
fn read_view(v: &Value) -> Value {
    let Value::Table(mut m) = v.clone() else { unreachable!() };
    m.entry("a_u32".into()).or_insert(u(7));
    Value::Table(m)
}

/// Only the required fields.
fn minimal_value() -> Value {
    tbl(vec![("a_u8", u(42)), ("flag2", Value::Bool(true))])
}

fn all_fields_value() -> Value {
    tbl(vec![
        ("a_u8", u(200)),
        ("a_i16", i(-12345)),
        ("a_u32", u(4_000_000_000)),
        ("a_i64", i(-9_000_000_000_000_000_000)),
        ("a_u128", u(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210)),
        ("a_i128", i(i128::MIN)),
        ("a_f32", Value::F32(1.5)),
        ("a_f64", Value::F64(-1234.5678)),
        ("flag", Value::Bool(true)),
        ("flag2", Value::Bool(true)),
        ("color", u(2)),
        ("point", point(-1, i32::MAX)),
        ("name", s("grvn ✓ ação")),
        ("blob", by(&[0x00, 0x01, 0xfe, 0xff])),
        ("nums", Value::List(vec![u(0), u(1), u(65535)])),
        ("points", Value::List(vec![point(1, 2), point(-3, -4)])),
        (
            "leaves",
            Value::List(vec![
                leaf(1, Some("a"), None),
                leaf(2, None, None),
                leaf(3, Some("c"), Some(leaf(4, Some("d"), None))),
            ]),
        ),
        ("names", Value::List(vec![s(""), s("x"), s("olá")])),
        ("blobs", Value::List(vec![by(&[]), by(&[0xab]), by(&[1, 2, 3])])),
        ("child", leaf(10, Some("filho"), Some(leaf(11, None, Some(leaf(12, None, None)))))),
        ("choice", Value::Union { tag: 1, value: Box::new(leaf(20, Some("escolha"), None)) }),
    ])
}

/// Every optional scalar present with value 0 (and the required ones 0/false too).
fn zero_value() -> Value {
    tbl(vec![
        ("a_u8", u(0)),
        ("a_i16", i(0)),
        ("a_u32", u(0)),
        ("a_i64", i(0)),
        ("a_u128", u(0)),
        ("a_i128", i(0)),
        ("a_f32", Value::F32(0.0)),
        ("a_f64", Value::F64(0.0)),
        ("flag", Value::Bool(false)),
        ("flag2", Value::Bool(false)),
        ("color", u(0)),
        ("point", point(0, 0)),
    ])
}

fn empty_lists_value() -> Value {
    with(
        minimal_value(),
        vec![
            ("name", s("")),
            ("blob", by(&[])),
            ("nums", Value::List(vec![])),
            ("points", Value::List(vec![])),
            ("leaves", Value::List(vec![])),
            ("names", Value::List(vec![])),
            ("blobs", Value::List(vec![])),
        ],
    )
}

// ----------------------------------------------------------------- writing

fn encode(v: &Value, def: &TableDef) -> Vec<u8> {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new(&pool);
    crate::write(v, def, &mut b).expect("write");
    b.finished().expect("finished").to_vec()
}

fn encode_enveloped(v: &Value, def: &TableDef, id: u64) -> Vec<u8> {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new_enveloped(&pool);
    crate::write(v, def, &mut b).expect("write");
    b.flush_envelope(id).expect("flush_envelope").to_vec()
}

/// Message built by hand with `TableW` on the `Root` layout.
fn encode_with(f: impl FnOnce(&mut TableW<'_>)) -> Vec<u8> {
    encode_def_with(conformance_root(), f)
}

/// Message built by hand with `TableW` on the layout of `def`.
fn encode_def_with(def: &TableDef, f: impl FnOnce(&mut TableW<'_>)) -> Vec<u8> {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new(&pool);
    {
        let mut t = b.root(layout(def).fixed_len);
        f(&mut t);
    }
    b.finished().expect("finished").to_vec()
}

/// Slot / presence bit of a field of `def`.
fn field_of(def: &TableDef, name: &str) -> graven_layout::FieldLayout {
    layout(def).fields[def.index_of(name).unwrap_or_else(|| panic!("{}.{name}", def.name))]
}

/// Slot / presence bit of a `Root` field.
fn root_field(name: &str) -> graven_layout::FieldLayout {
    let r = conformance_root();
    layout(r).fields[r.index_of(name).unwrap_or_else(|| panic!("Root.{name}"))]
}

fn leaf_field(name: &str) -> graven_layout::FieldLayout {
    let l = conformance_leaf();
    layout(l).fields[l.index_of(name).unwrap_or_else(|| panic!("Leaf.{name}"))]
}

/// The required fields of `minimal_value()`, written by hand.
fn put_minimal(t: &mut TableW<'_>) {
    let (a, f2) = (root_field("a_u8"), root_field("flag2"));
    t.set::<u8>(slot(&a), bit(&a), 42);
    t.set_bool(bit(&f2), true);
}

/// A `Leaf` value written by hand into `t` (works for streaming elements too).
fn put_leaf(t: &mut TableW<'_>, v: &Value) {
    let Value::Table(m) = v else { unreachable!() };
    let n = as_u32(&m["n"]);
    let nf = leaf_field("n");
    t.set::<u32>(slot(&nf), bit(&nf), n);
    if let Some(Value::Str(st)) = m.get("s") {
        t.set_str(slot(&leaf_field("s")), st);
    }
    if let Some(next) = m.get("next") {
        let mut c = t.table(slot(&leaf_field("next")), layout(conformance_leaf()).fixed_len);
        put_leaf(&mut c, next);
    }
}

fn as_u32(v: &Value) -> u32 {
    match v {
        Value::UInt(x) => u32::try_from(*x).expect("u32"),
        other => panic!("expected UInt, got {other:?}"),
    }
}

/// Pointer slots of a `Leaf` fixed part (for `tables_streaming`), checked against the layout.
static LEAF_PTR_SLOTS: [u16; 2] = [5, 9];

fn leaf_ptr_slots() -> &'static [u16] {
    assert_eq!(LEAF_PTR_SLOTS, [slot(&leaf_field("s")), slot(&leaf_field("next"))], "Leaf layout changed");
    &LEAF_PTR_SLOTS
}

// ------------------------------------------------------------ byte helpers

fn rd_i32(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn wr_i32(b: &mut [u8], at: usize, v: i32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// Recompute an envelope's CRC32C trailer over `[0, len - 4)`.
fn fix_crc(b: &mut [u8]) {
    let k = b.len() - envelope::TRAILER_LEN;
    let c = graven::crc32c(&b[..k]);
    b[k..].copy_from_slice(&c.to_le_bytes());
}

/// Absolute position of the root's fixed part (after the `u16 fixed_len` header).
const ROOT_FIXED: usize = 2;

/// Absolute position of a root pointer slot and its target.
fn root_ptr(b: &[u8], name: &str) -> (usize, usize) {
    let at = ROOT_FIXED + slot(&root_field(name)) as usize;
    let rel = rd_i32(b, at);
    assert_ne!(rel, 0, "Root.{name} is null");
    (at, (at as i64 + rel as i64) as usize)
}

// ------------------------------------------------------------------- cases

fn valid(name: &'static str, schema: &'static str, bytes: Vec<u8>, expect: Value) -> Built {
    Built { case: Case { name, schema, enveloped: false, bytes, expect: Expect::Value(expect) }, base: None }
}

fn invalid(name: &'static str, enveloped: bool, base: &[u8], err: &'static str, f: impl FnOnce(&mut Vec<u8>)) -> Built {
    invalid_on(name, ROOT, enveloped, base, err, f)
}

fn invalid_on(
    name: &'static str,
    schema: &'static str,
    enveloped: bool,
    base: &[u8],
    err: &'static str,
    f: impl FnOnce(&mut Vec<u8>),
) -> Built {
    let mut bytes = base.to_vec();
    f(&mut bytes);
    assert_ne!(bytes, base, "{name}: mutation changed nothing");
    Built { case: Case { name, schema, enveloped, bytes, expect: Expect::Error(err) }, base: Some(base.to_vec()) }
}

/// `base` with a mutation applied (for valid cases a value-driven writer cannot produce).
fn patched(base: &[u8], f: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut b = base.to_vec();
    f(&mut b);
    assert_ne!(b, base, "mutation changed nothing");
    b
}

/// Root `fixed_len` header rewritten.
fn set_fixed_len(b: &mut [u8], fixed_len: u16) {
    b[0..2].copy_from_slice(&fixed_len.to_le_bytes());
}

/// Absolute position of a pointer slot of the root of `def` and its target.
fn ptr_of(b: &[u8], def: &TableDef, name: &str) -> (usize, usize) {
    let at = ROOT_FIXED + slot(&field_of(def, name)) as usize;
    let rel = rd_i32(b, at);
    assert_ne!(rel, 0, "{}.{name} is null", def.name);
    (at, (at as i64 + rel as i64) as usize)
}

pub(crate) fn built() -> Vec<Built> {
    let root = conformance_root();
    let root_v2 = conformance_root_v2();
    let mut out = Vec::new();

    // ------------------------------------------------------------- valid
    let minimal = encode(&minimal_value(), root);
    out.push(valid("minimal", ROOT, minimal.clone(), read_view(&minimal_value())));

    let all = all_fields_value();
    out.push(valid("all_fields", ROOT, encode(&all, root), all.clone()));

    out.push(valid("zero_is_present", ROOT, encode(&zero_value(), root), zero_value()));

    let el = empty_lists_value();
    out.push(valid("empty_lists", ROOT, encode(&el, root), read_view(&el)));

    // `leaves` through `tables_streaming`: strings and nested tables are appended to the
    // buffer before the list header, so element pointers are negative (backward).
    let stream_leaves = vec![
        leaf(1, Some("um"), None),
        leaf(2, Some("dois"), None),
        leaf(3, None, Some(leaf(4, Some("quatro"), None))),
    ];
    let bytes = encode_with(|t| {
        put_minimal(t);
        let mut l = t.tables_streaming(slot(&root_field("leaves")), layout(conformance_leaf()).fixed_len, leaf_ptr_slots());
        for v in &stream_leaves {
            let mut e = l.push();
            put_leaf(&mut e, v);
        }
        l.close();
    });
    let v = with(minimal_value(), vec![("leaves", Value::List(stream_leaves.clone()))]);
    out.push(valid("streaming_leaves", ROOT, bytes, read_view(&v)));

    let v = with(minimal_value(), vec![("color", u(9))]);
    out.push(valid("unknown_enum", ROOT, encode(&v, root), read_view(&v)));

    // Tag 5 is not in `Choice`; its payload is a real `Leaf` that the reader never visits.
    let bytes = encode_with(|t| {
        put_minimal(t);
        let mut c = t.union(slot(&root_field("choice")), 5, layout(conformance_leaf()).fixed_len);
        put_leaf(&mut c, &leaf(77, Some("invisível"), None));
    });
    let v = with(minimal_value(), vec![("choice", Value::Union { tag: 5, value: Box::new(tbl(vec![])) })]);
    out.push(valid("unknown_union_tag", ROOT, bytes, read_view(&v)));

    let v1 = with(minimal_value(), vec![("name", s("v1"))]);
    let v2 = with(v1.clone(), vec![("extra", u(0x1122_3344_5566_7788))]);
    out.push(valid("v2_read_by_v1", ROOT, encode(&v2, root_v2), read_view(&v1)));
    out.push(valid("v1_read_by_v2", ROOT_V2, encode(&v1, root), read_view(&v1)));

    // An old writer whose `Root` still had `old @21: u32?`.
    let old_root = TableDef {
        name: root.name,
        fields: root
            .fields
            .iter()
            .cloned()
            .map(|mut f| {
                if f.id == 21 {
                    assert!(matches!(f.presence, Presence::Reserved) && f.name == "old");
                    f.presence = Presence::Optional;
                }
                f
            })
            .collect(),
    };
    let v = with(minimal_value(), vec![("old", u(0xdead_beef))]);
    out.push(valid("reserved_ignored", ROOT, encode(&v, &old_root), read_view(&minimal_value())));

    out.push(Built {
        case: Case {
            name: "enveloped_all_fields",
            schema: ROOT,
            enveloped: true,
            bytes: encode_enveloped(&all, root, schema_id(ROOT)),
            expect: Expect::Value(all.clone()),
        },
        base: None,
    });

    // ----------------------------------------------------------- invalid
    // Root fixed_len shrunk so it ends in the middle of `point`'s 8-byte slot. Every
    // required field (a_u8 slot + bit, flag2 bits) stays inside.
    let point_slot = slot(&root_field("point"));
    out.push(invalid("truncated_slot", false, &minimal, "Truncated", |b| {
        b[0..2].copy_from_slice(&(point_slot + 4).to_le_bytes());
    }));

    // `name` (null in `minimal`) pointing exactly one past the end of the buffer.
    out.push(invalid("bad_offset", false, &minimal, "BadOffset", |b| {
        let at = ROOT_FIXED + slot(&root_field("name")) as usize;
        let rel = (b.len() - at) as i32;
        wr_i32(b, at, rel);
    }));

    // Base: minimal + name "grvn" (the string is the last thing in the buffer).
    let named = encode(&with(minimal_value(), vec![("name", s("grvn"))]), root);
    out.push(invalid("string_len_oob", false, &named, "OutOfBounds", |b| {
        let (_, p) = root_ptr(b, "name");
        assert_eq!(p + 8, b.len());
        b[p..p + 4].copy_from_slice(&5u32.to_le_bytes()); // 4 ⇒ 5: one byte past the end
    }));
    out.push(invalid("bad_utf8", false, &named, "InvalidUtf8", |b| {
        let (_, p) = root_ptr(b, "name");
        b[p + 4] = 0xff; // 'g' ⇒ 0xFF, never valid in UTF-8
    }));

    // a_u8's presence bit cleared (its slot value stays: data behind a clear bit is ignored).
    let a = root_field("a_u8");
    out.push(invalid("missing_required", false, &minimal, "Missing", |b| {
        b[ROOT_FIXED + bit(&a).byte as usize] &= !(1 << bit(&a).shift);
    }));

    // Base: minimal + child {n: 5}; the child's null `next` becomes a pointer to itself.
    let with_child = encode(&with(minimal_value(), vec![("child", leaf(5, None, None))]), root);
    out.push(invalid("self_cycle", false, &with_child, "DepthExceeded", |b| {
        let (_, leaf_pos) = root_ptr(b, "child");
        let at = leaf_pos + 2 + slot(&leaf_field("next")) as usize;
        assert_eq!(rd_i32(b, at), 0);
        wr_i32(b, at, leaf_pos as i32 - at as i32);
    }));

    // Base: 1000 leaves, only leaves[0] has `s` (a 2 KiB string). Mutation: the `s` of
    // every other leaf points at that same string. File ≈ 15 KiB, so the budget is the
    // 1 MiB floor; the charges reach 114 + (6 + 13 000) + 1000 × (4 + 2048) ≈ 2 MiB.
    const N_LEAVES: u32 = 1000;
    let big = "a".repeat(2048);
    let leaves: Vec<Value> =
        (0..N_LEAVES).map(|n| leaf(n, if n == 0 { Some(big.as_str()) } else { None }, None)).collect();
    let amp_base = encode(&with(minimal_value(), vec![("leaves", Value::List(leaves))]), root);
    out.push(invalid("budget_amplification", false, &amp_base, "BudgetExceeded", |b| {
        let (_, list) = root_ptr(b, "leaves");
        let efl = u16::from_le_bytes([b[list + 4], b[list + 5]]) as usize;
        let s_off = slot(&leaf_field("s")) as usize;
        let slot0 = list + 6 + s_off;
        let target = slot0 as i64 + rd_i32(b, slot0) as i64;
        for k in 1..N_LEAVES as usize {
            let at = list + 6 + k * efl + s_off;
            assert_eq!(rd_i32(b, at), 0);
            wr_i32(b, at, (target - at as i64) as i32);
        }
    }));

    // Envelope errors: base is `minimal` in an envelope with the Root schema id.
    let env = encode_enveloped(&minimal_value(), root, schema_id(ROOT));
    let n = env.len();
    // Header mutations recompute the CRC (it covers the header), so the header byte is
    // the only defect.
    out.push(invalid("env_bad_magic", true, &env, "BadMagic", |b| {
        b[0] = b'X';
        fix_crc(b);
    }));
    out.push(invalid("env_bad_version", true, &env, "UnsupportedVersion", |b| {
        b[4] = 2;
        fix_crc(b);
    }));
    out.push(invalid("env_bad_header", true, &env, "BadHeader", |b| {
        b[5] = 1; // flags ≠ 0
        fix_crc(b);
    }));
    out.push(invalid("env_cut_header", true, &env, "Incomplete", |b| b.truncate(10)));
    out.push(invalid("env_cut_payload", true, &env, "Incomplete", |b| b.truncate(envelope::HEADER_LEN + 10)));
    out.push(invalid("env_cut_crc", true, &env, "Incomplete", |b| b.truncate(n - 2)));
    // Payload byte 3 = a_u8's value (42 ⇒ 43).
    out.push(invalid("env_bitflip_payload", true, &env, "ChecksumMismatch", |b| {
        b[envelope::HEADER_LEN + ROOT_FIXED + slot(&a) as usize] ^= 0x01
    }));
    out.push(invalid("env_bitflip_crc", true, &env, "ChecksumMismatch", |b| b[n - 1] ^= 0x80));
    // A well-formed envelope (CRC valid) for another schema id.
    let wrong = encode_enveloped(&minimal_value(), root, schema_id(LEAF));
    out.push(Built {
        case: Case { name: "env_wrong_schema", schema: ROOT, enveloped: true, bytes: wrong, expect: Expect::Error("SchemaMismatch") },
        base: Some(env.clone()),
    });

    // ------------------------------------------- §10.3 (appended; Plano 3)
    out.extend(spec_10_3_cases(&env));

    out
}

/// Vectors for the spec §10.3 rules that had none. Added after the original frozen
/// vectors; none of them changes the earlier ones.
fn spec_10_3_cases(env: &[u8]) -> Vec<Built> {
    let root = conformance_root();
    let extra = conformance_extra();
    let holder = conformance_holder();
    let leaf_fl = layout(conformance_leaf()).fixed_len;
    let mut out = Vec::new();
    let pick1 = || Value::Union { tag: 1, value: Box::new(leaf(1, None, None)) };
    // `pick` (required) = Leaf { n: 1 }.
    let put_pick = |t: &mut TableW<'_>| {
        let mut c = t.union(slot(&field_of(extra, "pick")), 1, leaf_fl);
        put_leaf(&mut c, &leaf(1, None, None));
    };

    // bool in a struct and `[bool]`: any non-zero byte is true; `[enum]` with an unknown value.
    let bytes = encode_def_with(extra, |t| {
        let fl = field_of(extra, "flags");
        t.set::<u16>(slot(&fl), bit(&fl), u16::from_le_bytes([0x02, 0x01])); // on = 02, c = Green
        t.set_scalars::<u8>(slot(&field_of(extra, "bools")), &[0x00, 0x01, 0x07]);
        t.set_scalars::<u8>(slot(&field_of(extra, "colors")), &[0, 2, 9]);
        put_pick(t);
    });
    let v = tbl(vec![
        ("flags", tbl(vec![("on", Value::Bool(true)), ("c", u(1))])),
        ("bools", Value::List(vec![Value::Bool(false), Value::Bool(true), Value::Bool(true)])),
        ("colors", Value::List(vec![u(0), u(2), u(9)])),
        ("pick", pick1()),
    ]);
    out.push(valid("bool_struct_and_lists", EXTRA, bytes, v));

    // `[string]` / `[bytes]` element with pointer 0 ⇒ empty.
    let base = encode_def_with(extra, |t| {
        t.set_strs(slot(&field_of(extra, "names")), ["a", "b", "c"].into_iter());
        t.set_bytes_list(slot(&field_of(extra, "blobs")), [&b"x"[..]].into_iter());
        put_pick(t);
    });
    let bytes = patched(&base, |b| {
        let (_, names) = ptr_of(b, extra, "names");
        wr_i32(b, names + 4 + 4, 0); // element 1
        let (_, blobs) = ptr_of(b, extra, "blobs");
        wr_i32(b, blobs + 4, 0); // element 0
    });
    let v = tbl(vec![
        ("names", Value::List(vec![s("a"), s(""), s("c")])),
        ("blobs", Value::List(vec![by(&[])])),
        ("pick", pick1()),
    ]);
    out.push(valid("strlist_null_elem", EXTRA, bytes, v));

    // Union with tag ≠ 0 and pointer 0: optional ⇒ absent; required ⇒ Missing.
    let base = encode(&with(minimal_value(), vec![("choice", Value::Union { tag: 1, value: Box::new(leaf(3, None, None)) })]), root);
    let bytes = patched(&base, |b| wr_i32(b, ROOT_FIXED + slot(&root_field("choice")) as usize + 2, 0));
    out.push(valid("union_tag_null_ptr_optional", ROOT, bytes, read_view(&minimal_value())));
    let extra_base = encode_def_with(extra, put_pick);
    out.push(invalid_on("union_tag_null_ptr_required", EXTRA, false, &extra_base, "Missing", |b| {
        wr_i32(b, ROOT_FIXED + slot(&field_of(extra, "pick")) as usize + 2, 0)
    }));

    // Unknown union tag whose pointer is invalid: the reader checks the pointer even though
    // it does not visit the payload.
    let base = encode_with(|t| {
        put_minimal(t);
        let mut c = t.union(slot(&root_field("choice")), 5, leaf_fl);
        put_leaf(&mut c, &leaf(77, None, None));
    });
    out.push(invalid("union_unknown_tag_bad_ptr", false, &base, "BadOffset", |b| {
        let at = ROOT_FIXED + slot(&root_field("choice")) as usize + 2;
        let rel = (b.len() - at) as i32;
        wr_i32(b, at, rel);
    }));

    // `fixed_len` cutting a reserved slot in the middle ⇒ no error; `tail` (after it) absent.
    let res = field_of(extra, "_reserved_8");
    let tail = field_of(extra, "tail");
    let with_tail = encode_def_with(extra, |t| {
        put_pick(t);
        t.set_str(slot(&tail), "t");
    });
    let bytes = patched(&with_tail, |b| set_fixed_len(b, slot(&res) + 4));
    out.push(valid("reserved_slot_partial", EXTRA, bytes, tbl(vec![("pick", pick1())])));
    // `fixed_len` cutting a pointer slot in the middle ⇒ Truncated.
    out.push(invalid_on("ptr_slot_partial", EXTRA, false, &with_tail, "Truncated", |b| set_fixed_len(b, slot(&tail) + 2)));

    // Zero-size slot (`Empty`) with `o == fixed_len`: inside, the bit decides.
    let e = field_of(extra, "e");
    let with_e = encode_def_with(extra, |t| {
        put_pick(t);
        t.set::<u8>(slot(&e), bit(&e), 0); // marks the bit; writes nothing (size 0 slot shares `o`)
    });
    let bytes = patched(&with_e, |b| set_fixed_len(b, slot(&e)));
    out.push(valid("zero_size_slot_at_end", EXTRA, bytes, tbl(vec![("e", tbl(vec![])), ("pick", pick1())])));

    // `[Leaf]` with `elem_fixed_len` different from the reader's layout.
    let items = slot(&field_of(holder, "items"));
    let two = [leaf(1, Some("a"), None), leaf(2, None, None)];
    let bytes = encode_def_with(holder, |t| {
        let mut l = t.tables(items, 2, leaf_fl + 8);
        for v in &two {
            let mut el = l.push();
            put_leaf(&mut el, v);
        }
    });
    out.push(valid("table_list_elem_wider", HOLDER, bytes, tbl(vec![("items", Value::List(two.to_vec()))])));
    let n_end = slot(&leaf_field("n")) + 4;
    let narrow = encode_def_with(holder, |t| {
        let mut l = t.tables(items, 2, n_end);
        for k in 1..=2u32 {
            let mut el = l.push();
            let nf = leaf_field("n");
            el.set::<u32>(slot(&nf), bit(&nf), k);
        }
    });
    let v = tbl(vec![("items", Value::List(vec![leaf(1, None, None), leaf(2, None, None)]))]);
    out.push(valid("table_list_elem_narrower", HOLDER, narrow.clone(), v));
    // elem_fixed_len = 0: no element has room for `n` (required) ⇒ Missing.
    out.push(invalid_on("table_list_elem_zero", HOLDER, false, &narrow, "Missing", |b| {
        let (_, list) = ptr_of(b, holder, "items");
        b[list + 4..list + 6].copy_from_slice(&0u16.to_le_bytes());
    }));

    // Envelope concatenation: bytes after the first envelope's CRC are not an error.
    let mut two_env = env.to_vec();
    two_env.extend_from_slice(&encode_enveloped(&all_fields_value(), root, schema_id(ROOT)));
    out.push(Built {
        case: Case {
            name: "env_concat",
            schema: ROOT,
            enveloped: true,
            bytes: two_env,
            expect: Expect::Value(read_view(&minimal_value())),
        },
        base: None,
    });
    // `payload_len` one short, with a CRC valid for the shorter frame: the envelope is
    // well formed, the message (its last byte cut) is not.
    let n = env.len() - envelope::HEADER_LEN - envelope::TRAILER_LEN;
    out.push(invalid("env_payload_len_short", true, env, "OutOfBounds", |b| {
        b[16..20].copy_from_slice(&(n as u32 - 1).to_le_bytes());
        b.truncate(envelope::HEADER_LEN + n - 1 + envelope::TRAILER_LEN);
        fix_crc(b);
    }));
    // `payload_len` corrupted in place (one short, CRC untouched): the CRC is read one byte
    // early, over the wrong range ⇒ ChecksumMismatch, never an accepted shorter payload.
    out.push(invalid("env_payload_len_short_inplace", true, env, "ChecksumMismatch", |b| {
        b[16..20].copy_from_slice(&(n as u32 - 1).to_le_bytes());
    }));
    // `payload_len` past the end of the buffer ⇒ Incomplete.
    out.push(invalid("env_payload_len_long", true, env, "Incomplete", |b| {
        b[16..20].copy_from_slice(&(n as u32 + 1).to_le_bytes());
    }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn names_unique() {
        let cs = cases();
        let names: HashSet<_> = cs.iter().map(|c| c.name).collect();
        assert_eq!(names.len(), cs.len());
    }

    #[test]
    fn every_case_reads_as_expected() {
        for c in cases() {
            let got = evaluate(&c.bytes, c.schema, c.enveloped);
            match (&c.expect, got) {
                (Expect::Value(want), Ok(v)) => assert_eq!(&v, want, "{}", c.name),
                (Expect::Error(want), Err(e)) => assert_eq!(&error_name(&e), want, "{}: {e:?}", c.name),
                (want, got) => panic!("{}: expected {want:?}, got {got:?}", c.name),
            }
        }
    }

    /// Exactly one defect per error case: the unmutated base is valid.
    #[test]
    fn error_bases_read_ok() {
        for b in built() {
            match (&b.case.expect, &b.base) {
                (Expect::Error(_), Some(base)) => {
                    let r = evaluate(base, b.case.schema, b.case.enveloped);
                    assert!(r.is_ok(), "{}: base does not read: {r:?}", b.case.name);
                }
                (Expect::Error(_), None) => panic!("{}: error case without a base", b.case.name),
                (Expect::Value(_), _) => assert!(b.base.is_none()),
            }
        }
    }

    /// Same-length byte mutations touch one field only: a window of at most 4 bytes
    /// (except `budget_amplification`, which repeats the same pointer patch, and
    /// `env_wrong_schema`, whose schema id and CRC differ).
    #[test]
    fn mutations_are_local() {
        for b in built() {
            let (Some(base), name) = (&b.base, b.case.name) else { continue };
            if base.len() != b.case.bytes.len() || name == "budget_amplification" || name == "env_wrong_schema" {
                continue;
            }
            let n = base.len();
            // Enveloped header mutations also rewrite the 4-byte CRC trailer; ignore it.
            let end = if b.case.enveloped && matches!(name, "env_bad_magic" | "env_bad_version" | "env_bad_header") {
                n - envelope::TRAILER_LEN
            } else {
                n
            };
            let diff: Vec<_> = (0..end).filter(|&k| base[k] != b.case.bytes[k]).collect();
            assert!(!diff.is_empty() && diff.last().unwrap() - diff[0] < 4, "{name}: diff at {diff:?}");
        }
    }

    /// Enveloped error cases whose defect is not the CRC or a cut carry a valid CRC.
    #[test]
    fn envelope_crc_valid_unless_checksum_or_cut() {
        let mut checked = 0;
        for c in cases() {
            let Expect::Error(e) = c.expect else { continue };
            if !c.enveloped || e == "ChecksumMismatch" || e == "Incomplete" {
                continue;
            }
            let k = c.bytes.len() - envelope::TRAILER_LEN;
            let stored = u32::from_le_bytes(c.bytes[k..].try_into().unwrap());
            assert_eq!(stored, graven::crc32c(&c.bytes[..k]), "{}: stale CRC", c.name);
            checked += 1;
        }
        assert_eq!(checked, 5); // bad_magic, bad_version, bad_header, wrong_schema, payload_len_short
    }

    #[test]
    fn budget_amplification_is_small_but_expensive() {
        let c = cases().into_iter().find(|c| c.name == "budget_amplification").unwrap();
        assert!(c.bytes.len() * 8 < 1 << 20, "budget must be the 1 MiB floor");
        assert!(c.bytes.len() < 32 * 1024);
    }

    #[test]
    fn streaming_leaves_has_backward_pointers() {
        let c = cases().into_iter().find(|c| c.name == "streaming_leaves").unwrap();
        let (_, list) = root_ptr(&c.bytes, "leaves");
        let s0 = list + 6 + slot(&leaf_field("s")) as usize;
        assert!(rd_i32(&c.bytes, s0) < 0, "streaming element pointers point backward");
    }

    #[test]
    fn deterministic() {
        let (a, b) = (cases(), cases());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.bytes, y.bytes, "{}", x.name);
        }
    }
}
