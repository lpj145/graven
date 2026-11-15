use proptest::prelude::*;
use graven_compiler::*;

fn repo_file(rel: &str) -> String {
    let p = format!("{}/../{}", env!("CARGO_MANIFEST_DIR"), rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

const FILES: [&str; 5] = [
    "graven-schemas/schemas/bank.graven",
    "graven-schemas/schemas/game.graven",
    "graven-schemas/schemas/iot.graven",
    "graven-schemas/schemas/stream.graven",
    "graven-conformance/schemas/conformance.graven",
];

fn table<'a>(s: &'a Schema, name: &str) -> &'a TableDecl {
    s.items
        .iter()
        .find_map(|i| match i {
            Item::Table(t) if t.name == name => Some(t),
            _ => None,
        })
        .unwrap()
}

fn named(s: &str) -> TypeRef {
    TypeRef::Named(s.into())
}

#[test]
fn parses_all_repo_schemas() {
    for f in FILES {
        let src = repo_file(f);
        parse(&src).unwrap_or_else(|e| panic!("{f}: {e}"));
    }
    let bank = parse(&repo_file(FILES[0])).unwrap();
    assert_eq!(bank.namespace.0, "bank");
    let tables = bank.items.iter().filter(|i| matches!(i, Item::Table(_))).count();
    let enums = bank.items.iter().filter(|i| matches!(i, Item::Enum(_))).count();
    assert_eq!((tables, enums), (2, 1));

    let conf = parse(&repo_file(FILES[4])).unwrap();
    for t in ["Root", "RootV2"] {
        let f = table(&conf, t).fields.iter().find(|f| f.id == 21).unwrap();
        assert_eq!(f.name, None);
        assert_eq!(f.ty, Some(named("u32")));
    }
    let root = table(&conf, "Root");
    let a = root.fields.iter().find(|f| f.id == 2).unwrap();
    assert_eq!(a.default, Some(Literal::Int(7)));
    let n = root.fields.iter().find(|f| f.id == 14).unwrap();
    assert_eq!(n.ty, Some(TypeRef::List(Box::new(named("u16")))));
    assert!(n.optional);
}

#[test]
fn error_has_line_col() {
    let e = parse("namespace a;\ntable T { x @0 u8; }").unwrap_err();
    assert_eq!(e.span.line, 2);
    assert_eq!(e.span.col, 16);
    assert!(e.msg.contains("':'"), "{}", e.msg);
    assert!(e.to_string().starts_with("2:16: "), "{e}");
}

#[test]
fn attrs_and_defaults() {
    let s = parse(
        "namespace a;\nenum Color : u8 { BRL = 0 }\n\
         table T { #[allow_breaking_readers] @3: reserved; c @1: Color = BRL; }",
    )
    .unwrap();
    let t = table(&s, "T");
    let r = &t.fields[0];
    assert_eq!((r.id, r.name.clone(), r.ty.clone()), (3, None, None));
    assert_eq!(r.attrs.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["allow_breaking_readers"]);
    let c = &t.fields[1];
    assert_eq!(c.name.as_deref(), Some("c"));
    assert_eq!(c.ty, Some(named("Color")));
    assert_eq!(c.default, Some(Literal::Ident("BRL".into())));
    assert!(!c.optional && c.attrs.is_empty());
}

#[test]
fn comments_literals_and_lists() {
    let s = parse(
        "// c\nnamespace a.b; /* x\n y */\n\
         enum E : u16 { A = 0x10, B = 17, }\n\
         struct S { x: f32, y: [[u8]] }\n\
         table T { a @0: i32 = -5; b @1: f64 = 1.5e2; c @2: bool = true;\n\
           d @3: [string]?; @4: reserved [u8]; #[a] #[b] e @5: u8?; }\n\
         union U { L @1: S, M @2: S }",
    )
    .unwrap();
    assert_eq!(s.namespace.0, "a.b");
    let Item::Enum(e) = &s.items[0] else { panic!() };
    assert_eq!(e.base, named("u16"));
    assert_eq!(e.values.iter().map(|v| v.1).collect::<Vec<_>>(), vec![16, 17]);
    let Item::Struct(st) = &s.items[1] else { panic!() };
    assert_eq!(st.fields[1].1, TypeRef::List(Box::new(TypeRef::List(Box::new(named("u8"))))));
    let t = table(&s, "T");
    assert_eq!(t.fields[0].default, Some(Literal::Int(-5)));
    assert_eq!(t.fields[1].default, Some(Literal::Float(150.0)));
    assert_eq!(t.fields[2].default, Some(Literal::Bool(true)));
    assert!(t.fields[3].optional);
    assert_eq!(t.fields[4].ty, Some(TypeRef::List(Box::new(named("u8")))));
    assert_eq!(t.fields[5].attrs.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
    let Item::Union(u) = &s.items[3] else { panic!() };
    assert_eq!((u.variants[1].0.as_str(), u.variants[1].1), ("M", 2));
}

#[test]
fn errors_are_reported() {
    for (src, needle) in [
        ("table T {}", "namespace"),
        ("namespace a; table T { x @70000: u8; }", "u16"),
        ("namespace a; table T { x @0: u8 }", "';'"),
        ("namespace a; /* aberto", "comment"),
        ("namespace a; enum E : u8 { A = -1 }", "integer"),
        ("namespace a; struct S { x: $ }", "character"),
        ("namespace a; table T { x @0: [u8; }", "']'"),
    ] {
        let e = parse(src).unwrap_err();
        assert!(e.msg.contains(needle), "{src}: {e}");
    }
}

proptest! {
    #[test]
    fn parse_never_panics(s in proptest::collection::vec(
        proptest::sample::select(vec![
            "namespace", "table", "struct", "enum", "union", "reserved", "u8", "x", "A", " ", "\n",
            "{", "}", "[", "]", ",", ";", ":", "@", "?", "=", "#", "-", ".", "0", "12", "0x1F",
            "1.5", "//", "/*", "*/", "é", "$", "\"", "\0",
        ]), 0..400).prop_map(|v| v.concat())
    ) {
        let _ = parse(&s);
    }

    #[test]
    fn parse_never_panics_any_string(s in ".{0,400}") {
        let _ = parse(&s);
    }
}

#[test]
fn integer_literals_span_128_bits_with_sign() {
    let s = parse("namespace a; table T { a @0: u128 = 340282366920938463463374607431768211455; b @1: i128 = -170141183460469231731687303715884105728; }").unwrap();
    let Item::Table(t) = &s.items[0] else { panic!() };
    assert_eq!(t.fields[0].default, Some(Literal::UInt(u128::MAX)));
    assert_eq!(t.fields[1].default, Some(Literal::Int(i128::MIN)));
    let e = parse("namespace a; table T { a @0: i128 = -170141183460469231731687303715884105729; }").unwrap_err();
    assert!(e.msg.contains("below the i128 range"), "{e}");
    let e = parse("namespace a; table T { a @0: u128 = 340282366920938463463374607431768211456; }").unwrap_err();
    assert!(e.msg.contains("128-bit range"), "{e}");
    let e = parse("namespace a; table T { a @0: f64 = 1e309; }").unwrap_err();
    assert!(e.msg.contains("1e309 is too large for f64"), "{e}");
    let e = parse("namespace a; table T { a @0: f64 = 1e-400; }").unwrap_err();
    assert!(e.msg.contains("1e-400 is too small for f64"), "{e}");
}

#[test]
fn doc_comments_attach_to_the_next_declaration() {
    let s = parse(
        "namespace a;
/// A color.
/// Second line.
enum C : u8 {
  /// The red one.
  Red = 0,
  Blue = 1 // not a doc
}
//// four slashes: a plain comment
struct P { /// x coordinate
  x: i32 }
table T {
  /// The id.
  #[allow_breaking_readers] id @0: u32?;
  /// gone
  @1: reserved u8;
}
union U { /// A leaf.
  T @1: T }",
    )
    .unwrap();
    let d = &s.docs;
    assert_eq!(d.get("C").map(String::as_str), Some("A color.\nSecond line."));
    assert_eq!(d.get("C.Red").map(String::as_str), Some("The red one."));
    assert_eq!(d.get("C.Blue"), None);
    assert_eq!(d.get("P"), None, "`////` is a plain comment");
    assert_eq!(d.get("P.x").map(String::as_str), Some("x coordinate"));
    assert_eq!(d.get("T.id").map(String::as_str), Some("The id."));
    assert_eq!(d.get("U.T").map(String::as_str), Some("A leaf."));
    // A reserved field generates no code, so its documentation is not kept.
    assert_eq!(d.len(), 5, "{d:?}");
}

#[test]
fn dangling_doc_comments_are_errors() {
    for src in [
        "namespace a; table T { a @0: u8; /// nothing after\n }",
        "namespace a; table T { a @0: u8; }\n/// at the end",
        "namespace a; enum E : u8 { A = 0, /// dangling\n }",
    ] {
        let e = parse(src).unwrap_err();
        assert!(e.msg.contains("without a declaration after it"), "{src}: {e}");
    }
    let e = parse("namespace a; table T /// misplaced\n { a @0: u8; }").unwrap_err();
    assert!(e.msg.contains("documentation comment"), "{e}");
}

#[test]
fn imports_and_qualified_types() {
    let s = parse("namespace orders;\nimport common;\nimport acme.money as m;\ntable T { a @0: common.Uuid; b @1: [m.Money]; }").unwrap();
    let names: Vec<(&str, &str)> = s.imports.iter().map(|i| (i.namespace.as_str(), i.name.as_str())).collect();
    assert_eq!(names, [("common", "common"), ("acme.money", "m")]);
    let t = table(&s, "T");
    assert_eq!(t.fields[0].ty, Some(TypeRef::Named("common.Uuid".into())));
    assert_eq!(t.fields[1].ty, Some(TypeRef::List(Box::new(TypeRef::Named("m.Money".into())))));
    // `import` belongs before the declarations.
    assert!(parse("namespace o;\ntable T { a @0: u8; }\nimport common;").is_err());
}

#[test]
fn validation_attrs_syntax() {
    let s = parse(
        "namespace shop;\n\
         #[check(valid_period)]\n\
         table Promo {\n\
           #[len(min = 1, max = 64), check(slug)]\n\
           name @0: string;\n\
           #[range(min = -10, max = 10_000)]\n\
           delta @1: i32;\n\
         }",
    )
    .unwrap();
    let t = table(&s, "Promo");
    assert_eq!(t.attrs.len(), 1);
    assert_eq!(t.attrs[0].name, "check");
    assert_eq!(t.attrs[0].args[0].name, "valid_period");
    assert_eq!(t.attrs[0].args[0].value, None);

    let name = &t.fields[0];
    assert_eq!(name.attrs.len(), 2);
    assert_eq!(name.attrs[0].name, "len");
    assert_eq!(name.attrs[0].args[0].name, "min");
    assert_eq!(name.attrs[0].args[0].value, Some(Literal::Int(1)));
    assert_eq!(name.attrs[0].args[1].name, "max");
    assert_eq!(name.attrs[0].args[1].value, Some(Literal::Int(64)));
    assert_eq!(name.attrs[1].name, "check");
    assert_eq!(name.attrs[1].args[0].name, "slug");

    let delta = &t.fields[1];
    assert_eq!(delta.attrs[0].name, "range");
    assert_eq!(delta.attrs[0].args[0].value, Some(Literal::Int(-10)));
    assert_eq!(delta.attrs[0].args[1].value, Some(Literal::Int(10_000)));
}

#[test]
fn service_syntax_and_contextual_keywords() {
    let s = parse(
        "namespace inv;\n\
         table Req { service @0: string; stream @1: bool; Result @2: u32; }\n\
         table Res { x @0: u32; }\n\
         union Err { A @1: Res }\n\
         /// Service doc.\n\
         #[timeout(ms = 5000)]\n\
         service Inventory {\n\
           /// Method doc.\n\
           #[timeout(ms = 500)]\n\
           get @1: (Req) -> Result<Res, Err>;\n\
           list @2: (Req) -> stream Res;\n\
           fallible_stream @3: (Req) -> Result<stream Res, Err>;\n\
           ping @4: (Req);\n\
           #[allow_breaking_readers]\n\
           @5: reserved;\n\
         }",
    )
    .unwrap();
    let svc = s.items.iter().find_map(|i| match i {
        Item::Service(d) => Some(d),
        _ => None,
    }).unwrap();
    assert_eq!(svc.name, "Inventory");
    assert_eq!(svc.methods.len(), 5);
    assert_eq!(svc.methods[0].name.as_deref(), Some("get"));
    assert!(svc.methods[1].reply.as_ref().unwrap().stream);
    assert!(svc.methods[2].reply.as_ref().unwrap().stream);
    assert!(svc.methods[2].reply.as_ref().unwrap().error.is_some());
    assert!(svc.methods[3].reply.is_none());
    assert!(svc.methods[4].name.is_none());
    assert_eq!(s.docs.get("Inventory").map(String::as_str), Some("Service doc."));
    assert_eq!(s.docs.get("Inventory.get").map(String::as_str), Some("Method doc."));

    let err = parse(
        "namespace inv;\n\
         service S { m @1: (Req) -> stream Result<Res, Err>; }",
    )
    .unwrap_err();
    assert!(err.msg.contains("stream Result"), "got: {}", err.msg);
}
