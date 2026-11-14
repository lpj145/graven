//! The conformance schemas described as data. `conformance_root()` mirrors
//! `schemas/conformance.graven`; `bank_transaction()` / `game_frame()` mirror
//! `graven-schemas/schemas/{bank,game}.graven`.

pub use graven_dynamic::model::*;
use graven_dynamic::Value;
use std::sync::LazyLock;

fn f(id: u16, name: &'static str, ty: Ty, presence: Presence) -> Field {
    Field { id, name, ty, presence }
}

use Presence::{Optional as Opt, Required as Req};

fn list(t: Ty) -> Ty {
    Ty::List(Box::new(t))
}

// ------------------------------------------------------------ conformance.v1

pub static COLOR: LazyLock<Ty> = LazyLock::new(|| Ty::Enum { base: Box::new(Ty::U8) });

pub static POINT: LazyLock<StructDef> = LazyLock::new(|| StructDef {
    name: "conformance.v1.Point",
    fields: vec![("x", Ty::I32), ("y", Ty::I32)],
    size: 8,
});

pub static LEAF: LazyLock<TableDef> = LazyLock::new(|| TableDef {
    name: "conformance.v1.Leaf",
    fields: vec![f(0, "n", Ty::U32, Req), f(1, "s", Ty::Str, Opt), f(2, "next", Ty::Table(TableLink::lazy(&LEAF)), Opt)],
});

pub static CHOICE: LazyLock<UnionDef> =
    LazyLock::new(|| UnionDef { name: "conformance.v1.Choice", variants: vec![(1, "Leaf", TableLink::lazy(&LEAF))] });

fn root_fields() -> Vec<Field> {
    vec![
        f(0, "a_u8", Ty::U8, Req),
        f(1, "a_i16", Ty::I16, Opt),
        f(2, "a_u32", Ty::U32, Presence::Default(Value::UInt(7))),
        f(3, "a_i64", Ty::I64, Opt),
        f(4, "a_u128", Ty::U128, Opt),
        f(5, "a_i128", Ty::I128, Opt),
        f(6, "a_f32", Ty::F32, Opt),
        f(7, "a_f64", Ty::F64, Opt),
        f(8, "flag", Ty::Bool, Opt),
        f(9, "flag2", Ty::Bool, Req),
        f(10, "color", COLOR.clone(), Opt),
        f(11, "point", Ty::Struct(&POINT), Opt),
        f(12, "name", Ty::Str, Opt),
        f(13, "blob", Ty::Bytes, Opt),
        f(14, "nums", list(Ty::U16), Opt),
        f(15, "points", list(Ty::Struct(&POINT)), Opt),
        f(16, "leaves", list(Ty::Table(TableLink::lazy(&LEAF))), Opt),
        f(17, "names", list(Ty::Str), Opt),
        f(18, "blobs", list(Ty::Bytes), Opt),
        f(19, "child", Ty::Table(TableLink::lazy(&LEAF)), Opt),
        f(20, "choice", Ty::Union(&CHOICE), Opt),
        // was `old @21: u32?`
        f(21, "old", Ty::U32, Presence::Reserved),
    ]
}

pub static ROOT: LazyLock<TableDef> =
    LazyLock::new(|| TableDef { name: "conformance.v1.Root", fields: root_fields() });

pub static ROOT_V2: LazyLock<TableDef> = LazyLock::new(|| {
    let mut fields = root_fields();
    fields.push(f(22, "extra", Ty::U64, Opt));
    TableDef { name: "conformance.v1.RootV2", fields }
});

pub fn conformance_root() -> &'static TableDef {
    &ROOT
}

pub fn conformance_root_v2() -> &'static TableDef {
    &ROOT_V2
}

pub fn conformance_leaf() -> &'static TableDef {
    &LEAF
}

pub static FLAGS: LazyLock<StructDef> = LazyLock::new(|| StructDef {
    name: "conformance.v1.Flags",
    fields: vec![("on", Ty::Bool), ("c", COLOR.clone())],
    size: 2,
});

pub static EMPTY: LazyLock<StructDef> =
    LazyLock::new(|| StructDef { name: "conformance.v1.Empty", fields: vec![], size: 0 });

/// §10.3 vectors that `Root` cannot reach (see `schemas/conformance.graven`).
pub static EXTRA: LazyLock<TableDef> = LazyLock::new(|| TableDef {
    name: "conformance.v1.Extra",
    fields: vec![
        f(0, "flags", Ty::Struct(&FLAGS), Opt),
        f(1, "bools", list(Ty::Bool), Opt),
        f(2, "colors", list(COLOR.clone()), Opt),
        f(3, "names", list(Ty::Str), Opt),
        f(4, "blobs", list(Ty::Bytes), Opt),
        f(5, "pick", Ty::Union(&CHOICE), Req),
        f(6, "leaves", list(Ty::Table(TableLink::lazy(&LEAF))), Opt),
        f(7, "e", Ty::Struct(&EMPTY), Opt),
        f(8, "_reserved_8", Ty::U64, Presence::Reserved),
        f(9, "tail", Ty::Str, Opt),
    ],
});

pub static HOLDER: LazyLock<TableDef> = LazyLock::new(|| TableDef {
    name: "conformance.v1.Holder",
    fields: vec![f(0, "items", list(Ty::Table(TableLink::lazy(&LEAF))), Req)],
});

pub fn conformance_extra() -> &'static TableDef {
    &EXTRA
}

pub fn conformance_holder() -> &'static TableDef {
    &HOLDER
}

// ------------------------------------------------------------------- bank.v1

pub static BANK_ENTRY: LazyLock<TableDef> = LazyLock::new(|| TableDef {
    name: "bank.v1.Entry",
    fields: vec![f(0, "account", Ty::U64, Req), f(1, "amount", Ty::I64, Req), f(2, "memo", Ty::Str, Opt)],
});

pub static BANK_TRANSACTION: LazyLock<TableDef> = LazyLock::new(|| TableDef {
    name: "bank.v1.Transaction",
    fields: vec![
        f(0, "id", Ty::U64, Req),
        f(1, "ts", Ty::U64, Req),
        f(2, "currency", Ty::Enum { base: Box::new(Ty::U16) }, Req),
        f(3, "from_shard", Ty::U32, Req),
        f(4, "to_shard", Ty::U32, Req),
        f(5, "entries", list(Ty::Table(TableLink::lazy(&BANK_ENTRY))), Req),
        f(6, "note", Ty::Str, Opt),
    ],
});

pub fn bank_transaction() -> &'static TableDef {
    &BANK_TRANSACTION
}

pub fn bank_entry() -> &'static TableDef {
    &BANK_ENTRY
}

// ------------------------------------------------------------------- game.v1

pub static GAME_VEC3: LazyLock<StructDef> = LazyLock::new(|| StructDef {
    name: "game.v1.Vec3",
    fields: vec![("x", Ty::F32), ("y", Ty::F32), ("z", Ty::F32)],
    size: 12,
});

pub static GAME_ENTITY: LazyLock<TableDef> = LazyLock::new(|| TableDef {
    name: "game.v1.Entity",
    fields: vec![
        f(0, "id", Ty::U32, Req),
        f(1, "pos", Ty::Struct(&GAME_VEC3), Req),
        f(2, "vel", Ty::Struct(&GAME_VEC3), Req),
        f(3, "hp", Ty::U16, Req),
        f(4, "alive", Ty::Bool, Req),
    ],
});

pub static GAME_FRAME: LazyLock<TableDef> = LazyLock::new(|| TableDef {
    name: "game.v1.Frame",
    fields: vec![f(0, "tick", Ty::U64, Req), f(1, "entities", list(Ty::Table(TableLink::lazy(&GAME_ENTITY))), Req)],
});

pub fn game_frame() -> &'static TableDef {
    &GAME_FRAME
}
