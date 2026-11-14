//! Schema-driven reader/writer: [`read`], [`write`](fn@crate::write), [`layout`] and the dynamic [`Value`].

mod read;
mod write;

pub use read::read;
pub use write::write;

use crate::model::{TableDef, Ty};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use graven::Bit;
use graven_layout::{FieldKind, FieldLayout, TableLayout, compute_layout};

/// A decoded message. See the crate docs for the JSON representation.
///
/// Equality is exact: floats compare by bit pattern (`NaN == NaN` with the same payload,
/// `0.0 != -0.0`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    Bool(bool),
    /// Unsigned integers and enums.
    UInt(#[serde(with = "dec_u128")] u128),
    /// Signed integers.
    Int(#[serde(with = "dec_i128")] i128),
    F32(#[serde(with = "bits_f32")] f32),
    F64(#[serde(with = "bits_f64")] f64),
    Str(String),
    Bytes(#[serde(with = "hex_bytes")] Vec<u8>),
    List(Vec<Value>),
    /// Table or struct, keyed by field name.
    Table(BTreeMap<String, Value>),
    Union { tag: u16, value: Box<Value> },
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use Value::*;
        match (self, other) {
            (Bool(a), Bool(b)) => a == b,
            (UInt(a), UInt(b)) => a == b,
            (Int(a), Int(b)) => a == b,
            (F32(a), F32(b)) => a.to_bits() == b.to_bits(),
            (F64(a), F64(b)) => a.to_bits() == b.to_bits(),
            (Str(a), Str(b)) => a == b,
            (Bytes(a), Bytes(b)) => a == b,
            (List(a), List(b)) => a == b,
            (Table(a), Table(b)) => a == b,
            (Union { tag: t1, value: v1 }, Union { tag: t2, value: v2 }) => t1 == t2 && v1 == v2,
            _ => false,
        }
    }
}

mod dec_u128 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        String::deserialize(d)?.parse().map_err(D::Error::custom)
    }
}

mod dec_i128 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(v: &i128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i128, D::Error> {
        String::deserialize(d)?.parse().map_err(D::Error::custom)
    }
}

fn parse_bits<E: serde::de::Error>(s: &str, prefix: &str) -> Result<u64, E> {
    let hex = s
        .strip_prefix(prefix)
        .and_then(|r| r.strip_prefix("0x"))
        .ok_or_else(|| E::custom(format!("expected `{prefix}0x<hex>`, got `{s}`")))?;
    u64::from_str_radix(hex, 16).map_err(E::custom)
}

mod bits_f32 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("f32:0x{:08x}", v.to_bits()))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        let bits = super::parse_bits::<D::Error>(&String::deserialize(d)?, "f32:")?;
        u32::try_from(bits).map(f32::from_bits).map_err(D::Error::custom)
    }
}

mod bits_f64 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("f64:0x{:016x}", v.to_bits()))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        super::parse_bits::<D::Error>(&String::deserialize(d)?, "f64:").map(f64::from_bits)
    }
}

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        let mut out = String::with_capacity(v.len() * 2);
        for b in v {
            out.push_str(&format!("{b:02x}"));
        }
        s.serialize_str(&out)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() % 2 != 0 || !s.is_ascii() {
            return Err(D::Error::custom("hex string of odd length or non-ascii"));
        }
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(D::Error::custom)).collect()
    }
}

// ------------------------------------------------------------------ layout

/// Byte size of a fixed-size value (scalars, enums, structs; `Bool` = 1 byte inside
/// structs and lists). `None` for pointer-like types.
#[doc(hidden)]
pub fn fixed_size(ty: &Ty) -> Option<u16> {
    Some(match ty {
        Ty::U8 | Ty::I8 | Ty::Bool => 1,
        Ty::U16 | Ty::I16 => 2,
        Ty::U32 | Ty::I32 | Ty::F32 => 4,
        Ty::U64 | Ty::I64 | Ty::F64 => 8,
        Ty::U128 | Ty::I128 => 16,
        Ty::Enum { base } => fixed_size(base)?,
        Ty::Struct(s) => s.size,
        Ty::Str | Ty::Bytes | Ty::List(_) | Ty::Table(_) | Ty::Union(_) => return None,
    })
}

/// Wire shape of a table field of type `ty`.
pub fn field_kind(ty: &Ty) -> FieldKind {
    match ty {
        Ty::Bool => FieldKind::Bool,
        Ty::Union(_) => FieldKind::Union,
        Ty::Str | Ty::Bytes | Ty::List(_) | Ty::Table(_) => FieldKind::Ptr,
        _ => FieldKind::Scalar { size: fixed_size(ty).expect("fixed-size type") },
    }
}

/// Fixed-part layout of `t` (reserved fields keep their original kind).
pub fn layout(t: &TableDef) -> TableLayout {
    let kinds: Vec<FieldKind> = t.fields.iter().map(|f| field_kind(&f.ty)).collect();
    compute_layout(&kinds)
}

#[doc(hidden)]
pub fn bit(fl: &FieldLayout) -> Bit {
    let (byte, shift) = fl.presence.expect("field has a presence bit");
    Bit { byte, shift }
}

#[doc(hidden)]
pub fn slot(fl: &FieldLayout) -> u16 {
    fl.slot.expect("field has a slot")
}
