//! Full read: mirrors a generated `Root::read(buf)` followed by `verify()`.

use super::{Value, bit, fixed_size, layout, slot};
use crate::model::{Presence, StructDef, TableDef, Ty};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use graven::{Error, TableRef, VerifyCtx};
use graven_layout::{FieldKind, TableLayout};

/// Decode the whole message rooted at `buf[0]` with schema `root`.
///
/// Per table, in this order: required-field check (`docs/spec/graven-format-v1.md` §4.3; partially covered optional
/// scalar slots ⇒ `Truncated`), `VerifyCtx::enter`, `charge(2 + fixed_len)` (list
/// elements are charged by their list instead), then every non-reserved field in ID
/// order. See the crate docs for the full charging rules.
pub fn read(buf: &[u8], root: &TableDef) -> Result<Value, Error> {
    let t = TableRef::at(buf, 0)?;
    let mut r = Reader { buf, ctx: VerifyCtx::new(buf.len()), layouts: HashMap::new() };
    r.table(t, root, true)
}

struct Reader<'a> {
    buf: &'a [u8],
    ctx: VerifyCtx,
    layouts: HashMap<*const TableDef, Rc<TableLayout>>,
}

fn missing(name: &'static str) -> Error {
    Error::Missing { field: name }
}

/// Required-field check of the generated `read()` (`check_required` in graven-schemas).
fn check_required(t: &TableRef<'_>, def: &TableDef, lay: &TableLayout) -> Result<(), Error> {
    for (f, fl) in def.fields.iter().zip(&lay.fields) {
        let req = match f.presence {
            Presence::Reserved => continue,
            Presence::Required => true,
            Presence::Optional | Presence::Default(_) => false,
        };
        match super::field_kind(&f.ty) {
            FieldKind::Scalar { size } => {
                let inside = t.slot_state(slot(fl), size)?;
                if req && (!inside || !t.bit(bit(fl))) {
                    return Err(missing(f.name));
                }
            }
            FieldKind::Bool => {
                if req && !t.bit(bit(fl)) {
                    return Err(missing(f.name));
                }
            }
            FieldKind::Ptr => {
                if req && (!t.slot_state(slot(fl), 4)? || t.raw::<i32>(slot(fl)) == 0) {
                    return Err(missing(f.name));
                }
            }
            FieldKind::Union => {
                if req && (!t.slot_state(slot(fl), 6)? || t.raw::<u16>(slot(fl)) == 0) {
                    return Err(missing(f.name));
                }
            }
        }
    }
    Ok(())
}

/// Little-endian bytes of a fixed-size value ⇒ `Value`. `b.len() == fixed_size(ty)`.
pub(crate) fn decode_fixed(ty: &Ty, b: &[u8]) -> Value {
    fn arr<const N: usize>(b: &[u8]) -> [u8; N] {
        b.try_into().expect("decode_fixed: size checked by caller")
    }
    match ty {
        Ty::U8 => Value::UInt(b[0] as u128),
        Ty::U16 => Value::UInt(u16::from_le_bytes(arr(b)) as u128),
        Ty::U32 => Value::UInt(u32::from_le_bytes(arr(b)) as u128),
        Ty::U64 => Value::UInt(u64::from_le_bytes(arr(b)) as u128),
        Ty::U128 => Value::UInt(u128::from_le_bytes(arr(b))),
        Ty::I8 => Value::Int(b[0] as i8 as i128),
        Ty::I16 => Value::Int(i16::from_le_bytes(arr(b)) as i128),
        Ty::I32 => Value::Int(i32::from_le_bytes(arr(b)) as i128),
        Ty::I64 => Value::Int(i64::from_le_bytes(arr(b)) as i128),
        Ty::I128 => Value::Int(i128::from_le_bytes(arr(b))),
        Ty::F32 => Value::F32(f32::from_le_bytes(arr(b))),
        Ty::F64 => Value::F64(f64::from_le_bytes(arr(b))),
        Ty::Bool => Value::Bool(b[0] != 0),
        Ty::Enum { base } => decode_fixed(base, b),
        Ty::Struct(s) => decode_struct(s, b),
        Ty::Str | Ty::Bytes | Ty::List(_) | Ty::Table(_) | Ty::Union(_) => {
            unreachable!("decode_fixed on a pointer type")
        }
    }
}

fn decode_struct(s: &StructDef, b: &[u8]) -> Value {
    let mut m = BTreeMap::new();
    let mut at = 0usize;
    for (name, ty) in &s.fields {
        let n = fixed_size(ty).expect("struct fields are fixed-size") as usize;
        m.insert(name.to_string(), decode_fixed(ty, &b[at..at + n]));
        at += n;
    }
    assert_eq!(at, s.size as usize, "StructDef `{}`: size does not match its fields", s.name);
    Value::Table(m)
}

impl<'a> Reader<'a> {
    fn layout(&mut self, def: &TableDef) -> Rc<TableLayout> {
        self.layouts.entry(def as *const _).or_insert_with(|| Rc::new(layout(def))).clone()
    }

    /// One table: required check, enter, charge header (unless a list element), fields.
    fn table(&mut self, t: TableRef<'a>, def: &TableDef, charge_header: bool) -> Result<Value, Error> {
        let lay = self.layout(def);
        check_required(&t, def, &lay)?;
        self.ctx.enter()?;
        if charge_header {
            self.ctx.charge(2 + t.fixed_len() as usize)?;
        }
        let mut m = BTreeMap::new();
        for (f, fl) in def.fields.iter().zip(&lay.fields) {
            if matches!(f.presence, Presence::Reserved) {
                continue;
            }
            match (self.field(&t, &f.ty, fl)?, &f.presence) {
                (Some(v), _) => {
                    m.insert(f.name.to_string(), v);
                }
                (None, Presence::Default(d)) => {
                    m.insert(f.name.to_string(), d.clone());
                }
                // e.g. a union with a non-zero tag but a null pointer.
                (None, Presence::Required) => return Err(missing(f.name)),
                (None, _) => {}
            }
        }
        self.ctx.leave();
        Ok(Value::Table(m))
    }

    fn field(&mut self, t: &TableRef<'a>, ty: &Ty, fl: &graven_layout::FieldLayout) -> Result<Option<Value>, Error> {
        Ok(match ty {
            Ty::Bool => t.bool(bit(fl)).map(Value::Bool),
            Ty::Str => match t.str(slot(fl))? {
                Some(s) => {
                    self.ctx.charge(4 + s.len())?;
                    Some(Value::Str(s.to_string()))
                }
                None => None,
            },
            Ty::Bytes => match t.bytes(slot(fl))? {
                Some(b) => {
                    self.ctx.charge(4 + b.len())?;
                    Some(Value::Bytes(b.to_vec()))
                }
                None => None,
            },
            Ty::List(elem) => self.list(t, slot(fl), elem)?,
            Ty::Table(def) => match t.table(slot(fl))? {
                Some(c) => Some(self.table(c, def, true)?),
                None => None,
            },
            Ty::Union(u) => match t.union(slot(fl))? {
                Some((tag, c)) => {
                    let value = match u.variant(tag) {
                        Some(def) => self.table(c, def, true)?,
                        // Unknown tag: the payload is never visited.
                        None => Value::Table(BTreeMap::new()),
                    };
                    Some(Value::Union { tag, value: Box::new(value) })
                }
                None => None,
            },
            // Integers, floats, enums, structs: `size` bytes + presence bit (`docs/spec/graven-format-v1.md` §4.2, field read rule).
            _ => {
                let size = fixed_size(ty).expect("fixed-size type");
                let off = slot(fl);
                if !t.slot_state(off, size)? || !t.bit(bit(fl)) {
                    None
                } else {
                    let start = t.fixed_start() + off as usize;
                    let b = t.buf().get(start..start + size as usize).ok_or(Error::OutOfBounds)?;
                    Some(decode_fixed(ty, b))
                }
            }
        })
    }

    fn list(&mut self, t: &TableRef<'a>, off: u16, elem: &Ty) -> Result<Option<Value>, Error> {
        let mut out = Vec::new();
        match elem {
            Ty::Str | Ty::Bytes => {
                let Some(l) = t.strs(off)? else { return Ok(None) };
                self.ctx.charge(l.len().saturating_mul(4).saturating_add(4))?;
                for i in 0..l.len() {
                    let v = if matches!(elem, Ty::Str) {
                        Value::Str(l.get(i)?.to_string())
                    } else {
                        Value::Bytes(l.get_bytes(i)?.to_vec())
                    };
                    let n = match &v {
                        Value::Str(s) => s.len(),
                        Value::Bytes(b) => b.len(),
                        _ => unreachable!(),
                    };
                    self.ctx.charge(4 + n)?;
                    out.push(v);
                }
            }
            Ty::Table(def) => {
                let Some(l) = t.tables(off)? else { return Ok(None) };
                // Same charge as `ListR::verify`.
                let per = (l.elem_fixed_len() as usize).max(1);
                self.ctx.charge(l.len().saturating_mul(per).saturating_add(6))?;
                for i in 0..l.len() {
                    out.push(self.table(l.get(i)?, def, false)?);
                }
            }
            Ty::List(_) | Ty::Union(_) => panic!("unsupported list element type {elem:?}"),
            // `[scalar]` / `[enum]` / `[struct]`: `[u32 count][count × size]`.
            _ => {
                let Some(p) = t.ptr(off)? else { return Ok(None) };
                let size = fixed_size(elem).expect("fixed-size type") as usize;
                let count = p
                    .checked_add(4)
                    .and_then(|e| self.buf.get(p..e))
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
                    .ok_or(Error::OutOfBounds)?;
                let len = count.checked_mul(size).ok_or(Error::OutOfBounds)?;
                let data = (p + 4).checked_add(len).and_then(|e| self.buf.get(p + 4..e)).ok_or(Error::OutOfBounds)?;
                // max(1): a list of zero-size structs must still consume budget per element.
                self.ctx.charge(count.saturating_mul(size.max(1)).saturating_add(4))?;
                for i in 0..count {
                    out.push(decode_fixed(elem, &data[i * size..(i + 1) * size]));
                }
            }
        }
        Ok(Some(Value::List(out)))
    }
}
