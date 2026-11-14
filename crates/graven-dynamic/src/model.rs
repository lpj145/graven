//! Schemas described as data.
//!
//! Table references go through [`TableLink`], which points at a `static LazyLock` (hand-written
//! schemas) or a `OnceLock` set after construction (schemas built at run time). Neither is
//! forced when the link is created, so tables can refer to themselves (`Leaf.next: Leaf?`).

use crate::interp::Value;
use std::collections::HashSet;
use std::sync::{LazyLock, OnceLock};

/// Field type.
#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
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
    Bool,
    /// Enum with a fixed unsigned base type (`U8`/`U16`/`U32`). Values are always `Value::UInt`.
    Enum { base: Box<Ty> },
    Struct(&'static StructDef),
    Str,
    Bytes,
    List(Box<Ty>),
    Table(TableLink),
    Union(&'static UnionDef),
}

/// Reference to a table definition that is resolved only when followed, which allows
/// recursive schemas. `Debug` prints only the name and equality is identity, so recursive
/// definitions never recurse in those impls.
#[derive(Clone, Copy)]
pub struct TableLink(Target);

#[derive(Clone, Copy)]
enum Target {
    Lazy(&'static LazyLock<TableDef>),
    Cell(&'static OnceLock<TableDef>),
}

impl TableLink {
    /// A link to a `static LazyLock` (hand-written schemas).
    pub const fn lazy(t: &'static LazyLock<TableDef>) -> Self {
        TableLink(Target::Lazy(t))
    }
    /// A link to a cell that is set once the table is built (schemas built at run time).
    pub const fn cell(t: &'static OnceLock<TableDef>) -> Self {
        TableLink(Target::Cell(t))
    }
    /// The table.
    ///
    /// # Panics
    /// If the link is a cell that was never set.
    pub fn get(self) -> &'static TableDef {
        match self.0 {
            Target::Lazy(l) => l,
            Target::Cell(c) => c.get().expect("TableLink followed before its table was set"),
        }
    }
    fn addr(self) -> *const () {
        match self.0 {
            Target::Lazy(l) => l as *const LazyLock<TableDef> as *const (),
            Target::Cell(c) => c as *const OnceLock<TableDef> as *const (),
        }
    }
}

impl core::ops::Deref for TableLink {
    type Target = TableDef;
    fn deref(&self) -> &TableDef {
        self.get()
    }
}

impl core::fmt::Debug for TableLink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "TableLink({})", self.get().name)
    }
}

impl PartialEq for TableLink {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.addr(), other.addr())
    }
}

/// How the reader treats a field (`docs/spec/graven-format-v1.md` §4.3).
#[derive(Debug, Clone, PartialEq)]
pub enum Presence {
    Required,
    Optional,
    /// Absent ⇒ the reader yields this value.
    Default(Value),
    /// Kept in the layout with its original type; never read or written.
    Reserved,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub id: u16,
    pub name: &'static str,
    pub ty: Ty,
    pub presence: Presence,
}

/// Table; `fields` are in ID order (IDs `0..n`, reserved ones included).
#[derive(Debug, Clone, PartialEq)]
pub struct TableDef {
    pub name: &'static str,
    pub fields: Vec<Field>,
}

impl TableDef {
    /// Field by name.
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }
    /// Index (= position in ID order) of a field by name.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }
}

/// Frozen struct: fields packed in declaration order, no padding, no presence bits.
/// Allowed field types: integers, floats, `Bool` (1 byte, 0/1), `Enum`, nested `Struct`.
#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub name: &'static str,
    pub fields: Vec<(&'static str, Ty)>,
    /// Total size in bytes (must equal the sum of the field sizes).
    pub size: u16,
}

/// Union of tables; variant tags are `1..`.
#[derive(Debug, Clone, PartialEq)]
pub struct UnionDef {
    pub name: &'static str,
    pub variants: Vec<(u16, &'static str, TableLink)>,
}

impl UnionDef {
    pub fn variant(&self, tag: u16) -> Option<&'static TableDef> {
        self.variants.iter().find(|v| v.0 == tag).map(|v| v.2.get())
    }
}

/// Structural checks of a schema and everything reachable from it: field IDs equal their
/// position (`0..n`, so strictly increasing and gap-free, as the layout assumes), enum
/// bases are `U8`/`U16`/`U32`, every struct's `size` equals the sum of its field sizes and
/// struct fields are fixed-size, union tags are non-zero and unique, and list elements
/// are not lists or unions (unsupported by the interpreter).
pub fn validate(root: &TableDef) -> Result<(), String> {
    fn table(t: &TableDef, seen: &mut HashSet<*const TableDef>) -> Result<(), String> {
        if !seen.insert(t as *const _) {
            return Ok(());
        }
        for (i, f) in t.fields.iter().enumerate() {
            if f.id as usize != i {
                return Err(format!("{}.{}: id {} at position {i}", t.name, f.name, f.id));
            }
            ty(&f.ty, &format!("{}.{}", t.name, f.name), seen)?;
        }
        Ok(())
    }
    fn ty(x: &Ty, what: &str, seen: &mut HashSet<*const TableDef>) -> Result<(), String> {
        match x {
            Ty::Enum { base } => match **base {
                Ty::U8 | Ty::U16 | Ty::U32 => Ok(()),
                ref b => Err(format!("{what}: enum base must be U8/U16/U32, got {b:?}")),
            },
            Ty::Struct(s) => {
                let mut sum = 0u32;
                for (n, ft) in &s.fields {
                    let w = format!("{}.{n}", s.name);
                    if matches!(ft, Ty::Str | Ty::Bytes | Ty::List(_) | Ty::Table(_) | Ty::Union(_)) {
                        return Err(format!("{w}: struct fields must be fixed-size"));
                    }
                    ty(ft, &w, seen)?;
                    sum += crate::interp::fixed_size(ft).unwrap_or(0) as u32;
                }
                if sum != s.size as u32 {
                    return Err(format!("{}: size {} but fields sum to {sum}", s.name, s.size));
                }
                Ok(())
            }
            Ty::List(e) => match **e {
                Ty::List(_) | Ty::Union(_) => Err(format!("{what}: unsupported list element {e:?}")),
                ref e => ty(e, what, seen),
            },
            Ty::Table(l) => table(l.get(), seen),
            Ty::Union(u) => {
                let mut tags = HashSet::new();
                for (tag, n, l) in &u.variants {
                    if *tag == 0 || !tags.insert(*tag) {
                        return Err(format!("{}.{n}: tag {tag} is zero or duplicated", u.name));
                    }
                    table(l.get(), seen)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    table(root, &mut HashSet::new())
}

