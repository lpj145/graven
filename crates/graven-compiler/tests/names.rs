//! Names that would break (or collide in) the generated Rust: `check` rejects them with a
//! diagnostic and a position. One rule per test, plus a test that the repository's schemas
//! remain valid.

use graven_compiler::*;

fn compile(src: &str) -> Result<Ir, Vec<Diag>> {
    let s = parse(src).unwrap_or_else(|e| panic!("parse: {e}"));
    check(&s, &|_: &str, _: u16| None)
}

/// Exactly one diagnostic, whose message contains every `needle`; returns the position.
fn one(src: &str, needles: &[&str]) -> Span {
    let d = compile(src).expect_err("expected a diagnostic");
    assert_eq!(d.len(), 1, "expected 1 diagnostic, got {d:?}");
    for n in needles {
        assert!(d[0].msg.contains(n), "msg {:?} does not contain {n:?}", d[0].msg);
    }
    d[0].span
}

fn ok(src: &str) {
    if let Err(d) = compile(src) {
        panic!("diags: {d:?}");
    }
}

#[test]
fn repo_schemas_still_pass() {
    for rel in [
        "graven-schemas/schemas/bank.graven",
        "graven-schemas/schemas/game.graven",
        "graven-schemas/schemas/iot.graven",
        "graven-schemas/schemas/stream.graven",
        "graven-conformance/schemas/conformance.graven",
        "graven-codegen-tests/schemas/edge.graven",
        "graven-codegen-tests/schemas/names.graven",
    ] {
        let p = format!("{}/../{rel}", env!("CARGO_MANIFEST_DIR"));
        ok(&std::fs::read_to_string(&p).unwrap());
    }
}

#[test]
fn type_name_must_start_uppercase() {
    let sp = one("namespace t;\ntable foo { a @0: u32; }", &["`foo`", "uppercase"]);
    assert_eq!((sp.line, sp.col), (2, 1));
    // Rust keywords and primitives fall under the same rule.
    one("namespace t; struct str { a: u8 }", &["`str`", "uppercase"]);
    one("namespace t; enum type : u8 { A = 0 }", &["`type`", "uppercase"]);
    one("namespace t; table T { a @0: u8; } union usize { T @1: T }", &["`usize`", "uppercase"]);
    one("namespace t; table _T { a @0: u32; }", &["`_T`", "uppercase"]);
}

#[test]
fn prelude_like_type_names_are_fine() {
    // The generated code only uses absolute paths (`::core::…`), so these names do not
    // shadow anything.
    ok("namespace t; table Option { a @0: u32; } table String { o @0: Option?; } struct FnOnce { a: u8 } enum Result : u8 { Ok = 0 }");
}

#[test]
fn self_super_crate_rejected_everywhere() {
    one("namespace t; table Self { a @0: u32; }", &["`Self`", "Rust"]);
    let sp = one("namespace t;\ntable T {\n  self @0: u32;\n}", &["`self`", "Rust"]);
    assert_eq!(sp.line, 3);
    one("namespace t; struct S { super: u8 }", &["`super`"]);
    one("namespace t; enum E : u8 { crate = 0 }", &["`crate`"]);
    one("namespace t; table A { x @0: u8; } union U { Self @1: A }", &["`Self`"]);
    one("namespace t; table T { _ @0: u32; }", &["`_`"]);
}

#[test]
fn raw_identifier_keywords_are_fine() {
    ok("namespace t; enum E : u8 { type = 0, match = 1 } struct S { fn: u8 } table A { x @0: u8; } union U { loop @1: A } table T { type @0: u32; mod @1: U?; }");
}

#[test]
fn enum_variant_unknown_rejected() {
    let sp = one("namespace t;\nenum E : u8 {\n  A = 0,\n  Unknown = 1,\n}", &["`Unknown`", "`E`"]);
    assert_eq!(sp.line, 4);
}

#[test]
fn enum_variant_named_like_enum_method_rejected() {
    one("namespace t; enum E : u8 { from_raw = 0 }", &["`from_raw`"]);
    one("namespace t; enum E : u8 { to_raw = 0 }", &["`to_raw`"]);
}

#[test]
fn union_variant_unknown_rejected() {
    one("namespace t; table A { x @0: u8; } union U { Unknown @1: A }", &["`Unknown`", "`U`"]);
}

#[test]
fn snake_case_module_collision() {
    let sp = one(
        "namespace t;\ntable HTTPServer { a @0: u32; }\ntable HttpServer { a @0: u32; }",
        &["`HttpServer`", "`HTTPServer`", "http_server"],
    );
    assert_eq!(sp.line, 3);
}

#[test]
fn generated_suffix_collisions() {
    one("namespace t; table Foo { a @0: u32; } table FooW { a @0: u32; }", &["`FooW`", "`Foo`"]);
    one("namespace t; table Foo { a @0: u32; } struct FooVerified { a: u8 }", &["`FooVerified`", "`Foo`"]);
    one("namespace t; table Foo { a @0: u32; } enum FooE : u8 { A = 0 }", &["`FooE`", "`Foo`"]);
    one("namespace t; table Foo { a @0: u32; } table FooListW { a @0: u32; }", &["`FooListW`", "`Foo`"]);
    one("namespace t; table A { x @0: u8; } union U { A @1: A } table UVerified { a @0: u32; }", &["`UVerified`", "`U`"]);
}

#[test]
fn generated_const_collisions() {
    // `FOO_REQ` is the required-fields const of `FOO`; the table `FOO_REQ` has a constructor of the same name.
    one("namespace t; table FOO { a @0: u32; } table FOO_REQ { a @0: u32; }", &["`FOO_REQ`", "`FOO`"]);
    one("namespace t; table SCHEMA_ID { a @0: u32; }", &["`SCHEMA_ID`"]);
}

#[test]
fn field_named_like_reader_method_rejected() {
    for m in ["read", "verify", "verified", "verified_with", "from_bytes", "SCHEMA_ID"] {
        one(&format!("namespace t; table T {{ {m} @0: u32; }}"), &[&format!("`{m}`")]);
    }
}

#[test]
fn field_named_like_view_or_writer_method_rejected() {
    for m in ["reader", "root", "to_bytes", "seal"] {
        one(&format!("namespace t; table T {{ {m} @0: u32; }}"), &[&format!("`{m}`")]);
    }
}

#[test]
fn field_named_like_layout_const_rejected() {
    one("namespace t; table T { fixed_len @0: u32; }", &["`fixed_len`", "FIXED_LEN"]);
    one("namespace t; table T { ptr_slots @0: string; }", &["`ptr_slots`", "PTR_SLOTS"]);
    // `x` (scalar) generates `X_BIT`; the field `x_bit` generates `X_BIT` as an offset.
    one("namespace t; table T { x @0: u32; x_bit @1: u32; }", &["`x_bit`", "X_BIT"]);
    // Fields that differ only in case/underscores become the same const.
    one("namespace t; table T { fooBar @0: u32; foo_bar @1: u32; }", &["`foo_bar`", "FOO_BAR"]);
}

#[test]
fn schema_id_field_is_fine() {
    // The method `schema_id()` does not collide with the associated const `SCHEMA_ID`.
    ok("namespace t; table T { schema_id @0: u64; x @1: string; x_bit @2: u8; }");
}

#[test]
fn streaming_setter_collision() {
    let sp = one("namespace t;\ntable T {\n  xs @0: [u32];\n  xs_streaming @1: u8;\n}", &["`xs_streaming`", "`xs`"]);
    assert_eq!(sp.line, 4);
    // `[string]` has no `_streaming`.
    ok("namespace t; table T { xs @0: [string]; xs_streaming @1: u8; }");
}

#[test]
fn union_setter_collision() {
    one(
        "namespace t; table A { x @0: u8; } union U { Big @1: A } table T { u @0: U; u_big @1: u8; }",
        &["`u_big`", "`u`"],
    );
    // Two variants whose snake_case coincides.
    one(
        "namespace t; table A { x @0: u8; } union U { FooBar @1: A, Foo_Bar @2: A } table T { u @0: U; }",
        &["u_foo_bar"],
    );
}

#[test]
fn type_whose_module_would_be_self_super_crate_rejected() {
    one("namespace t; table Crate { a @0: u32; }", &["`Crate`", "`crate`"]);
    one("namespace t; table Super { a @0: u32; }", &["`Super`", "`super`"]);
    one("namespace t; table SELF { a @0: u32; }", &["`SELF`", "`self`"]);
    // Other keywords become raw identifiers (`mod r#type`).
    ok("namespace t; table Type { a @0: u32; } table Match { t @0: Type; }");
}

#[test]
fn validation_generated_names_collide_only_when_rules_exist() {
    ok("namespace t; table Checks { validate @0: u32; to_bytes_validated @1: u32; }");
    one("namespace t; table Checks { #[range(min = 1)] a @0: u32; }", &["Checks", "collides"]);
    one("namespace t; table T { #[range(min = 1)] validate @0: u32; }", &["validate", "collides"]);
    one("namespace t; table T { #[range(min = 1)] to_bytes_validated @0: u32; }", &["to_bytes_validated", "collides"]);
}
