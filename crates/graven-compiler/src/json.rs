//! Plain JSON for Graven messages, driven by the schema (`gravenc to-json` / `from-json`).
//!
//! | schema type | JSON |
//! |---|---|
//! | integers | number; a string when outside the 64-bit range JSON tools handle (`u128`, `i128`) |
//! | `f32`, `f64` | number; `"NaN"`, `"inf"`, `"-inf"` for non-finite values |
//! | `bool` | `true` / `false` |
//! | enum | the variant name; a number for a value the schema does not know |
//! | `string` | string |
//! | `bytes` | lowercase hex string |
//! | struct, table | object with the fields in declaration / ID order |
//! | list | array |
//! | union | `{"Variant": {…}}`; `{"$tag": N}` for a tag the schema does not know |
//!
//! Absent optional fields are left out; an absent field with a default shows the default.
//! Reserved fields never appear. On input, `null` is the same as leaving the field out.

use crate::ir::{Ir, Prim, TableId, Ty};
use graven_dynamic::Value;
use serde_json::{Map, Number, Value as J};
use std::collections::BTreeMap;

/// A table value as JSON.
pub fn table_to_json(ir: &Ir, t: TableId, v: &Value) -> J {
    to_json(ir, &Ty::Table(t), v)
}

/// JSON (in the format above) to a table value. Errors name the path, e.g. `Entry.memo`.
pub fn table_from_json(ir: &Ir, t: TableId, j: &J) -> Result<Value, String> {
    from_json(ir, &Ty::Table(t), j, &ir.tables[t.0].name)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn float_json(x: f64) -> J {
    if x.is_nan() {
        J::String("NaN".into())
    } else if x.is_infinite() {
        J::String(if x > 0.0 { "inf" } else { "-inf" }.into())
    } else {
        Number::from_f64(x).map(J::Number).unwrap_or(J::Null)
    }
}

fn to_json(ir: &Ir, ty: &Ty, v: &Value) -> J {
    match (ty, v) {
        (_, Value::Bool(b)) => J::Bool(*b),
        (Ty::Enum(id), Value::UInt(raw)) => {
            let e = &ir.enums[id.0];
            match e.values.iter().find(|(_, x)| *x as u128 == *raw) {
                Some((name, _)) => J::String(name.clone()),
                None => J::Number((*raw as u64).into()),
            }
        }
        (_, Value::UInt(u)) => match u64::try_from(*u) {
            Ok(x) => J::Number(x.into()),
            Err(_) => J::String(u.to_string()),
        },
        (_, Value::Int(i)) => match i64::try_from(*i) {
            Ok(x) => J::Number(x.into()),
            Err(_) => J::String(i.to_string()),
        },
        (_, Value::F32(x)) => float_json(*x as f64),
        (_, Value::F64(x)) => float_json(*x),
        (_, Value::Str(s)) => J::String(s.clone()),
        (_, Value::Bytes(b)) => J::String(hex(b)),
        (Ty::List(e), Value::List(items)) => J::Array(items.iter().map(|x| to_json(ir, e, x)).collect()),
        (Ty::Struct(id), Value::Table(m)) => {
            let mut out = Map::new();
            for (name, ft) in &ir.structs[id.0].fields {
                if let Some(x) = m.get(name) {
                    out.insert(name.clone(), to_json(ir, ft, x));
                }
            }
            J::Object(out)
        }
        (Ty::Table(id), Value::Table(m)) => {
            let mut out = Map::new();
            for f in &ir.tables[id.0].fields {
                if let Some(x) = m.get(&f.name) {
                    out.insert(f.name.clone(), to_json(ir, &f.ty, x));
                }
            }
            J::Object(out)
        }
        (Ty::Union(id), Value::Union { tag, value }) => {
            let u = &ir.unions[id.0];
            let mut out = Map::new();
            match u.variants.iter().find(|(_, t, _)| t == tag) {
                Some((name, _, tid)) => {
                    out.insert(name.clone(), to_json(ir, &Ty::Table(*tid), value));
                }
                None => {
                    out.insert("$tag".into(), J::Number((*tag).into()));
                }
            }
            J::Object(out)
        }
        (ty, v) => J::String(format!("<{} does not match {v:?}>", ir.ty_name(ty))),
    }
}

fn err<T>(path: &str, msg: impl std::fmt::Display) -> Result<T, String> {
    Err(format!("{path}: {msg}"))
}

fn int_of(j: &J, path: &str) -> Result<i128, String> {
    match j {
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i as i128)
            } else if let Some(u) = n.as_u64() {
                Ok(u as i128)
            } else {
                err(path, format!("expected an integer, got {n}"))
            }
        }
        J::String(s) => s.parse::<i128>().or_else(|_| err(path, format!("expected an integer, got \"{s}\""))),
        other => err(path, format!("expected an integer, got {other}")),
    }
}

fn float_of(j: &J, path: &str) -> Result<f64, String> {
    match j {
        J::Number(n) => n.as_f64().ok_or_else(|| format!("{path}: not a number")),
        J::String(s) => match s.as_str() {
            "NaN" => Ok(f64::NAN),
            "inf" => Ok(f64::INFINITY),
            "-inf" => Ok(f64::NEG_INFINITY),
            _ => s.parse::<f64>().or_else(|_| err(path, format!("expected a number, got \"{s}\""))),
        },
        other => err(path, format!("expected a number, got {other}")),
    }
}

fn prim_value(p: Prim, j: &J, path: &str) -> Result<Value, String> {
    if p.is_float() {
        let x = float_of(j, path)?;
        return Ok(if p == Prim::F32 { Value::F32(x as f32) } else { Value::F64(x) });
    }
    let (lo, hi) = p.int_range().expect("integer type");
    // u128 above i128::MAX only fits as a decimal string.
    if p == Prim::U128
        && let J::String(s) = j
        && let Ok(u) = s.parse::<u128>()
    {
        return Ok(Value::UInt(u));
    }
    let i = int_of(j, path)?;
    if i < lo || i > hi {
        return err(path, format!("{i} does not fit in {}", p.name()));
    }
    Ok(if lo == 0 { Value::UInt(i as u128) } else { Value::Int(i) })
}

fn object<'j>(j: &'j J, path: &str) -> Result<&'j Map<String, J>, String> {
    j.as_object().ok_or_else(|| format!("{path}: expected an object, got {j}"))
}

fn from_json(ir: &Ir, ty: &Ty, j: &J, path: &str) -> Result<Value, String> {
    match ty {
        Ty::Scalar(p) => prim_value(*p, j, path),
        Ty::Bool => j.as_bool().map(Value::Bool).ok_or_else(|| format!("{path}: expected true or false, got {j}")),
        Ty::Enum(id) => {
            let e = &ir.enums[id.0];
            match j {
                J::String(s) => match e.values.iter().find(|(n, _)| n == s) {
                    Some((_, raw)) => Ok(Value::UInt(*raw as u128)),
                    None => err(path, format!("`{s}` is not a variant of {}", e.name)),
                },
                _ => {
                    let raw = int_of(j, path)?;
                    let max = match e.base.size() {
                        1 => u8::MAX as i128,
                        2 => u16::MAX as i128,
                        _ => u32::MAX as i128,
                    };
                    if !(0..=max).contains(&raw) {
                        return err(path, format!("{raw} does not fit in {}", e.base.name()));
                    }
                    Ok(Value::UInt(raw as u128))
                }
            }
        }
        Ty::Str => j.as_str().map(|s| Value::Str(s.to_string())).ok_or_else(|| format!("{path}: expected a string, got {j}")),
        Ty::Bytes => {
            let s = j.as_str().ok_or_else(|| format!("{path}: expected a hex string, got {j}"))?;
            if s.len() % 2 != 0 || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
                return err(path, "expected an even-length hex string");
            }
            Ok(Value::Bytes((0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()))
        }
        Ty::List(e) => {
            let items = j.as_array().ok_or_else(|| format!("{path}: expected an array, got {j}"))?;
            items.iter().enumerate().map(|(i, x)| from_json(ir, e, x, &format!("{path}[{i}]"))).collect::<Result<_, _>>().map(Value::List)
        }
        Ty::Struct(id) => {
            let s = &ir.structs[id.0];
            let o = object(j, path)?;
            if let Some(k) = o.keys().find(|k| !s.fields.iter().any(|(n, _)| n == *k)) {
                return err(path, format!("struct {} has no field `{k}`", s.name));
            }
            let mut m = BTreeMap::new();
            for (n, ft) in &s.fields {
                let x = o.get(n).ok_or_else(|| format!("{path}: struct {} needs every field; `{n}` is missing", s.name))?;
                m.insert(n.clone(), from_json(ir, ft, x, &format!("{path}.{n}"))?);
            }
            Ok(Value::Table(m))
        }
        Ty::Table(id) => {
            let t = &ir.tables[id.0];
            let o = object(j, path)?;
            let mut m = BTreeMap::new();
            for (k, x) in o {
                let Some(f) = t.fields.iter().find(|f| &f.name == k && !matches!(f.presence, crate::ir::Presence::Reserved { .. })) else {
                    return err(path, format!("table {} has no field `{k}`", t.name));
                };
                // JSON `null` means "not sent": the field is left absent (a required field
                // is then reported as missing below).
                if x.is_null() {
                    continue;
                }
                m.insert(k.clone(), from_json(ir, &f.ty, x, &format!("{path}.{k}"))?);
            }
            if let Some(f) = t.fields.iter().find(|f| matches!(f.presence, crate::ir::Presence::Required) && !m.contains_key(&f.name)) {
                return err(path, format!("required field `{}` is missing", f.name));
            }
            Ok(Value::Table(m))
        }
        Ty::Union(id) => {
            let u = &ir.unions[id.0];
            let o = object(j, path)?;
            if o.len() != 1 {
                return err(path, format!("a union is an object with exactly one key (the variant), got {} keys", o.len()));
            }
            let (k, x) = o.iter().next().expect("one key");
            if k == "$tag" {
                return err(path, "cannot write an unknown union tag");
            }
            let Some((_, tag, tid)) = u.variants.iter().find(|(n, _, _)| n == k) else {
                return err(path, format!("`{k}` is not a variant of {}", u.name));
            };
            Ok(Value::Union { tag: *tag, value: Box::new(from_json(ir, &Ty::Table(*tid), x, &format!("{path}.{k}"))?) })
        }
    }
}
