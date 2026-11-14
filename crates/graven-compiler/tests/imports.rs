//! `import` between schema files, through `gravenc`: resolution, errors, compatibility of
//! imported types, and the data tools on messages that use them.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("imports-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn gravenc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gravenc")).args(args).output().unwrap()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

const COMMON: &str = "namespace common;\nstruct Money { cents: i64, currency: u16 }\nenum Kind : u8 { A = 0, B = 1 }\ntable Tag { name @0: string; }\n";
const ORDERS: &str =
    "namespace orders;\nimport common;\ntable Order {\n  id @0: u64;\n  total @1: common.Money;\n  kind @2: common.Kind = B;\n  tags @3: [common.Tag]?;\n}\n";

/// `common.graven` and `orders.graven` with their locks.
fn project(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let d = tmp(name);
    let (c, o) = (d.join("common.graven"), d.join("orders.graven"));
    std::fs::write(&c, COMMON).unwrap();
    std::fs::write(&o, ORDERS).unwrap();
    for f in [&c, &o] {
        let r = gravenc(&["update", s(f), "--init"]);
        assert_eq!(r.status.code(), Some(0), "{}", err(&r));
    }
    (d, c, o)
}

#[test]
fn imported_types_resolve_and_are_recorded_in_the_lock() {
    let (_d, c, o) = project("ok");
    assert_eq!(gravenc(&["check", "--locked", s(&o)]).status.code(), Some(0));
    assert_eq!(gravenc(&["check", "--locked", s(&c), s(&o)]).status.code(), Some(0));
    let lock = std::fs::read_to_string(format!("{}.lock", o.display())).unwrap();
    for name in ["common.Money", "common.Kind", "common.Tag"] {
        assert!(lock.contains(&format!("name = \"{name}\"")), "{name} missing from the lock:\n{lock}");
    }
    let id = format!("{:#018x}", graven::envelope::schema_id("common.Tag"));
    assert!(lock.contains(&id), "imported table keeps its owner's schema id");
}

#[test]
fn an_alias_renames_an_import() {
    let d = tmp("alias");
    std::fs::write(d.join("common.graven"), "namespace acme.common;\nstruct Money { cents: i64 }\n").unwrap();
    let o = d.join("o.graven");
    std::fs::write(&o, "namespace orders;\nimport acme.common as money;\ntable T { m @0: money.Money; }\n").unwrap();
    assert_eq!(gravenc(&["update", s(&d.join("common.graven")), "--init"]).status.code(), Some(0));
    let r = gravenc(&["update", s(&o), "--init"]);
    assert_eq!(r.status.code(), Some(0), "{}", err(&r));
}

#[test]
fn resolution_errors_are_reported() {
    let d = tmp("errors");
    std::fs::write(d.join("common.graven"), COMMON).unwrap();
    assert_eq!(gravenc(&["update", s(&d.join("common.graven")), "--init"]).status.code(), Some(0));
    for (src, needle) in [
        ("namespace o;\nimport nope;\ntable T { a @0: u8; }\n", "namespace `nope` is not among the schemas compiled together"),
        ("namespace o;\nimport common;\ntable T { a @0: common.Nope; }\n", "namespace `common` has no type `Nope`"),
        ("namespace o;\ntable T { a @0: common.Money; }\n", "no import is named `common`"),
        ("namespace o;\nimport common;\nimport common;\ntable T { a @0: u8; }\n", "two imports are named `common`"),
        ("namespace o;\nimport o;\ntable T { a @0: u8; }\n", "cannot import its own namespace"),
        ("namespace o;\nimport common;\ntable common { a @0: u8; }\n", "is also the name of a type"),
    ] {
        let f = d.join("o.graven");
        std::fs::write(&f, src).unwrap();
        let _ = std::fs::remove_file(format!("{}.lock", f.display()));
        let r = gravenc(&["update", s(&f), "--init"]);
        assert_eq!(r.status.code(), Some(1), "{src}");
        assert!(err(&r).contains(needle), "{src}\n{}", err(&r));
    }
}

#[test]
fn import_cycles_are_errors() {
    let d = tmp("cycle");
    let (a, b) = (d.join("a.graven"), d.join("b.graven"));
    std::fs::write(&a, "namespace a;\nimport b;\nstruct X { v: u8 }\ntable T { y @0: b.Y; }\n").unwrap();
    std::fs::write(&b, "namespace b;\nimport a;\nstruct Y { v: u8 }\ntable U { x @0: a.X; }\n").unwrap();
    let r = gravenc(&["check", s(&a), s(&b)]);
    assert_eq!(r.status.code(), Some(1));
    assert!(err(&r).contains("import cycle: a → b → a") || err(&r).contains("import cycle: b → a → b"), "{}", err(&r));
}

#[test]
fn a_namespace_declared_twice_is_ambiguous() {
    let (d, _c, o) = project("twice");
    std::fs::create_dir_all(d.join("old")).unwrap();
    std::fs::write(d.join("old/common.graven"), COMMON).unwrap();
    let r = gravenc(&["check", s(&o)]);
    assert_eq!(r.status.code(), Some(1));
    assert!(err(&r).contains("namespace `common` is declared in several files"), "{}", err(&r));
}

#[test]
fn breaking_an_imported_type_is_refused_in_the_importer() {
    let (_d, c, o) = project("compat");
    // `common` changes a struct (it would refuse this itself; here its lock is recreated on purpose).
    std::fs::write(&c, COMMON.replace("cents: i64", "cents: i32")).unwrap();
    std::fs::remove_file(format!("{}.lock", c.display())).unwrap();
    assert_eq!(gravenc(&["update", s(&c), "--init"]).status.code(), Some(0));

    let r = gravenc(&["update", s(&o)]);
    assert_eq!(r.status.code(), Some(1));
    let e = err(&r);
    assert!(e.contains("imported type `common.Money` (declared in namespace `common`) changed"), "{e}");
    assert!(e.contains("(import-changed)"), "{e}");
    let d = gravenc(&["diff", s(&o)]);
    assert!(out(&d).contains("struct common.Money") && out(&d).contains("error: import-changed"), "{}", out(&d));
}

#[test]
fn a_compatible_change_to_an_imported_table_is_accepted() {
    let (_d, c, o) = project("compatible");
    std::fs::write(&c, COMMON.replace("name @0: string; }", "name @0: string; color @1: u32?; }")).unwrap();
    assert_eq!(gravenc(&["update", s(&c)]).status.code(), Some(0));
    // The importer's lock records the imported layout, so it must be refreshed, and may be.
    assert_eq!(gravenc(&["check", "--locked", s(&o)]).status.code(), Some(1));
    let r = gravenc(&["update", s(&o)]);
    assert_eq!(r.status.code(), Some(0), "{}", err(&r));
    assert_eq!(gravenc(&["check", "--locked", s(&o)]).status.code(), Some(0));
}

#[test]
fn data_tools_decode_imported_types() {
    let (d, _c, o) = project("tools");
    let json = d.join("o.json");
    std::fs::write(&json, r#"{"id": 7, "total": {"cents": 990, "currency": 986}, "tags": [{"name": "x"}]}"#).unwrap();
    let bin = d.join("o.bin");
    let r = gravenc(&["from-json", s(&o), "--root", "Order", s(&json), "-o", s(&bin)]);
    assert_eq!(r.status.code(), Some(0), "{}", err(&r));
    let r = gravenc(&["to-json", s(&o), s(&bin)]);
    assert_eq!(r.status.code(), Some(0), "{}", err(&r));
    let v: serde_json::Value = serde_json::from_str(out(&r).trim()).unwrap();
    assert_eq!(v["total"]["cents"], 990);
    assert_eq!(v["kind"], "B", "imported enum, by name, with its default");
    assert_eq!(v["tags"][0]["name"], "x");
}

#[test]
fn gen_also_generates_the_imported_schemas() {
    let (d, _c, o) = project("gen");
    let outdir = d.join("out");
    let r = gravenc(&["gen", s(&o), "-o", s(&outdir)]);
    assert_eq!(r.status.code(), Some(0), "{}", err(&r));
    assert!(outdir.join("orders.rs").exists() && outdir.join("common.rs").exists());
    let code = std::fs::read_to_string(outdir.join("orders.rs")).unwrap();
    assert!(code.contains("super::common::Money"), "refers to the imported type");
    assert!(!code.contains("pub struct Money"), "does not redeclare it");
}
