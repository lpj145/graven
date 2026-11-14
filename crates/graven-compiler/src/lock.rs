//! Lockfile (`graven.lock`, TOML): the schema state that must survive across
//! compilations — stable schema IDs, each table's layout and the types of reserved
//! fields — plus what the compatibility check compares against the new schema.
//!
//! The output is deterministic: the order is the schema's (tables, enums, structs and unions in
//! IR order; fields by ID), with nothing that depends on hashing or the clock.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ir::*;
use graven_layout::FieldKind;

/// Version of the lock format.
pub const LOCK_FORMAT: u32 = 1;

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct Lock {
    /// Always [`LOCK_FORMAT`].
    pub format: u32,
    pub namespace: String,
    pub tables: Vec<LockTable>,
    pub enums: Vec<LockEnum>,
    pub structs: Vec<LockStruct>,
    pub unions: Vec<LockUnion>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct LockTable {
    pub name: String,
    /// `"0x%016x"`; FNV-1a of `"ns.Table"` the first time, fixed afterwards.
    pub schema_id: String,
    pub fixed_len: u16,
    /// In ID order (`fields[i].id == i`).
    pub fields: Vec<LockField>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct LockField {
    pub id: u16,
    /// Reserved field: `"_reserved_<id>"`.
    pub name: String,
    /// Canonical form ([`Ir::ty_name`]): `"u32"`, `"[Leaf]"`, `"Color"`. Also for reserved fields.
    pub ty: String,
    /// Bytes of the field in the fixed-part slot: size of the scalar/enum/struct, 4 for a
    /// pointer (`string`, `bytes`, list, table), 6 for a union (`u16` tag + `i32`
    /// offset), 0 for `bool` (it only takes bits).
    pub size: u16,
    /// Offset of the slot in the fixed part (`None` for `bool`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u16>,
    /// Presence bit `(byte, shift)` (`None` for pointer/union).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit: Option<(u16, u8)>,
    /// `"required"`, `"optional"`, `"default:<literal>"` or `"reserved"`. The literal is
    /// decimal for integers, the `{:?}` of the `f64` for floats (lossless round trip),
    /// `true`/`false` for bool and the variant name for enums.
    pub presence: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct LockEnum {
    pub name: String,
    /// `"u8"`, `"u16"` or `"u32"`.
    pub base: String,
    /// Variants in declaration order.
    pub values: Vec<LockEnumValue>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct LockEnumValue {
    pub name: String,
    pub value: u32,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct LockStruct {
    pub name: String,
    /// Size in bytes (packed fields, no padding).
    pub size: u16,
    pub fields: Vec<LockStructField>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct LockStructField {
    pub name: String,
    /// Canonical form of the type.
    pub ty: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct LockUnion {
    pub name: String,
    /// In declaration order.
    pub variants: Vec<LockUnionVariant>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct LockUnionVariant {
    pub name: String,
    pub tag: u16,
    /// Name of the variant's table.
    pub table: String,
}

/// Error reading the lock; always names the file.
#[derive(Debug)]
pub enum LockError {
    /// I/O failure (except a missing file, which is `Ok(None)`).
    Io { path: PathBuf, source: io::Error },
    /// Invalid TOML or incompatible structure (includes a missing required field).
    Parse { path: PathBuf, source: toml::de::Error },
    /// `format` different from [`LOCK_FORMAT`].
    Format { path: PathBuf, found: i64 },
    /// Well-formed TOML with invalid content (`schema_id` in the wrong format,
    /// `fields[i].id != i`).
    Invalid { path: PathBuf, what: String },
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockError::Io { path, source } => write!(f, "{}: could not read the lock: {source}", path.display()),
            LockError::Parse { path, source } => write!(f, "{}: invalid lock: {source}", path.display()),
            LockError::Invalid { path, what } => write!(f, "{}: invalid lock: {what}", path.display()),
            LockError::Format { path, found } => {
                write!(f, "{}: unsupported lock format {found} (expected {LOCK_FORMAT})", path.display())
            }
        }
    }
}

impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LockError::Io { source, .. } => Some(source),
            LockError::Parse { source, .. } => Some(source),
            LockError::Format { .. } | LockError::Invalid { .. } => None,
        }
    }
}

/// Canonical text of a default (see [`LockField::presence`]).
fn value_text(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(x) => format!("{x:?}"),
        Value::Bool(b) => b.to_string(),
        Value::Enum { name, .. } => name.clone(),
    }
}

fn presence_text(p: &Presence, ty: &Ty) -> String {
    match p {
        // f32: the text comes from the f32 value, so literals that are equal in f32 give the same text.
        Presence::Default(Value::Float(x)) if *ty == Ty::Scalar(Prim::F32) => format!("default:{:?}", *x as f32),
        Presence::Required => "required".to_string(),
        Presence::Optional => "optional".to_string(),
        Presence::Default(v) => format!("default:{}", value_text(v)),
        Presence::Reserved { .. } => "reserved".to_string(),
    }
}

/// Bytes of the field in the fixed-part slot (see [`LockField::size`]).
fn slot_size(ir: &Ir, ty: &Ty) -> u16 {
    match ir.field_kind(ty) {
        FieldKind::Scalar { size } => size,
        FieldKind::Bool => 0,
        FieldKind::Ptr => 4,
        FieldKind::Union => 6,
    }
}

/// Builds the lock from the IR. A table's `schema_id` comes from `prev` if it was already
/// there (matched by name, even if the namespace changed: the ID is fixed once
/// assigned); otherwise it is `schema_id("ns.Table")`.
pub fn lock_from_ir(ir: &Ir, prev: Option<&Lock>) -> Lock {
    let prev_ids: HashMap<&str, &str> = prev
        .map(|p| p.tables.iter().map(|t| (t.name.as_str(), t.schema_id.as_str())).collect())
        .unwrap_or_default();

    let tables = ir
        .tables
        .iter()
        .map(|t| {
            let schema_id = match prev_ids.get(t.name.as_str()) {
                Some(id) => id.to_string(),
                // An imported table keeps the id of the namespace that declares it.
                None => {
                    let qualified = match &t.origin {
                        Some(ns) => format!("{ns}.{}", crate::ir::base_name(&t.name)),
                        None => format!("{}.{}", ir.namespace, t.name),
                    };
                    format!("{:#018x}", graven::envelope::schema_id(&qualified))
                }
            };
            debug_assert_eq!(t.fields.len(), t.layout.fields.len(), "table {}: fields and layout diverge", t.name);
            let fields = t
                .fields
                .iter()
                .zip(&t.layout.fields)
                .map(|(f, l)| LockField {
                    id: f.id,
                    name: f.name.clone(),
                    ty: ir.ty_name(&f.ty),
                    size: slot_size(ir, &f.ty),
                    slot: l.slot,
                    bit: l.presence,
                    presence: presence_text(&f.presence, &f.ty),
                })
                .collect();
            LockTable { name: t.name.clone(), schema_id, fixed_len: t.layout.fixed_len, fields }
        })
        .collect();

    let enums = ir
        .enums
        .iter()
        .map(|e| LockEnum {
            name: e.name.clone(),
            base: e.base.name().to_string(),
            values: e.values.iter().map(|(n, v)| LockEnumValue { name: n.clone(), value: *v }).collect(),
        })
        .collect();

    let structs = ir
        .structs
        .iter()
        .map(|s| LockStruct {
            name: s.name.clone(),
            size: s.size,
            fields: s.fields.iter().map(|(n, t)| LockStructField { name: n.clone(), ty: ir.ty_name(t) }).collect(),
        })
        .collect();

    let unions = ir
        .unions
        .iter()
        .map(|u| LockUnion {
            name: u.name.clone(),
            variants: u
                .variants
                .iter()
                .map(|(n, tag, t)| LockUnionVariant { name: n.clone(), tag: *tag, table: ir.tables[t.0].name.clone() })
                .collect(),
        })
        .collect();

    Lock { format: LOCK_FORMAT, namespace: ir.namespace.clone(), tables, enums, structs, unions }
}

/// Reads the lock. `Ok(None)` only when the file does not exist; any other problem is an error.
pub fn read_lock(path: &Path) -> Result<Option<Lock>, LockError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(LockError::Io { path: path.to_path_buf(), source }),
    };
    let parse_err = |source| LockError::Parse { path: path.to_path_buf(), source };
    let table: toml::Table = text.parse().map_err(parse_err)?;
    // Check the version before the structure: a future format may have another shape.
    if let Some(toml::Value::Integer(found)) = table.get("format")
        && *found != i64::from(LOCK_FORMAT)
    {
        return Err(LockError::Format { path: path.to_path_buf(), found: *found });
    }
    let lock: Lock = table.try_into().map_err(parse_err)?;
    validate(&lock).map_err(|what| LockError::Invalid { path: path.to_path_buf(), what })?;
    Ok(Some(lock))
}

/// `0x` + exactly 16 lowercase hexadecimal digits.
fn valid_schema_id(s: &str) -> bool {
    s.strip_prefix("0x")
        .is_some_and(|h| h.len() == 16 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// Invariants that `lock_from_ir` and `check` assume; a hand-edited lock may break them.
fn validate(l: &Lock) -> Result<(), String> {
    for t in &l.tables {
        if !valid_schema_id(&t.schema_id) {
            return Err(format!(
                "schema_id {:?} of table `{}` is invalid (expected `0x` + 16 lowercase hex digits)",
                t.schema_id, t.name
            ));
        }
        for (i, f) in t.fields.iter().enumerate() {
            if usize::from(f.id) != i {
                return Err(format!("table `{}`: field `{}` at position {i} has id {} (expected {i})", t.name, f.name, f.id));
            }
        }
    }
    Ok(())
}

const HEADER: &str = "# graven.lock: generated by `gravenc update`; do not edit by hand.\n";

/// Writes the lock as deterministic TOML, atomically (`<path>.tmp` + rename).
pub fn write_lock(path: &Path, l: &Lock) -> io::Result<()> {
    use std::io::Write;
    let body = toml::to_string(l).map_err(io::Error::other)?;
    // Atomic: write to a temporary file next to it, sync, and rename over the target.
    let mut tmp_name = path.file_name().ok_or_else(|| io::Error::other("lock path has no file name"))?.to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(HEADER.as_bytes())?;
        f.write_all(body.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Field types according to the lock, for [`mod@crate::check`]: `(table, id)` gives the canonical
/// text of the type (`"u32"`, `"[Leaf]"`, …) of *any* field in the lock, reserved or
/// not. `check` only queries this for reserved fields without a type in the schema (and, when the
/// schema gives the type, compares against it), so also returning active fields allows
/// retiring a field by writing just `@N: reserved;`.
pub fn reserved_types(l: &Lock) -> impl Fn(&str, u16) -> Option<String> {
    let map: HashMap<(String, u16), String> = l
        .tables
        .iter()
        .flat_map(|t| t.fields.iter().map(move |f| ((t.name.clone(), f.id), f.ty.clone())))
        .collect();
    move |table, id| map.get(&(table.to_string(), id)).cloned()
}
