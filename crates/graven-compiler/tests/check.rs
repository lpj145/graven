use graven_compiler::*;

fn repo_file(rel: &str) -> String {
    let p = format!("{}/../{}", env!("CARGO_MANIFEST_DIR"), rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn no_lock(_: &str, _: u16) -> Option<String> {
    None
}

fn compile(src: &str) -> Result<Ir, Vec<Diag>> {
    let s = parse(src).unwrap_or_else(|e| panic!("parse: {e}"));
    check(&s, &no_lock)
}

fn ok(src: &str) -> Ir {
    compile(src).unwrap_or_else(|d| panic!("diags: {d:?}"))
}

fn ok_file(rel: &str) -> Ir {
    ok(&repo_file(rel))
}

/// Exactly one diagnostic, whose message contains `needle`.
fn one_diag(src: &str, needle: &str) -> Diag {
    let d = compile(src).expect_err("expected a diagnostic");
    assert_eq!(d.len(), 1, "expected 1 diagnostic, got {d:?}");
    assert!(d[0].msg.contains(needle), "msg {:?} does not contain {needle:?}", d[0].msg);
    d.into_iter().next().unwrap()
}

fn table<'a>(ir: &'a Ir, name: &str) -> &'a Table {
    ir.tables.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("table {name}"))
}

fn fl(t: &Table, field: &str) -> graven_layout::FieldLayout {
    let i = t.fields.iter().position(|f| f.name == field).unwrap_or_else(|| panic!("field {field}"));
    t.layout.fields[i]
}

fn slot(t: &Table, field: &str) -> u16 {
    fl(t, field).slot.unwrap()
}

fn bit(t: &Table, field: &str) -> (u16, u8) {
    fl(t, field).presence.unwrap()
}

macro_rules! b {
    ($e:expr) => {
        ($e.byte, $e.shift)
    };
}

/// The IR layout (what the lock records) is the same one the code generated in `graven-schemas`
/// exposes as constants. This is not tautological: it catches a codegen that emits a constant
/// different from the layout it was given (the bytes written follow the constants).
#[test]
fn ir_layouts_match_generated_consts() {
    use graven_schemas::{bank, game, iot, stream};

    let ir = ok_file("graven-schemas/schemas/bank.graven");
    assert_eq!(ir.namespace, "bank");
    let t = table(&ir, "Entry");
    assert_eq!(t.layout.fixed_len, bank::entry::FIXED_LEN);
    assert_eq!(slot(t, "account"), bank::entry::ACCOUNT);
    assert_eq!(bit(t, "account"), b!(bank::entry::ACCOUNT_BIT));
    assert_eq!(slot(t, "amount"), bank::entry::AMOUNT);
    assert_eq!(bit(t, "amount"), b!(bank::entry::AMOUNT_BIT));
    assert_eq!(slot(t, "memo"), bank::entry::MEMO);
    assert_eq!(t.ptr_slots, bank::entry::PTR_SLOTS);
    assert!(!t.all_scalar);
    let t = table(&ir, "Transaction");
    assert_eq!(t.layout.fixed_len, bank::transaction::FIXED_LEN);
    assert_eq!(slot(t, "id"), bank::transaction::ID);
    assert_eq!(bit(t, "id"), b!(bank::transaction::ID_BIT));
    assert_eq!(slot(t, "ts"), bank::transaction::TS);
    assert_eq!(bit(t, "ts"), b!(bank::transaction::TS_BIT));
    assert_eq!(slot(t, "currency"), bank::transaction::CURRENCY);
    assert_eq!(bit(t, "currency"), b!(bank::transaction::CURRENCY_BIT));
    assert_eq!(slot(t, "from_shard"), bank::transaction::FROM_SHARD);
    assert_eq!(bit(t, "from_shard"), b!(bank::transaction::FROM_SHARD_BIT));
    assert_eq!(slot(t, "to_shard"), bank::transaction::TO_SHARD);
    assert_eq!(bit(t, "to_shard"), b!(bank::transaction::TO_SHARD_BIT));
    assert_eq!(slot(t, "entries"), bank::transaction::ENTRIES);
    assert_eq!(slot(t, "note"), bank::transaction::NOTE);
    assert_eq!(t.ptr_slots, bank::transaction::PTR_SLOTS);

    let ir = ok_file("graven-schemas/schemas/game.graven");
    let t = table(&ir, "Entity");
    assert_eq!(t.layout.fixed_len, game::entity::FIXED_LEN);
    assert_eq!(slot(t, "id"), game::entity::ID);
    assert_eq!(bit(t, "id"), b!(game::entity::ID_BIT));
    assert_eq!(slot(t, "pos"), game::entity::POS);
    assert_eq!(bit(t, "pos"), b!(game::entity::POS_BIT));
    assert_eq!(slot(t, "vel"), game::entity::VEL);
    assert_eq!(bit(t, "vel"), b!(game::entity::VEL_BIT));
    assert_eq!(slot(t, "hp"), game::entity::HP);
    assert_eq!(bit(t, "hp"), b!(game::entity::HP_BIT));
    assert_eq!(fl(t, "alive").slot, None);
    assert_eq!(bit(t, "alive"), b!(game::entity::ALIVE_BIT));
    assert_eq!(t.ptr_slots, game::entity::PTR_SLOTS);
    assert!(t.all_scalar);
    assert_eq!(ir.structs[0].size, 12);
    let t = table(&ir, "Frame");
    assert_eq!(t.layout.fixed_len, game::frame::FIXED_LEN);
    assert_eq!(slot(t, "tick"), game::frame::TICK);
    assert_eq!(bit(t, "tick"), b!(game::frame::TICK_BIT));
    assert_eq!(slot(t, "entities"), game::frame::ENTITIES);
    assert_eq!(t.ptr_slots, game::frame::PTR_SLOTS);

    let ir = ok_file("graven-schemas/schemas/iot.graven");
    let t = table(&ir, "Telemetry");
    assert_eq!(t.layout.fixed_len, iot::telemetry::FIXED_LEN);
    assert_eq!(slot(t, "device"), iot::telemetry::DEVICE);
    assert_eq!(bit(t, "device"), b!(iot::telemetry::DEVICE_BIT));
    assert_eq!(slot(t, "ts"), iot::telemetry::TS);
    assert_eq!(bit(t, "ts"), b!(iot::telemetry::TS_BIT));
    assert_eq!(slot(t, "temp"), iot::telemetry::TEMP);
    assert_eq!(bit(t, "temp"), b!(iot::telemetry::TEMP_BIT));
    assert_eq!(slot(t, "humidity"), iot::telemetry::HUMIDITY);
    assert_eq!(bit(t, "humidity"), b!(iot::telemetry::HUMIDITY_BIT));
    assert_eq!(slot(t, "battery"), iot::telemetry::BATTERY);
    assert_eq!(bit(t, "battery"), b!(iot::telemetry::BATTERY_BIT));
    assert_eq!(t.ptr_slots, iot::telemetry::PTR_SLOTS);
    assert!(t.all_scalar);
    assert_eq!(t.fields[3].presence, Presence::Optional);
    assert_eq!(t.fields[0].presence, Presence::Required);

    let ir = ok_file("graven-schemas/schemas/stream.graven");
    let t = table(&ir, "Chunk");
    assert_eq!(t.layout.fixed_len, stream::chunk::FIXED_LEN);
    assert_eq!(slot(t, "stream_id"), stream::chunk::STREAM_ID);
    assert_eq!(bit(t, "stream_id"), b!(stream::chunk::STREAM_ID_BIT));
    assert_eq!(slot(t, "seq"), stream::chunk::SEQ);
    assert_eq!(bit(t, "seq"), b!(stream::chunk::SEQ_BIT));
    assert_eq!(slot(t, "data"), stream::chunk::DATA);
    assert_eq!(t.ptr_slots, stream::chunk::PTR_SLOTS);
}

#[test]
fn layout_matches_conformance_model() {
    use graven_conformance::{conformance_leaf, conformance_root, conformance_root_v2, layout};
    let ir = ok_file("graven-conformance/schemas/conformance.graven");
    for (name, def) in [("Root", conformance_root()), ("RootV2", conformance_root_v2()), ("Leaf", conformance_leaf())] {
        let t = table(&ir, name);
        assert_eq!(t.layout, layout(def), "{name}");
        assert_eq!(t.fields.len(), def.fields.len(), "{name}");
        for (f, m) in t.fields.iter().zip(&def.fields) {
            use graven_conformance::{Presence as MP, Value as MV};
            assert_eq!(f.id, m.id, "{name}");
            if !matches!(f.presence, Presence::Reserved { .. }) {
                assert_eq!(f.name, m.name, "{name}");
            }
            let same = match (&f.presence, &m.presence) {
                (Presence::Required, MP::Required) | (Presence::Optional, MP::Optional) => true,
                (Presence::Reserved { allow_breaking: false }, MP::Reserved) => true,
                (Presence::Default(Value::UInt(a)), MP::Default(MV::UInt(b))) => a == b,
                (Presence::Default(Value::Int(a)), MP::Default(MV::Int(b))) => a == b,
                _ => false,
            };
            assert!(same, "{name}.{}: {:?} vs {:?}", f.name, f.presence, m.presence);
        }
    }
    let root = table(&ir, "Root");
    assert_eq!(table(&ir, "Leaf").ptr_slots, [5, 9]);
    assert_eq!(table(&ir, "RootV2").ptr_slots, root.ptr_slots);
    assert_eq!(root.layout.fixed_len, 112);
    assert_eq!(table(&ir, "RootV2").layout.fixed_len, 120);
    assert_eq!(table(&ir, "Leaf").layout.fixed_len, 13);
    let r = &root.fields[21];
    assert_eq!(r.name, "_reserved_21");
    assert_eq!(r.ty, Ty::Scalar(Prim::U32));
    assert_eq!(r.presence, Presence::Reserved { allow_breaking: false });
    assert_eq!(root.fields[2].presence, Presence::Default(Value::UInt(7)));
    assert_eq!(root.fields[16].ty, Ty::List(Box::new(Ty::Table(TableId(0)))));
    assert_eq!(ir.ty_name(&root.fields[16].ty), "[Leaf]");
    assert_eq!(ir.ty_name(&root.fields[20].ty), "Choice");
    assert_eq!(ir.ty_name(&root.fields[11].ty), "Point");
    // Leaf.next: Leaf? — recursion through a table pointer is fine.
    assert_eq!(table(&ir, "Leaf").fields[2].ty, Ty::Table(TableId(0)));
    assert_eq!(ir.enums[0].base, Prim::U8);
    assert_eq!(ir.unions[0].variants, vec![("Leaf".to_string(), 1, TableId(0))]);
}

#[test]
fn union_ptr_slot_is_off_plus_2() {
    let ir = ok_file("graven-conformance/schemas/conformance.graven");
    let root = table(&ir, "Root");
    assert_eq!(slot(root, "choice"), 102);
    assert!(root.ptr_slots.contains(&104));
    assert!(!root.ptr_slots.contains(&102));
    assert_eq!(root.ptr_slots, [70, 74, 78, 82, 86, 90, 94, 98, 104]);
    assert!(!root.all_scalar);
}

#[test]
fn reserved_from_lock_and_attr() {
    let src = "namespace a; table T { x @0: u8; #[allow_breaking_readers] @1: reserved; }";
    let s = parse(src).unwrap();
    let lock = |t: &str, id: u16| (t == "T" && id == 1).then(|| "u64".to_string());
    let ir = check(&s, &lock).unwrap();
    let t = &ir.tables[0];
    assert_eq!(t.fields[1].ty, Ty::Scalar(Prim::U64));
    assert_eq!(t.fields[1].presence, Presence::Reserved { allow_breaking: true });
    assert_eq!(t.layout.fixed_len, 10);
}

#[test]
fn reserved_type_must_match_lock() {
    let s = parse("namespace a; table T { @0: reserved u32; }").unwrap();
    let d = check(&s, &|_: &str, _: u16| Some("u64".to_string())).unwrap_err();
    assert_eq!(d.len(), 1);
    assert!(d[0].msg.contains("differs from the lock"), "{:?}", d[0].msg);
}

#[test]
fn bool_in_struct_is_one_byte() {
    let ir = ok("namespace a; enum E : u16 { A = 1 } struct S { a: bool, b: u32, e: E } \
                struct W { s: S, c: bool } table T { s @0: W; }");
    assert_eq!(ir.structs[0].size, 7);
    assert_eq!(ir.structs[1].size, 8);
    assert_eq!(ir.tables[0].layout.fixed_len, 9);
}

#[test]
fn defaults_are_typed() {
    let ir = ok("namespace a; enum C : u8 { Red = 0, Blue = 2 } table T { \
                 a @0: i8 = -128; b @1: f32 = 1.5; c @2: bool = true; d @3: C = Blue; e @4: f64 = 2; }");
    let f = &ir.tables[0].fields;
    assert_eq!(f[0].presence, Presence::Default(Value::Int(-128)));
    assert_eq!(f[1].presence, Presence::Default(Value::Float(1.5)));
    assert_eq!(f[2].presence, Presence::Default(Value::Bool(true)));
    assert_eq!(f[3].presence, Presence::Default(Value::Enum { name: "Blue".into(), raw: 2 }));
    assert_eq!(f[4].presence, Presence::Default(Value::Float(2.0)));
}

// ---- one diagnostic per rule ----

#[test]
fn diag_id_hole() {
    let d = one_diag("namespace a; table T { a @0: u8; b @2: u8; }", "field @1 is missing");
    assert_eq!(d.span, Span { line: 1, col: 14 });
}

#[test]
fn diag_duplicate_id() {
    one_diag("namespace a; table T { a @0: u8; b @0: u8; }", "duplicate id @0");
}

#[test]
fn diag_recursive_struct() {
    one_diag("namespace a; struct S { x: u8, s: S } table T { s @0: S; }", "recursive");
    one_diag("namespace a; struct A { b: B } struct B { a: A }", "recursive");
}

#[test]
fn diag_enum_base_u64() {
    one_diag("namespace a; enum E : u64 { A = 0 }", "base type of enum `E` must be u8, u16 or u32");
}

#[test]
fn diag_enum_value_out_of_base() {
    one_diag("namespace a; enum E : u8 { A = 256 }", "does not fit in u8");
}

#[test]
fn diag_union_non_table_variant() {
    one_diag("namespace a; struct P { x: u8 } union U { P @1: P }", "is not a table");
}

#[test]
fn diag_union_tag_zero_and_dup() {
    one_diag("namespace a; table L { } union U { L @0: L }", "tag must be > 0");
    one_diag("namespace a; table L { } table M { } union U { L @1: L, M @1: M }", "duplicate tag 1");
}

#[test]
fn diag_list_of_list() {
    one_diag("namespace a; table T { x @0: [[u8]]; }", "list of lists");
}

#[test]
fn diag_list_of_union() {
    one_diag("namespace a; table L { } union U { L @1: L } table T { x @0: [U]; }", "list of unions");
}

#[test]
fn diag_default_on_string() {
    one_diag("namespace a; table T { s @0: string = 1; }", "defaults are only allowed on scalar, enum or bool fields");
}

#[test]
fn diag_default_wrong_type() {
    one_diag("namespace a; table T { s @0: u8 = 256; }", "does not fit in u8");
    one_diag("namespace a; table T { s @0: u8 = true; }", "is not a u8");
    one_diag("namespace a; enum E : u8 { A = 0 } table T { e @0: E = B; }", "is not a variant of `E`");
    one_diag("namespace a; table T { s @0: f32 = 1e300; }", "is too large for f32");
}

#[test]
fn diag_reserved_without_type_or_lock() {
    one_diag("namespace a; table T { x @0: u8; @1: reserved; }", "reserved field @1 of `T` has no type");
}

#[test]
fn diag_unknown_type() {
    one_diag("namespace a; table T { x @0: Nope; }", "unknown type `Nope`");
}

#[test]
fn diag_duplicate_names() {
    one_diag("namespace a; table T { } struct T { }", "`T` declared more than once");
    one_diag("namespace a; table T { x @0: u8; x @1: u8; }", "duplicate field `x`");
}

#[test]
fn diag_struct_with_pointer() {
    one_diag("namespace a; struct S { s: string }", "a struct cannot contain");
}

#[test]
fn diag_optional_with_default() {
    one_diag("namespace a; table T { x @0: u8? = 1; }", "optional and have a default");
}

#[test]
fn diags_are_collected() {
    let d = compile("namespace a; enum E : u64 { A = 0 } table T { x @0: [[u8]]; y @2: Nope; }").unwrap_err();
    assert!(d.len() >= 4, "{d:?}");
}

#[test]
fn lock_type_text_round_trips() {
    // Every field type of `conformance.graven` (lists, nested names, enum, struct, union)
    // plus a few extra: `ty_name` → callback → resolved by `check` ⇒ same `Ty`.
    let base = repo_file("graven-conformance/schemas/conformance.graven");
    let ir = ok(&base);
    let mut tys: Vec<Ty> = ir.tables.iter().flat_map(|t| t.fields.iter().map(|f| f.ty.clone())).collect();
    tys.extend([
        Ty::List(Box::new(Ty::Enum(EnumId(0)))),
        Ty::List(Box::new(Ty::Bool)),
        Ty::List(Box::new(Ty::Scalar(Prim::F64))),
        Ty::Scalar(Prim::I128),
    ]);
    let src = format!("{base}\ntable X {{ @0: reserved; }}");
    let s = parse(&src).unwrap();
    for ty in tys {
        let text = ir.ty_name(&ty);
        let lock = |t: &str, id: u16| (t == "X" && id == 0).then(|| text.clone());
        let ir2 = check(&s, &lock).unwrap_or_else(|d| panic!("{text}: {d:?}"));
        let x = ir2.tables.iter().find(|t| t.name == "X").unwrap();
        assert_eq!(x.fields[0].ty, ty, "{text}");
        assert_eq!(ir2.ty_name(&x.fields[0].ty), text);
    }
}

#[test]
fn diag_lock_type_unknown_or_malformed() {
    let s = parse("namespace a; table T { x @0: u8; @1: reserved; }").unwrap();
    for (text, needle) in [
        ("Gone", "lock, reserved field @1 of `T`: unknown type `Gone`"),
        ("[u8", "lock, reserved field @1 of `T`: invalid type `[u8`"),
        ("u8 u8", "lock, reserved field @1 of `T`: invalid type `u8 u8`"),
        ("[[u8]]", "lock, reserved field @1 of `T`: list of lists"),
    ] {
        let d = check(&s, &|_: &str, _: u16| Some(text.to_string())).unwrap_err();
        assert_eq!(d.len(), 1, "{text}: {d:?}");
        assert!(d[0].msg.contains(needle), "{text}: {:?}", d[0].msg);
    }
}

#[test]
fn diag_field_name_with_reserved_prefix() {
    one_diag("namespace a; table T { _reserved_1 @0: u8; @1: reserved u8; }", "the prefix `_reserved_`");
}

#[test]
fn diag_struct_cycle_reported_once() {
    one_diag("namespace a; struct A { a1: A, a2: A }", "struct `A` is recursive: A -> A");
    one_diag("namespace a; struct A { b1: B, b2: B } struct B { a1: A, a2: A }", "A -> B -> A");
}

fn hand_table(f: FieldDecl) -> Schema {
    let sp = Span { line: 1, col: 1 };
    Schema {
        namespace: ("a".into(), sp),
        imports: Vec::new(),
        items: vec![Item::Table(TableDecl { name: "T".into(), attrs: vec![], fields: vec![f], span: sp })],
        docs: Default::default(),
    }
}

#[test]
fn diag_malformed_ast_fields() {
    let sp = Span { line: 1, col: 1 };
    let base = FieldDecl {
        id: 0,
        name: None,
        ty: Some(TypeRef::Named("u32".into())),
        optional: false,
        default: None,
        attrs: vec![],
        span: sp,
    };
    let cases = [
        (FieldDecl { optional: true, ..base.clone() }, "cannot be optional or have a default"),
        (FieldDecl { default: Some(Literal::Int(1)), ..base.clone() }, "cannot be optional or have a default"),
        (FieldDecl { name: Some("x".into()), ty: None, ..base.clone() }, "field `x` has no type"),
    ];
    for (f, needle) in cases {
        let d = check(&hand_table(f), &no_lock).unwrap_err();
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].msg.contains(needle), "{:?}", d[0].msg);
    }
}

/// An empty union would pass the check, but the generated type does not compile (unused `'a`).
#[test]
fn empty_union_rejected() {
    one_diag("namespace t; union U {} table T { u @0: U?; }", "has no variants");
}

/// `1e999999` would become `inf`; the parser refuses it, with the literal as written.
#[test]
fn float_literal_overflow_rejected() {
    for src in ["namespace a; table T { d @0: f64 = 1e999999; }", "namespace a; table T { d @0: f64 = -1e999999; }"] {
        let e = parse(src).unwrap_err();
        assert!(e.msg.contains("1e999999 is too large for f64"), "{e}");
    }
}

/// Defaults at the very ends of the integer ranges are accepted; one step beyond is not.
#[test]
fn integer_defaults_cover_the_whole_range() {
    for ok in [
        "u128 = 340282366920938463463374607431768211455",
        "u128 = 0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF",
        "i128 = -170141183460469231731687303715884105728",
        "i128 = 170141183460469231731687303715884105727",
        "u64 = 18446744073709551615",
        "i8 = -128",
    ] {
        compile(&format!("namespace a; table T {{ x @0: {ok}; }}")).unwrap_or_else(|d| panic!("{ok}: {d:?}"));
    }
    one_diag("namespace a; table T { x @0: i128 = 170141183460469231731687303715884105728; }", "does not fit in i128");
    one_diag("namespace a; table T { x @0: u64 = 18446744073709551616; }", "does not fit in u64");
    one_diag("namespace a; table T { x @0: u128 = -1; }", "does not fit in u128");
}

/// A float default that would silently become 0 or infinity is refused, with the literal as written.
#[test]
fn float_defaults_never_change_silently() {
    one_diag("namespace a; table T { x @0: f32 = 1e-50; }", "default 1e-50 is too small for f32: it would become 0");
    one_diag("namespace a; table T { x @0: f32 = 3.5e38; }", "default 3.5e38 is too large for f32");
    one_diag("namespace a; table T { x @0: u32 = 1.5; }", "default `1.5` is not a u32");
    // Rounding to the nearest representable value is normal float behavior, not an error.
    compile("namespace a; table T { x @0: f32 = 0.1; y @1: f32 = 1e-45; z @2: f64 = -0.0; }").unwrap();
}

#[test]
fn validation_rules_checked() {
    // Valid uses compile cleanly.
    compile(
        "namespace a;\n\
         enum Color : u8 { Red = 0, Green = 1 }\n\
         #[check(valid_t)]\n\
         table T {\n\
           #[range(min = 1, max = 100)] a @0: u32 = 5;\n\
           #[range(min = -1.5, max = 1.5), finite] b @1: f32;\n\
           #[len(min = 1, max = 64), check(slug)] s @2: string;\n\
           #[len(max = 10)] bs @3: bytes?;\n\
           #[known] c @4: Color;\n\
           #[known] cs @5: [Color];\n\
         }",
    )
    .unwrap();

    // Type mismatches.
    one_diag("namespace a; table T { #[range(min = 1)] s @0: string; }", "only valid on numeric fields");
    one_diag("namespace a; table T { #[len(min = 1)] n @0: u32; }", "only valid on `string`, `bytes` or list fields");
    one_diag("namespace a; table T { #[finite] n @0: u32; }", "only valid on `f32` or `f64`");
    one_diag("namespace a; table T { #[known] n @0: u32; }", "only valid on enum fields or lists of enums");
    one_diag("namespace a; table T { #[known] s @0: [string]; }", "only valid on enum fields or lists of enums");

    // Structs cannot have attributes/rules.
    one_diag("namespace a; #[check(x)] struct S { a: u32 }", "not allowed on struct");
    one_diag("namespace a; struct S { #[range(min = 1)] a: u32 }", "not allowed on fields of struct");

    // Bounds out of range / min > max.
    one_diag("namespace a; table T { #[range(max = 300)] a @0: u8; }", "out of range for `u8`");
    one_diag("namespace a; table T { #[range(min = -1)] a @0: u32; }", "out of range for `u32`");
    one_diag("namespace a; table T { #[range(min = 10, max = 5)] a @0: u32; }", "min > max");
    one_diag("namespace a; table T { #[len(min = -1)] s @0: string; }", "non-negative integer");
    one_diag("namespace a; table T { #[len(min = 10, max = 5)] s @0: string; }", "min > max");

    // Default violating its own rule.
    one_diag("namespace a; table T { #[range(min = 10)] a @0: u32 = 5; }", "below `range` minimum");
    one_diag("namespace a; table T { #[range(max = 10)] a @0: u32 = 15; }", "exceeds `range` maximum");

    // Same check(name) with different input types.
    one_diag(
        "namespace a; table T { #[check(slug)] s @0: string; #[check(slug)] n @1: u32; }",
        "`check(slug)` used on `u32`",
    );
}

#[test]
fn service_rules_enforced() {
    // Valid service with all 5 shapes, timeout inheritance and overrides, and reserved ID.
    let ir = ok(
        "namespace inv;\n\
         table Req { x @0: u32; }\n\
         table Res { y @0: u32; }\n\
         union Err { E @1: Res }\n\
         #[timeout(ms = 5000)]\n\
         service Inventory {\n\
           #[timeout(ms = 500)]\n\
           get @1: (Req) -> Res;\n\
           fallible @2: (Req) -> Result<Res, Err>;\n\
           list @3: (Req) -> stream Res;\n\
           fallible_stream @4: (Req) -> Result<stream Res, Err>;\n\
           ping @5: (Req);\n\
           @6: reserved;\n\
         }",
    );
    assert_eq!(ir.services.len(), 1);
    let svc = &ir.services[0];
    assert_eq!(svc.timeout_ms, Some(5000));
    assert_eq!(svc.methods[0].timeout_ms, Some(500));
    assert_eq!(svc.methods[1].timeout_ms, Some(5000)); // inherited
    assert_eq!(svc.methods[4].timeout_ms, None); // one-way has no timeout

    // Scalar/struct/enum/list inputs, outputs, errors rejected.
    one_diag("namespace a; table Res { y @0: u32; } service S { m @1: (u32) -> Res; }", "must be a table or union");
    one_diag("namespace a; struct St { x: u32 } table Req { x @0: u32; } service S { m @1: (Req) -> St; }", "must be a table or union");
    one_diag("namespace a; enum E : u8 { A = 1 } table Req { x @0: u32; } service S { m @1: (Req) -> Result<Req, E>; }", "must be a table or union");
    one_diag("namespace a; table Req { x @0: u32; } service S { m @1: (Req) -> [Req]; }", "must be a table or union");

    // Method IDs 0, duplicates, duplicate names.
    one_diag("namespace a; table T { x @0: u32; } service S { m @0: (T); }", "1..=65535");
    one_diag("namespace a; table T { x @0: u32; } service S { a @1: (T); b @1: (T); }", "duplicate method id @1");
    one_diag("namespace a; table T { x @0: u32; } service S { a @1: (T); a @2: (T); }", "duplicate method `a`");

    // Timeout on one-way, invalid timeout bounds, invalid attrs.
    one_diag("namespace a; table T { x @0: u32; } service S { #[timeout(ms = 100)] m @1: (T); }", "one-way");
    one_diag("namespace a; table T { x @0: u32; } #[timeout(ms = 0)] service S { }", "1..=4294967295");
    one_diag("namespace a; table T { x @0: u32; } #[check(foo)] service S { }", "not valid");
    one_diag("namespace a; table T { x @0: u32; } service S { #[allow_breaking_readers] m @1: (T); }", "not valid");
    one_diag("namespace a; table T { x @0: u32; } service S { #[timeout(ms = 10)] @1: reserved; }", "reserved method");

    // Collision with table name.
    one_diag("namespace a; table S { x @0: u32; } service S { }", "declared more than once");

    // A union's RPC writer `UW` is reserved even without a service, so a later `--side`
    // never breaks a schema that compiled before.
    one_diag(
        "namespace a; table Q { x @0: u32; } union Pick { Q @1: Q } table PickW { y @0: u32; }",
        "PickW",
    );
}

#[test]
fn align_attribute_checked() {
    let ir = ok("namespace a;\n\
         table T {\n\
           #[align(to = 16)] v @0: [f32];\n\
           #[align(to = 2)] b @1: bytes?;\n\
           #[align(to = 4096), len(max = 100)] p @2: [u8]?;\n\
         }");
    let t = &ir.tables[0];
    assert_eq!(t.fields.iter().map(|f| f.align).collect::<Vec<_>>(), vec![Some(16), Some(2), Some(4096)]);
    one_diag("namespace a; table T { #[align(to = 8)] s @0: string; }", "`align` is only valid on `bytes` or lists of numbers");
    one_diag("namespace a; struct P { x: u8 } table T { #[align(to = 8)] s @0: [P]; }", "`align` is only valid on");
    one_diag("namespace a; table T { #[align(to = 8)] s @0: [bool]; }", "`align` is only valid on");
    one_diag("namespace a; table T { #[align(to = 12)] v @0: [u32]; }", "must be a power of two from 2 to 4096");
    one_diag("namespace a; table T { #[align(to = 1)] v @0: [u32]; }", "must be a power of two from 2 to 4096");
    one_diag("namespace a; table T { #[align(to = 8192)] v @0: [u32]; }", "must be a power of two from 2 to 4096");
    one_diag("namespace a; table T { #[align] v @0: [u32]; }", "`align` needs `to = N`");
    one_diag("namespace a; table T { #[align(by = 8)] v @0: [u32]; }", "unknown `align` argument `by`");
    one_diag("namespace a; table T { #[align(to = 8), align(to = 16)] v @0: [u32]; }", "duplicate `align`");
}
