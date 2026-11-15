use graven_compiler::lock::*;
use graven_compiler::*;

fn repo_file(rel: &str) -> String {
    let p = format!("{}/../{}", env!("CARGO_MANIFEST_DIR"), rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

const CONF: &str = "graven-conformance/schemas/conformance.graven";

fn compile(src: &str) -> Ir {
    let s = parse(src).unwrap_or_else(|e| panic!("parse: {e}"));
    check(&s, &|_: &str, _: u16| None).unwrap_or_else(|d| panic!("diags: {d:?}"))
}

fn conformance() -> Ir {
    compile(&repo_file(CONF))
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("graven-lock-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.join("graven.lock")
}

#[test]
fn roundtrip() {
    let l = lock_from_ir(&conformance(), None);
    let p = tmp("roundtrip");
    write_lock(&p, &l).unwrap();
    let bytes1 = std::fs::read(&p).unwrap();
    let back = read_lock(&p).unwrap().expect("lock existe");
    assert_eq!(back, l);
    // Stable: re-writing (also from a lock rebuilt with prev) gives identical bytes.
    write_lock(&p, &back).unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), bytes1);
    let again = lock_from_ir(&conformance(), Some(&back));
    assert_eq!(again, l);
    write_lock(&p, &again).unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), bytes1);
    assert_eq!(l.format, 1);
    assert_eq!(l.namespace, "conformance.v1");
}

#[test]
fn lock_contents_match_ir() {
    let ir = conformance();
    let l = lock_from_ir(&ir, None);
    assert_eq!(l.tables.len(), ir.tables.len());
    for (lt, t) in l.tables.iter().zip(&ir.tables) {
        assert_eq!(lt.name, t.name);
        assert_eq!(lt.fixed_len, t.layout.fixed_len);
        assert_eq!(lt.fields.len(), t.fields.len());
        for (lf, (f, fl)) in lt.fields.iter().zip(t.fields.iter().zip(&t.layout.fields)) {
            assert_eq!((lf.id, &lf.name), (f.id, &f.name));
            assert_eq!(lf.ty, ir.ty_name(&f.ty));
            assert_eq!(lf.slot, fl.slot);
            assert_eq!(lf.bit, fl.presence);
        }
    }
    let root = l.tables.iter().find(|t| t.name == "Root").unwrap();
    assert_eq!(root.fields[2].presence, "default:7");
    assert_eq!(root.fields[1].presence, "optional");
    assert_eq!(root.fields[0].presence, "required");
    assert_eq!(root.fields[21].presence, "reserved");
    assert_eq!(root.fields[21].ty, "u32");
    assert_eq!(root.fields[16].ty, "[Leaf]");
    // Enums / structs / unions carry what compat needs.
    assert_eq!(l.enums.len(), ir.enums.len());
    assert_eq!(l.enums[0].name, "Color");
    assert_eq!(l.enums[0].base, "u8");
    assert_eq!(l.enums[0].values.len(), 3);
    assert_eq!(l.structs.len(), ir.structs.len());
    assert_eq!(l.structs[0].size, ir.structs[0].size);
    assert_eq!(l.structs[0].fields.len(), ir.structs[0].fields.len());
    assert_eq!(l.unions.len(), ir.unions.len());
    assert_eq!(l.unions[0].variants[0].table, "Leaf");
    assert_eq!(l.unions[0].variants[0].tag, 1);
}

#[test]
fn default_literals_roundtrip_text() {
    let ir = compile(
        "namespace a; enum E : u8 { A = 0, B = 1 } \
         table T { i @0: i32 = -5; f @1: f64 = 0.1; g @2: f32 = 1.5; b @3: bool = true; e @4: E = B; }",
    );
    let l = lock_from_ir(&ir, None);
    let p: Vec<_> = l.tables[0].fields.iter().map(|f| f.presence.as_str()).collect();
    assert_eq!(p, ["default:-5", "default:0.1", "default:1.5", "default:true", "default:B"]);
}

#[test]
fn schema_id_from_fnv_then_sticky() {
    let ir = conformance();
    let l = lock_from_ir(&ir, None);
    let root = l.tables.iter().find(|t| t.name == "Root").unwrap();
    assert_eq!(root.schema_id, format!("{:#018x}", graven::envelope::schema_id("conformance.v1.Root")));
    assert_eq!(root.schema_id.len(), 18);

    let mut prev = l.clone();
    for t in &mut prev.tables {
        if t.name == "Root" {
            t.schema_id = "0x00000000deadbeef".to_string();
        }
    }
    let l2 = lock_from_ir(&ir, Some(&prev));
    let root2 = l2.tables.iter().find(|t| t.name == "Root").unwrap();
    assert_eq!(root2.schema_id, "0x00000000deadbeef");
    // Tables not in prev still get the FNV id.
    let leaf2 = l2.tables.iter().find(|t| t.name == "Leaf").unwrap();
    assert_eq!(leaf2.schema_id, format!("{:#018x}", graven::envelope::schema_id("conformance.v1.Leaf")));
}

#[test]
fn reserved_type_from_lock() {
    let src = repo_file(CONF);
    assert!(src.contains("@21: reserved u32;"));
    let l = lock_from_ir(&conformance(), None);
    let stripped = src.replace("@21: reserved u32;", "@21: reserved;");
    assert_ne!(stripped, src);
    let s = parse(&stripped).unwrap();
    assert!(check(&s, &|_: &str, _: u16| None).is_err(), "without a lock it must fail");
    let ir = check(&s, &reserved_types(&l)).unwrap_or_else(|d| panic!("{d:?}"));
    let root = ir.tables.iter().find(|t| t.name == "Root").unwrap();
    assert_eq!(root.fields[21].ty, Ty::Scalar(Prim::U32));
    assert_eq!(root.fields[21].presence, Presence::Reserved { allow_breaking: false });
    // Unknown (table, id) gives None.
    let f = reserved_types(&l);
    assert_eq!(f("Root", 21).as_deref(), Some("u32"));
    assert_eq!(f("Nope", 0), None);
    assert_eq!(f("Root", 999), None);
}

#[test]
fn read_missing_file_is_none() {
    let p = tmp("missing");
    assert_eq!(read_lock(&p).unwrap(), None);
}

#[test]
fn lock_rejects_malformed() {
    let good = {
        let p = tmp("good");
        write_lock(&p, &lock_from_ir(&conformance(), None)).unwrap();
        std::fs::read_to_string(&p).unwrap()
    };
    let cases: Vec<(&str, String)> = vec![
        ("broken", "format = = [".to_string()),
        ("format2", good.replacen("format = 1", "format = 2", 1)),
        ("nofields", "format = 1\nnamespace = \"a\"\n[[tables]]\nname = \"T\"\nschema_id = \"0x1\"\nfixed_len = 0\n".to_string()),
    ];
    for (name, text) in cases {
        let p = tmp(name);
        std::fs::write(&p, &text).unwrap();
        let e = read_lock(&p).expect_err(name);
        let msg = e.to_string();
        assert!(msg.contains(p.to_str().unwrap()), "{name}: {msg}");
        assert!(msg.len() > p.to_str().unwrap().len() + 5, "{name}: {msg}");
    }
    let p = tmp("format2b");
    std::fs::write(&p, good.replacen("format = 1", "format = 2", 1)).unwrap();
    assert!(matches!(read_lock(&p), Err(LockError::Format { found: 2, .. })));
    // A directory is an IO error, not "absent".
    let d = p.parent().unwrap();
    assert!(matches!(read_lock(d), Err(LockError::Io { .. })));
}

#[test]
fn empty_table_roundtrips() {
    let ir = compile("namespace a; table E {} table T { x @0: u8; }");
    let l = lock_from_ir(&ir, None);
    let p = tmp("empty");
    write_lock(&p, &l).unwrap();
    assert_eq!(read_lock(&p).unwrap().unwrap(), l);
}

fn good_text(name: &str) -> String {
    let p = tmp(name);
    write_lock(&p, &lock_from_ir(&conformance(), None)).unwrap();
    std::fs::read_to_string(&p).unwrap()
}

#[test]
fn read_rejects_bad_schema_id() {
    let good = good_text("sid-good");
    let ok_id = format!("{:#018x}", graven::envelope::schema_id("conformance.v1.Leaf"));
    assert!(good.contains(&format!("schema_id = \"{ok_id}\"")));
    let bad = [
        "0xdeadbeef",                // too short
        "0X289CE2FAA6B04709",        // wrong prefix
        "0x289CE2FAA6B04709",        // uppercase
        "289ce2faa6b04709",          // no prefix
        "0x289ce2faa6b0470g",        // non-hex
        "0x289ce2faa6b047091",       // too long
        "",
    ];
    for (i, b) in bad.iter().enumerate() {
        let p = tmp(&format!("sid-{i}"));
        std::fs::write(&p, good.replacen(&ok_id, b, 1)).unwrap();
        let e = read_lock(&p).expect_err(b);
        assert!(matches!(e, LockError::Invalid { .. }), "{b:?}: {e}");
        let msg = e.to_string();
        assert!(msg.contains(p.to_str().unwrap()) && msg.contains("schema_id"), "{msg}");
    }
}

#[test]
fn read_rejects_field_id_mismatch() {
    let good = good_text("fid-good");
    let p = tmp("fid");
    // Leaf's field 1 (`s`) claims id 2.
    let bad = good.replacen("id = 1\nname = \"s\"", "id = 2\nname = \"s\"", 1);
    assert_ne!(bad, good);
    std::fs::write(&p, bad).unwrap();
    let e = read_lock(&p).unwrap_err();
    assert!(matches!(e, LockError::Invalid { .. }), "{e}");
    let msg = e.to_string();
    assert!(msg.contains(p.to_str().unwrap()) && msg.contains("Leaf"), "{msg}");
}

#[test]
fn write_is_atomic_leaves_no_tmp() {
    let p = tmp("atomic");
    let l = lock_from_ir(&conformance(), None);
    write_lock(&p, &l).unwrap();
    write_lock(&p, &l).unwrap(); // overwrite existing
    let names: Vec<_> =
        std::fs::read_dir(p.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert_eq!(names, ["graven.lock"]);
    assert_eq!(read_lock(&p).unwrap().unwrap(), l);
}

#[test]
fn f32_default_text_is_f32_canonical() {
    let ir = compile("namespace a; table T { a @0: f32 = 0.1; b @1: f32 = 0.100000001490116119384765625; c @2: f64 = 0.1; }");
    let l = lock_from_ir(&ir, None);
    let p: Vec<_> = l.tables[0].fields.iter().map(|f| f.presence.as_str()).collect();
    assert_eq!(p, ["default:0.1", "default:0.1", "default:0.1"]);
}

#[test]
fn regeneration_with_prev_keeps_reserved_and_sticky_id() {
    let src = repo_file(CONF);
    let l0 = lock_from_ir(&conformance(), None);
    let mut prev = l0.clone();
    for t in &mut prev.tables {
        t.schema_id = "0x0123456789abcdef".to_string();
    }
    let p = tmp("regen");
    write_lock(&p, &prev).unwrap();
    let prev = read_lock(&p).unwrap().unwrap();
    // Schema with the reserved type omitted: type comes from the lock.
    let s = parse(&src.replace("@21: reserved u32;", "@21: reserved;")).unwrap();
    let ir = check(&s, &reserved_types(&prev)).unwrap();
    let l1 = lock_from_ir(&ir, Some(&prev));
    assert_eq!(l1, prev);
    let root = l1.tables.iter().find(|t| t.name == "Root").unwrap();
    let r0 = l0.tables.iter().find(|t| t.name == "Root").unwrap();
    assert_eq!(root.schema_id, "0x0123456789abcdef");
    assert_eq!(root.fields[21], r0.fields[21]);
    assert_eq!((root.fields[21].ty.as_str(), root.fields[21].presence.as_str()), ("u32", "reserved"));
    assert!(root.fields[21].slot.is_some() && root.fields[21].bit.is_some());
}

#[test]
fn service_lock_roundtrip_and_no_service_backward_compat() {
    // Existing schema without services does not emit `[[services]]`.
    let no_svc_ir = compile("namespace a; table T { x @0: u32; }");
    let no_svc_toml = toml::to_string(&lock_from_ir(&no_svc_ir, None)).unwrap();
    assert!(!no_svc_toml.contains("services"), "unexpected services section in {no_svc_toml}");

    // Service schema records [[services]] and sticky service_id.
    let ir = compile(
        "namespace inv;\n\
         table Req { sku @0: string; }\n\
         table Res { ok @0: bool; }\n\
         union Err { Fail @1: Res }\n\
         #[timeout(ms = 5000)]\n\
         service Inventory {\n\
           #[timeout(ms = 500)]\n\
           get @1: (Req) -> Result<Res, Err>;\n\
           list @2: (Req) -> stream Res;\n\
           ping @3: (Req);\n\
           @4: reserved;\n\
         }",
    );
    let mut l0 = lock_from_ir(&ir, None);
    l0.services[0].service_id = "0x1122334455667788".into();
    let p = tmp("svc");
    write_lock(&p, &l0).unwrap();
    let l1 = read_lock(&p).unwrap().unwrap();
    assert_eq!(l1, l0);
    let l2 = lock_from_ir(&ir, Some(&l1));
    assert_eq!(l2.services[0].service_id, "0x1122334455667788");
    assert_eq!(l2.services[0].methods.len(), 4);
    assert_eq!(l2.services[0].methods[0].shape.as_deref(), Some("unary"));
    assert_eq!(l2.services[0].methods[1].shape.as_deref(), Some("stream"));
    assert_eq!(l2.services[0].methods[2].shape.as_deref(), Some("one-way"));
    assert!(l2.services[0].methods[3].reserved);
}
