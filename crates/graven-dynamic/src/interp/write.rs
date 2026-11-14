//! Schema-driven writer built only on the public `TableW` API.

use super::{Value, bit, fixed_size, layout, slot};
use crate::model::{Presence, StructDef, TableDef, Ty};
use std::collections::BTreeMap;
use graven::{Builder, Error, Le, TableW};
use graven_layout::FieldLayout;

/// Write `v` (a `Value::Table`) as a message with root `root` into `b`.
///
/// Fields missing from the map are not written (absent on the wire); fields are written
/// in ID order, so variable data is laid out in ID order too.
///
/// # Panics
/// When `v` does not match the schema: unknown or reserved field names, wrong value
/// kinds, integers out of range, union tag 0, or an unknown union tag whose value is not
/// an empty table. These are bugs in the caller, like `TableW`'s own panics.
pub fn write(v: &Value, root: &TableDef, b: &mut Builder) -> Result<(), Error> {
    let m = as_table(v, root.name);
    let lay = layout(root);
    let mut t = b.root(lay.fixed_len);
    write_fields(&mut t, root, m);
    Ok(())
}

fn as_table<'v>(v: &'v Value, what: &str) -> &'v BTreeMap<String, Value> {
    match v {
        Value::Table(m) => m,
        other => panic!("{what}: expected Value::Table, got {other:?}"),
    }
}

fn write_fields(t: &mut TableW<'_>, def: &TableDef, m: &BTreeMap<String, Value>) {
    for k in m.keys() {
        match def.field(k) {
            None => panic!("{}: unknown field `{k}`", def.name),
            Some(f) if matches!(f.presence, Presence::Reserved) => {
                panic!("{}: field `{k}` is reserved", def.name)
            }
            Some(_) => {}
        }
    }
    let lay = layout(def);
    for (f, fl) in def.fields.iter().zip(&lay.fields) {
        if let Some(v) = m.get(f.name) {
            write_field(t, &f.ty, fl, v, f.name);
        }
    }
}

/// Fixed-size value as `fixed_size(ty)` little-endian bytes, appended to `out`.
pub(crate) fn encode_fixed(ty: &Ty, v: &Value, what: &str, out: &mut Vec<u8>) {
    fn uint(v: &Value, max: u128, what: &str) -> u128 {
        match *v {
            Value::UInt(x) if x <= max => x,
            ref other => panic!("{what}: expected UInt <= {max}, got {other:?}"),
        }
    }
    fn int(v: &Value, min: i128, max: i128, what: &str) -> i128 {
        match *v {
            Value::Int(x) if (min..=max).contains(&x) => x,
            ref other => panic!("{what}: expected Int in {min}..={max}, got {other:?}"),
        }
    }
    match ty {
        Ty::U8 => out.push(uint(v, u8::MAX as u128, what) as u8),
        Ty::U16 => out.extend_from_slice(&(uint(v, u16::MAX as u128, what) as u16).to_le_bytes()),
        Ty::U32 => out.extend_from_slice(&(uint(v, u32::MAX as u128, what) as u32).to_le_bytes()),
        Ty::U64 => out.extend_from_slice(&(uint(v, u64::MAX as u128, what) as u64).to_le_bytes()),
        Ty::U128 => out.extend_from_slice(&uint(v, u128::MAX, what).to_le_bytes()),
        Ty::I8 => out.push(int(v, i8::MIN as i128, i8::MAX as i128, what) as i8 as u8),
        Ty::I16 => out.extend_from_slice(&(int(v, i16::MIN as i128, i16::MAX as i128, what) as i16).to_le_bytes()),
        Ty::I32 => out.extend_from_slice(&(int(v, i32::MIN as i128, i32::MAX as i128, what) as i32).to_le_bytes()),
        Ty::I64 => out.extend_from_slice(&(int(v, i64::MIN as i128, i64::MAX as i128, what) as i64).to_le_bytes()),
        Ty::I128 => out.extend_from_slice(&int(v, i128::MIN, i128::MAX, what).to_le_bytes()),
        Ty::F32 => match v {
            Value::F32(x) => out.extend_from_slice(&x.to_le_bytes()),
            other => panic!("{what}: expected F32, got {other:?}"),
        },
        Ty::F64 => match v {
            Value::F64(x) => out.extend_from_slice(&x.to_le_bytes()),
            other => panic!("{what}: expected F64, got {other:?}"),
        },
        Ty::Bool => match v {
            Value::Bool(x) => out.push(*x as u8),
            other => panic!("{what}: expected Bool, got {other:?}"),
        },
        Ty::Enum { base } => encode_fixed(base, v, what, out),
        Ty::Struct(s) => encode_struct(s, v, out),
        Ty::Str | Ty::Bytes | Ty::List(_) | Ty::Table(_) | Ty::Union(_) => {
            unreachable!("encode_fixed on a pointer type")
        }
    }
}

fn encode_struct(s: &StructDef, v: &Value, out: &mut Vec<u8>) {
    let m = as_table(v, s.name);
    for k in m.keys() {
        assert!(s.fields.iter().any(|(n, _)| n == k), "{}: unknown field `{k}`", s.name);
    }
    let start = out.len();
    for (name, ty) in &s.fields {
        let fv = m.get(*name).unwrap_or_else(|| panic!("{}: struct field `{name}` missing", s.name));
        encode_fixed(ty, fv, name, out);
    }
    assert_eq!(out.len() - start, s.size as usize, "StructDef `{}`: size does not match its fields", s.name);
}

/// `N` raw bytes as an `Le` element, so `[struct]` lists and inline slots of any size go
/// through the public `TableW::set` / `set_scalars` (generated code does the same with
/// its struct types, which implement `Le`).
#[derive(Clone, Copy)]
struct Raw<const N: usize>([u8; N]);

impl<const N: usize> Default for Raw<N> {
    fn default() -> Self {
        Raw([0; N])
    }
}

impl<const N: usize> Le for Raw<N> {
    const SIZE: usize = N;
    fn read_le(b: &[u8]) -> Self {
        Raw(b.try_into().expect("Raw: slice of SIZE bytes"))
    }
    fn write_le(self, out: &mut [u8]) {
        out.copy_from_slice(&self.0);
    }
}

fn set_raw<const N: usize>(t: &mut TableW<'_>, off: u16, presence: graven::Bit, b: &[u8]) {
    t.set::<Raw<N>>(off, presence, Raw::read_le(b));
}

fn set_raw_list<const N: usize>(t: &mut TableW<'_>, off: u16, b: &[u8]) {
    let elems: Vec<Raw<N>> = if N == 0 { Vec::new() } else { b.chunks_exact(N).map(Raw::read_le).collect() };
    t.set_scalars(off, &elems);
}

/// `[u32 count][count × N bytes]` for a zero-size element still needs `count` elements.
fn set_raw_list_zero(t: &mut TableW<'_>, off: u16, count: usize) {
    t.set_scalars(off, &vec![Raw::<0>([]); count]);
}

macro_rules! by_size {
    ($size:expr, $f:ident $args:tt; $($n:literal)*) => {
        match $size {
            $($n => $f::<$n> $args,)*
            other => panic!("fixed-size values larger than 64 bytes are not supported (got {other})"),
        }
    };
}

fn write_field(t: &mut TableW<'_>, ty: &Ty, fl: &FieldLayout, v: &Value, what: &str) {
    match ty {
        Ty::Bool => match v {
            Value::Bool(x) => t.set_bool(bit(fl), *x),
            other => panic!("{what}: expected Bool, got {other:?}"),
        },
        Ty::Str => match v {
            Value::Str(s) => t.set_str(slot(fl), s),
            other => panic!("{what}: expected Str, got {other:?}"),
        },
        Ty::Bytes => match v {
            Value::Bytes(b) => t.set_bytes(slot(fl), b),
            other => panic!("{what}: expected Bytes, got {other:?}"),
        },
        Ty::List(elem) => match v {
            Value::List(items) => write_list(t, slot(fl), elem, items, what),
            other => panic!("{what}: expected List, got {other:?}"),
        },
        Ty::Table(def) => {
            let m = as_table(v, what);
            let mut c = t.table(slot(fl), layout(def).fixed_len);
            write_fields(&mut c, def, m);
        }
        Ty::Union(u) => match v {
            Value::Union { tag, value } => {
                assert_ne!(*tag, 0, "{what}: union tag 0 means absent; omit the field instead");
                let m = as_table(value, what);
                match u.variant(*tag) {
                    Some(def) => {
                        let mut c = t.union(slot(fl), *tag, layout(def).fixed_len);
                        write_fields(&mut c, def, m);
                    }
                    None => {
                        assert!(m.is_empty(), "{what}: unknown union tag {tag} needs an empty table");
                        t.union(slot(fl), *tag, 0);
                    }
                }
            }
            other => panic!("{what}: expected Union, got {other:?}"),
        },
        _ => {
            let mut b = Vec::new();
            encode_fixed(ty, v, what, &mut b);
            let (off, presence) = (slot(fl), bit(fl));
            by_size!(b.len(), set_raw (t, off, presence, &b);
                0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32
                33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64)
        }
    }
}

fn write_list(t: &mut TableW<'_>, off: u16, elem: &Ty, items: &[Value], what: &str) {
    match elem {
        Ty::Str => {
            let strs: Vec<&str> = items
                .iter()
                .map(|v| match v {
                    Value::Str(s) => s.as_str(),
                    other => panic!("{what}: expected Str element, got {other:?}"),
                })
                .collect();
            t.set_strs(off, strs.into_iter());
        }
        Ty::Bytes => {
            let blobs: Vec<&[u8]> = items
                .iter()
                .map(|v| match v {
                    Value::Bytes(b) => b.as_slice(),
                    other => panic!("{what}: expected Bytes element, got {other:?}"),
                })
                .collect();
            t.set_bytes_list(off, blobs.into_iter());
        }
        Ty::Table(def) => {
            let efl = layout(def).fixed_len;
            let mut l = t.tables(off, items.len() as u32, efl);
            for v in items {
                let m = as_table(v, what);
                let mut e = l.push();
                write_fields(&mut e, def, m);
            }
        }
        Ty::List(_) | Ty::Union(_) => panic!("{what}: unsupported list element type {elem:?}"),
        _ => {
            let size = fixed_size(elem).expect("fixed-size type") as usize;
            let mut b = Vec::with_capacity(items.len() * size);
            for v in items {
                encode_fixed(elem, v, what, &mut b);
            }
            if size == 0 {
                set_raw_list_zero(t, off, items.len());
            } else {
                by_size!(size, set_raw_list (t, off, &b);
                    1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32
                    33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64)
            }
        }
    }
}
