//! Resolved IR: types with references by index and the fixed-part layout already computed.
//!
//! Produced by [`mod@crate::check`]; consumed by the lock, the compatibility check
//! and the codegen. The indices (`EnumId`, `StructId`, `TableId`, `UnionId`) point into
//! `Ir::enums`, `Ir::structs`, `Ir::tables` and `Ir::unions`, which follow the declaration
//! order in the schema (per item kind).

use crate::ast::Span;
use graven_layout::{FieldKind, TableLayout};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EnumId(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StructId(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TableId(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UnionId(pub usize);

/// Fixed-size primitive type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prim {
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    F32,
    F64,
}

impl Prim {
    pub const ALL: [Prim; 12] = [
        Prim::U8,
        Prim::U16,
        Prim::U32,
        Prim::U64,
        Prim::U128,
        Prim::I8,
        Prim::I16,
        Prim::I32,
        Prim::I64,
        Prim::I128,
        Prim::F32,
        Prim::F64,
    ];

    pub fn from_name(s: &str) -> Option<Prim> {
        Prim::ALL.into_iter().find(|p| p.name() == s)
    }

    /// Name in the language (`"u32"`, `"f64"`, …).
    pub fn name(self) -> &'static str {
        match self {
            Prim::U8 => "u8",
            Prim::U16 => "u16",
            Prim::U32 => "u32",
            Prim::U64 => "u64",
            Prim::U128 => "u128",
            Prim::I8 => "i8",
            Prim::I16 => "i16",
            Prim::I32 => "i32",
            Prim::I64 => "i64",
            Prim::I128 => "i128",
            Prim::F32 => "f32",
            Prim::F64 => "f64",
        }
    }

    /// Size in bytes on the wire.
    pub fn size(self) -> u16 {
        match self {
            Prim::U8 | Prim::I8 => 1,
            Prim::U16 | Prim::I16 => 2,
            Prim::U32 | Prim::I32 | Prim::F32 => 4,
            Prim::U64 | Prim::I64 | Prim::F64 => 8,
            Prim::U128 | Prim::I128 => 16,
        }
    }

    pub fn is_float(self) -> bool {
        matches!(self, Prim::F32 | Prim::F64)
    }

    /// Range of an integer (`None` for floats). `u128` is capped at `i128::MAX`,
    /// which is the largest literal the parser produces.
    pub fn int_range(self) -> Option<(i128, i128)> {
        Some(match self {
            Prim::U8 => (0, u8::MAX as i128),
            Prim::U16 => (0, u16::MAX as i128),
            Prim::U32 => (0, u32::MAX as i128),
            Prim::U64 => (0, u64::MAX as i128),
            Prim::U128 => (0, i128::MAX),
            Prim::I8 => (i8::MIN as i128, i8::MAX as i128),
            Prim::I16 => (i16::MIN as i128, i16::MAX as i128),
            Prim::I32 => (i32::MIN as i128, i32::MAX as i128),
            Prim::I64 => (i64::MIN as i128, i64::MAX as i128),
            Prim::I128 => (i128::MIN, i128::MAX),
            Prim::F32 | Prim::F64 => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    Scalar(Prim),
    Bool,
    Enum(EnumId),
    Struct(StructId),
    Str,
    Bytes,
    List(Box<Ty>),
    Table(TableId),
    Union(UnionId),
}

/// Value of a default, already validated against the field's type.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A signed integer within the type's range.
    Int(i128),
    /// An unsigned integer within the type's range (all of `u128` included).
    UInt(u128),
    /// A finite float (for `f32`, representable as a finite f32).
    Float(f64),
    Bool(bool),
    /// An enum variant, by name, with its raw value.
    Enum { name: String, raw: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Presence {
    Required,
    Optional,
    Default(Value),
    /// `allow_breaking` = the `#[allow_breaking_readers]` attribute on the field.
    Reserved { allow_breaking: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub id: u16,
    /// Reserved field: `"_reserved_<id>"`.
    pub name: String,
    /// For a reserved field: the original type (from the schema or the lock).
    pub ty: Ty,
    pub presence: Presence,
    /// The `#[allow_breaking_readers]` attribute on the field (reserved, or no longer
    /// required): confirms `required-reserved` / `required-relaxed`.
    pub allow_breaking: bool,
    pub span: Span,
}

/// Table. `fields` in ID order (`fields[i].id == i`), parallel to `layout.fields`.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub name: String,
    pub fields: Vec<Field>,
    pub layout: TableLayout,
    /// No `Ptr`/`Union` field (reserved fields count by their original type).
    pub all_scalar: bool,
    /// Offset of each `i32` pointer in the fixed part, in ID order: the slot of a `Ptr`,
    /// `slot + 2` of a `Union`. Includes reserved fields (the slot stays 0 and is ignored).
    pub ptr_slots: Vec<u16>,
    pub span: Span,
    /// For a type imported from another schema: the namespace that declares it. Its `name`
    /// is then qualified (`common.Uuid`), and code generators refer to it instead of
    /// declaring it.
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Enum {
    pub name: String,
    /// `U8`, `U16` or `U32`.
    pub base: Prim,
    /// Variants in declaration order.
    pub values: Vec<(String, u32)>,
    pub span: Span,
    /// For a type imported from another schema: the namespace that declares it. Its `name`
    /// is then qualified (`common.Uuid`), and code generators refer to it instead of
    /// declaring it.
    pub origin: Option<String>,
}

/// Frozen struct: packed fields, no padding; `bool` takes 1 byte.
#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    pub name: String,
    pub fields: Vec<(String, Ty)>,
    pub size: u16,
    pub span: Span,
    /// For a type imported from another schema: the namespace that declares it. Its `name`
    /// is then qualified (`common.Uuid`), and code generators refer to it instead of
    /// declaring it.
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Union {
    pub name: String,
    /// `(name, tag > 0, table)` in declaration order.
    pub variants: Vec<(String, u16, TableId)>,
    pub span: Span,
    /// For a type imported from another schema: the namespace that declares it. Its `name`
    /// is then qualified (`common.Uuid`), and code generators refer to it instead of
    /// declaring it.
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ir {
    pub namespace: String,
    pub enums: Vec<Enum>,
    pub structs: Vec<Struct>,
    pub tables: Vec<Table>,
    pub unions: Vec<Union>,
    /// Documentation comments from the schema, keyed by `Type` or `Type.member`
    /// (see [`crate::ast::Schema::docs`]). Code generators attach them to what they emit.
    pub docs: std::collections::BTreeMap<String, String>,
}

/// The name of a type without its namespace qualifier: `common.Uuid` → `Uuid`.
pub fn base_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

impl Ir {
    /// Documentation of `Type` or `Type.member`, if the schema has any.
    pub fn doc(&self, key: &str) -> Option<&str> {
        self.docs.get(key).map(String::as_str)
    }

    /// Canonical form of the type in the language: `"u32"`, `"bool"`, `"string"`, `"[Leaf]"`, `"Color"`.
    pub fn ty_name(&self, ty: &Ty) -> String {
        match ty {
            Ty::Scalar(p) => p.name().to_string(),
            Ty::Bool => "bool".to_string(),
            Ty::Str => "string".to_string(),
            Ty::Bytes => "bytes".to_string(),
            Ty::List(t) => format!("[{}]", self.ty_name(t)),
            Ty::Enum(id) => self.enums[id.0].name.clone(),
            Ty::Struct(id) => self.structs[id.0].name.clone(),
            Ty::Table(id) => self.tables[id.0].name.clone(),
            Ty::Union(id) => self.unions[id.0].name.clone(),
        }
    }

    /// Shape of the field in a table's fixed part (spec §3.2).
    pub fn field_kind(&self, ty: &Ty) -> FieldKind {
        match ty {
            Ty::Bool => FieldKind::Bool,
            Ty::Str | Ty::Bytes | Ty::List(_) | Ty::Table(_) => FieldKind::Ptr,
            Ty::Union(_) => FieldKind::Union,
            Ty::Scalar(_) | Ty::Enum(_) | Ty::Struct(_) => FieldKind::Scalar { size: self.inline_size(ty).unwrap() },
        }
    }

    /// Inline size (in a struct, in a list or as a slot): `None` for pointer types.
    /// `bool` = 1 byte (in structs and lists).
    pub fn inline_size(&self, ty: &Ty) -> Option<u16> {
        match ty {
            Ty::Scalar(p) => Some(p.size()),
            Ty::Bool => Some(1),
            Ty::Enum(id) => Some(self.enums[id.0].base.size()),
            Ty::Struct(id) => Some(self.structs[id.0].size),
            Ty::Str | Ty::Bytes | Ty::List(_) | Ty::Table(_) | Ty::Union(_) => None,
        }
    }
}

/// Semantic check diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub span: Span,
    pub msg: String,
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.col, self.msg)
    }
}
