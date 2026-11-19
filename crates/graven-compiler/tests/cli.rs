//! `gravenc` (CLI) and `graven_compiler::build::Build` (the `build.rs` API), in a temporary
//! directory per test.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use graven_compiler::build::Build;

/// A new, empty directory for the test `name`.
fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(p: &Path, s: &str) {
    std::fs::write(p, s).unwrap();
}

fn gravenc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gravenc")).args(args).output().unwrap()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn lock_of(schema: &Path) -> PathBuf {
    PathBuf::from(format!("{}.lock", schema.display()))
}

fn build(schema: &Path, out: &Path) -> Result<(), String> {
    Build::new().schema(schema).out_dir(out).compile().map_err(|e| e.to_string())
}

fn ls(d: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    v.sort();
    v
}

const V1: &str = "namespace t.v1;\ntable T {\n  a @0: u32;\n  b @1: u32;\n}\n";

/// A directory with `t.graven` (= `V1`) and `out/`.
fn setup(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let d = tmp(name);
    let schema = d.join("t.graven");
    write(&schema, V1);
    let out = d.join("out");
    std::fs::create_dir_all(&out).unwrap();
    (d, schema, out)
}

/// `update`, with `--init` when the schema has no lock yet.
fn update_ok(schema: &Path) -> Output {
    let o = if lock_of(schema).exists() { gravenc(&["update", s(schema)]) } else { gravenc(&["update", "--init", s(schema)]) };
    assert_eq!(code(&o), 0, "update failed: {}", stderr(&o));
    o
}

#[test]
fn build_missing_lock_errors() {
    let (d, schema, out) = setup("missing-lock");
    let e = build(&schema, &out).unwrap_err();
    assert!(e.contains("gravenc update"), "{e}");
    assert_eq!(ls(&d), ["out", "t.graven"], "nothing created next to the schema");
    assert!(ls(&out).is_empty());
}

#[test]
fn build_init_missing_locks() {
    let (d, schema, out) = setup("init-missing-locks");
    let init = || Build::new().quiet().init_missing_locks().schema(&schema).out_dir(&out).compile().map_err(|e| e.to_string());
    init().unwrap();
    assert!(out.join("t.rs").exists());
    // The same lock `gravenc update --init` writes.
    let created = std::fs::read_to_string(lock_of(&schema)).unwrap();
    std::fs::remove_file(lock_of(&schema)).unwrap();
    update_ok(&schema);
    assert_eq!(std::fs::read_to_string(lock_of(&schema)).unwrap(), created);
    assert_eq!(ls(&d), ["out", "t.graven", "t.graven.lock"]);
    // An existing lock is never changed: out of date is still an error.
    write(&schema, &V1.replace("b @1: u32;", "b @1: u32;\n  c @2: u8?;"));
    let e = init().unwrap_err();
    assert!(e.contains("out of date"), "{e}");
    assert_eq!(std::fs::read_to_string(lock_of(&schema)).unwrap(), created);
    // A schema that fails the check gets no lock.
    std::fs::remove_file(lock_of(&schema)).unwrap();
    write(&schema, "namespace t;\ntable T { a @0: Nope; }\n");
    assert!(init().is_err());
    assert!(!lock_of(&schema).exists());
}

#[test]
fn build_update_rewrites_like_gravenc_update() {
    let (_d, schema, out) = setup("build-update");
    let upd = || Build::new().quiet().update().schema(&schema).out_dir(&out).compile().map_err(|e| e.to_string());
    // A missing lock still needs `--init` (or `init_missing_locks`).
    assert!(upd().unwrap_err().contains("lock missing"));
    update_ok(&schema);
    let v1 = std::fs::read_to_string(lock_of(&schema)).unwrap();
    // Compatible change: the build rewrites the lock with what `gravenc update` writes.
    write(&schema, &V1.replace("b @1: u32;", "b @1: u32;\n  c @2: u8?;"));
    upd().unwrap();
    assert!(out.join("t.rs").exists());
    let by_build = std::fs::read_to_string(lock_of(&schema)).unwrap();
    assert_ne!(by_build, v1);
    write(&lock_of(&schema), &v1);
    update_ok(&schema);
    assert_eq!(std::fs::read_to_string(lock_of(&schema)).unwrap(), by_build);
    // Up to date: nothing to write, the build passes without `update` too.
    build(&schema, &out).unwrap();
    // Incompatible change or unconfirmed warning: error, the lock and the code are left as they were.
    std::fs::remove_file(out.join("t.rs")).unwrap();
    for bad in [V1.replace("a @0: u32", "a @0: u64"), V1.replace("b @1: u32;", "@1: reserved;")] {
        write(&schema, &bad);
        let e = upd().unwrap_err();
        assert!(e.contains("type-changed") || e.contains("required-reserved"), "{e}");
        assert_eq!(std::fs::read_to_string(lock_of(&schema)).unwrap(), by_build);
        assert!(!out.join("t.rs").exists());
    }
}

#[test]
fn cli_run_as_library() {
    use std::ffi::OsString;
    use std::process::ExitCode;
    let (_d, schema, _out) = setup("cli-run");
    let run = |args: &[&str]| {
        let argv = std::iter::once("xtask").chain(args.iter().copied()).map(OsString::from);
        graven_compiler::cli::run(argv)
    };
    assert_eq!(run(&["update", "--init", s(&schema)]), ExitCode::SUCCESS);
    assert!(lock_of(&schema).exists());
    assert_eq!(run(&["check", "--locked", s(&schema)]), ExitCode::SUCCESS);
    assert_eq!(run(&["nope"]), ExitCode::from(2));
}

#[test]
fn update_then_build_ok() {
    let (_d, schema, out) = setup("update-build");
    update_ok(&schema);
    assert!(lock_of(&schema).exists());
    build(&schema, &out).unwrap();
    let rs = std::fs::read_to_string(out.join("t.rs")).unwrap();
    assert!(rs.starts_with("// @generated by gravenc from t.graven"), "{rs}");
    // Second time: lock unchanged.
    let before = std::fs::read(lock_of(&schema)).unwrap();
    update_ok(&schema);
    assert_eq!(std::fs::read(lock_of(&schema)).unwrap(), before);
}

#[test]
fn incompatible_change_fails_check_and_build() {
    let (_d, schema, out) = setup("incompatible");
    update_ok(&schema);
    write(&schema, &V1.replace("a @0: u32", "a @0: u64"));
    let o = gravenc(&["check", s(&schema)]);
    assert_eq!(code(&o), 1);
    let err = stderr(&o);
    assert!(err.contains("type-changed"), "{err}");
    // `file:line:column: message`
    assert!(err.contains(&format!("{}:3:3: ", schema.display())), "{err}");
    let e = build(&schema, &out).unwrap_err();
    assert!(e.contains("type-changed"), "{e}");
    assert!(!out.join("t.rs").exists());
    // `update` also refuses and does not touch the lock.
    let lock = std::fs::read(lock_of(&schema)).unwrap();
    assert_eq!(code(&gravenc(&["update", s(&schema)])), 1);
    assert_eq!(std::fs::read(lock_of(&schema)).unwrap(), lock);
}

#[test]
fn update_refuses_unconfirmed_warning() {
    let (_d, schema, out) = setup("unconfirmed");
    update_ok(&schema);
    let lock = std::fs::read(lock_of(&schema)).unwrap();
    write(&schema, &V1.replace("b @1: u32;", "@1: reserved;"));
    let o = gravenc(&["update", s(&schema)]);
    assert_eq!(code(&o), 1);
    assert!(stderr(&o).contains("required-reserved"), "{}", stderr(&o));
    assert!(!stderr(&o).contains("warning:"), "unconfirmed is an error: {}", stderr(&o));
    assert_eq!(std::fs::read(lock_of(&schema)).unwrap(), lock, "lock intacto");
    assert_eq!(code(&gravenc(&["check", s(&schema)])), 1);
    assert!(build(&schema, &out).unwrap_err().contains("required-reserved"));
}

#[test]
fn update_accepts_confirmed_warning() {
    let (_d, schema, out) = setup("confirmed");
    update_ok(&schema);
    write(&schema, &V1.replace("b @1: u32;", "#[allow_breaking_readers] @1: reserved;"));
    let o = update_ok(&schema);
    let err = stderr(&o);
    assert!(err.contains("warning: ") && err.contains("required-reserved"), "{err}");
    let lock = std::fs::read_to_string(lock_of(&schema)).unwrap();
    assert!(lock.contains("presence = \"reserved\""), "{lock}");
    // check and build accept it (a confirmed warning does not fail).
    assert_eq!(code(&gravenc(&["check", s(&schema)])), 0);
    build(&schema, &out).unwrap();
}

#[test]
fn build_does_not_touch_lock() {
    let (_d, schema, out) = setup("touch");
    update_ok(&schema);
    let lp = lock_of(&schema);
    let (bytes, mtime) = (std::fs::read(&lp).unwrap(), std::fs::metadata(&lp).unwrap().modified().unwrap());
    build(&schema, &out).unwrap();
    // Compatible change: the lock would have to change ⇒ `compile` fails and does not rewrite it.
    write(&schema, &V1.replace("b @1: u32;", "b @1: u32;\n  c @2: u8?;"));
    let e = build(&schema, &out).unwrap_err();
    assert!(e.contains("gravenc update"), "{e}");
    assert_eq!(std::fs::read(&lp).unwrap(), bytes);
    assert_eq!(std::fs::metadata(&lp).unwrap().modified().unwrap(), mtime);
    // `update` resolve.
    update_ok(&schema);
    build(&schema, &out).unwrap();
}

#[test]
fn namespace_change_is_update_error() {
    let (_d, schema, _out) = setup("namespace");
    update_ok(&schema);
    write(&schema, &V1.replace("t.v1", "t.v2"));
    let o = gravenc(&["update", s(&schema)]);
    assert_eq!(code(&o), 1);
    let err = stderr(&o);
    assert!(err.contains("namespace-changed"), "{err}");
    // Position: the namespace name.
    assert!(err.contains(&format!("{}:1:11: ", schema.display())), "{err}");
}

#[test]
fn update_from_other_baseline() {
    let d = tmp("from");
    let v1 = d.join("v1.graven");
    let v2 = d.join("v2.graven");
    write(&v1, V1);
    update_ok(&v1);
    // Incompatible with v1 ⇒ refused, without creating the v2 lock.
    write(&v2, &V1.replace("a @0: u32", "a @0: u64"));
    let o = gravenc(&["update", s(&v2), "--from", s(&lock_of(&v1))]);
    assert_eq!(code(&o), 1, "{}", stderr(&o));
    assert!(stderr(&o).contains("type-changed"));
    assert!(!lock_of(&v2).exists());
    // Compatible ⇒ writes `v2.graven.lock`, with v1's schema ids.
    write(&v2, &V1.replace("b @1: u32;", "b @1: u32;\n  c @2: u8?;"));
    let o = gravenc(&["update", s(&v2), "--from", s(&lock_of(&v1))]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let l1 = std::fs::read_to_string(lock_of(&v1)).unwrap();
    let l2 = std::fs::read_to_string(lock_of(&v2)).unwrap();
    let sid = |l: &str| l.lines().find(|x| x.starts_with("schema_id")).unwrap().to_string();
    assert_eq!(sid(&l1), sid(&l2));
    assert!(l2.contains("name = \"c\""));
    // `--from` with a nonexistent lock is an error.
    assert_eq!(code(&gravenc(&["update", s(&v2), "--from", s(&d.join("none.lock"))])), 1);
}

#[test]
fn gen_writes_file() {
    let (_d, schema, out) = setup("gen");
    update_ok(&schema);
    let o = gravenc(&["gen", s(&schema), "-o", s(&out)]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    assert!(std::fs::read_to_string(out.join("t.rs")).unwrap().starts_with("// @generated"));
    assert!(!stdout(&o).contains("cargo:"), "gen does not talk to cargo");
}

#[test]
fn check_reports_parse_and_check_errors() {
    let d = tmp("errors");
    let bad = d.join("bad.graven");
    write(&bad, "namespace t;\ntable T {\n  a @0 u32;\n}\n");
    let o = gravenc(&["check", s(&bad)]);
    assert_eq!(code(&o), 1);
    assert!(stderr(&o).starts_with(&format!("{}:3:", bad.display())), "{}", stderr(&o));
    let semantic = d.join("semantic.graven");
    write(&semantic, "namespace t;\ntable T {\n  a @0: Nope;\n}\n");
    let o = gravenc(&["check", s(&semantic)]);
    assert_eq!(code(&o), 1);
    assert!(stderr(&o).contains(&format!("{}:3:3: unknown type `Nope`", semantic.display())), "{}", stderr(&o));
    // Without a lock, `check` only validates the schema.
    let good = d.join("good.graven");
    write(&good, V1);
    assert_eq!(code(&gravenc(&["check", s(&good)])), 0);
    assert!(!lock_of(&good).exists(), "check does not write a lock");
    // Several files: fails if any fails.
    assert_eq!(code(&gravenc(&["check", s(&good), s(&semantic)])), 1);
    // Nonexistent file.
    assert_eq!(code(&gravenc(&["check", s(&d.join("none.graven"))])), 1);
}

#[test]
fn usage_errors_exit_2() {
    assert_eq!(code(&gravenc(&[])), 2);
    assert_eq!(code(&gravenc(&["frobnicate"])), 2);
    assert_eq!(code(&gravenc(&["check"])), 2);
    assert_eq!(code(&gravenc(&["gen", "x.graven"])), 2);
    assert_eq!(code(&gravenc(&["update", "a.graven", "b.graven", "--from", "x.lock"])), 2);
    assert_eq!(code(&gravenc(&["update", "a.graven", "--bogus"])), 2);
    let h = gravenc(&["--help"]);
    assert_eq!(code(&h), 0);
    assert!(stdout(&h).contains("gravenc update"));
}

#[test]
fn build_api_errors() {
    let d = tmp("api");
    // No schema / no out dir.
    assert!(Build::new().out_dir(&d).compile().is_err());
    let schema = d.join("t.graven");
    write(&schema, "namespace t;\ntable T { a @0: Nope; }\n");
    write(&lock_of(&schema), "");
    let e = build(&schema, &d).unwrap_err();
    // an empty lock is invalid
    assert!(e.contains("lock"), "{e}");
}

fn gravenc_os(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gravenc")).args(args).output().unwrap()
}

#[cfg(unix)]
#[test]
fn non_utf8_args_are_handled() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let bad = OsStr::from_bytes(b"\xffcheck");
    // Non-UTF-8 command: usage error.
    let o = gravenc_os(&[bad]);
    assert_eq!(code(&o), 2, "{}", stderr(&o));
    // Non-UTF-8 option: usage error.
    let o = gravenc_os(&[OsStr::new("check"), OsStr::from_bytes(b"-\xff")]);
    assert_eq!(code(&o), 2, "{}", stderr(&o));
    // Non-UTF-8 path: it is just a path (nonexistent here ⇒ 1, no panic).
    let d = tmp("non-utf8");
    let p = d.join(OsStr::from_bytes(b"\xff.graven"));
    let o = gravenc_os(&[OsStr::new("check"), p.as_os_str()]);
    assert_eq!(code(&o), 1, "{}", stderr(&o));
    // And it really works with a schema whose name is not UTF-8.
    write(&p, V1);
    let o = gravenc_os(&[OsStr::new("update"), OsStr::new("--init"), p.as_os_str()]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    assert!(graven_compiler::build::lock_path(&p).exists());
}

#[test]
fn check_locked_requires_current_lock() {
    let (_d, schema, _out) = setup("locked");
    // Without a lock: `check` passes with a note; `--locked` fails.
    assert_eq!(code(&gravenc(&["check", s(&schema)])), 0);
    let o = gravenc(&["check", "--locked", s(&schema)]);
    assert_eq!(code(&o), 1);
    assert!(stderr(&o).contains("lock missing") && stderr(&o).contains("gravenc update"), "{}", stderr(&o));
    update_ok(&schema);
    assert_eq!(code(&gravenc(&["check", "--locked", s(&schema)])), 0);
    // Compatible change without `update`: outdated lock.
    write(&schema, &V1.replace("b @1: u32;", "b @1: u32;\n  c @2: u8?;"));
    assert_eq!(code(&gravenc(&["check", s(&schema)])), 0);
    let o = gravenc(&["check", s(&schema), "--locked"]);
    assert_eq!(code(&o), 1);
    assert!(stderr(&o).contains("out of date"), "{}", stderr(&o));
    // Never writes the lock.
    let lock = std::fs::read(lock_of(&schema)).unwrap();
    gravenc(&["check", "--locked", s(&schema)]);
    assert_eq!(std::fs::read(lock_of(&schema)).unwrap(), lock);
}

#[test]
fn gen_creates_out_dir() {
    let (d, schema, _out) = setup("gen-mkdir");
    update_ok(&schema);
    let out = d.join("new/sub");
    let o = gravenc(&["gen", s(&schema), "-o", s(&out)]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    assert!(out.join("t.rs").exists());
}

#[test]
fn update_from_ignores_unreadable_current_lock() {
    let d = tmp("from-garbage");
    let v1 = d.join("v1.graven");
    let v2 = d.join("v2.graven");
    write(&v1, V1);
    update_ok(&v1);
    write(&v2, V1);
    write(&lock_of(&v2), "this is not toml [[[");
    let o = gravenc(&["update", s(&v2), "--from", s(&lock_of(&v1))]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    assert_eq!(std::fs::read(lock_of(&v2)).unwrap(), std::fs::read(lock_of(&v1)).unwrap());
    // Without `--from`, an unreadable lock is still an error.
    write(&lock_of(&v2), "this is not toml [[[");
    assert_eq!(code(&gravenc(&["update", s(&v2)])), 1);
}

/// Without a lock, `update` requires `--init` (new schema) or `--from` (moved schema): deleting
/// the lock or renaming the file cannot reset the compatibility check.
#[test]
fn update_without_lock_requires_init_or_from() {
    let d = tmp("init");
    let v1 = d.join("v1.graven");
    write(&v1, V1);
    let o = gravenc(&["update", s(&v1)]);
    assert_eq!(code(&o), 1);
    let e = stderr(&o);
    assert!(e.contains("--init") && e.contains("--from"), "{e}");
    assert!(!lock_of(&v1).exists());
    update_ok(&v1);
    // `--init` with an existing lock is an error and does not touch the lock.
    let before = std::fs::read(lock_of(&v1)).unwrap();
    assert_eq!(code(&gravenc(&["update", "--init", s(&v1)])), 1);
    assert_eq!(std::fs::read(lock_of(&v1)).unwrap(), before);
    // Renamed schema with an incompatible change: without `--from` it is refused, with `--from`
    // the finding shows up.
    let moved = d.join("moved.graven");
    write(&moved, &V1.replace("a @0: u32", "a @0: u64"));
    assert_eq!(code(&gravenc(&["update", s(&moved)])), 1);
    let o = gravenc(&["update", s(&moved), "--from", s(&lock_of(&v1))]);
    assert_eq!(code(&o), 1);
    assert!(stderr(&o).contains("type-changed"), "{}", stderr(&o));
    assert!(!lock_of(&moved).exists());
    // Usage: `--init` and `--from` together.
    assert_eq!(code(&gravenc(&["update", "--init", s(&moved), "--from", s(&lock_of(&v1))])), 2);
}

/// Two schemas with the same file name would generate the same `<name>.rs`.
#[test]
fn build_rejects_output_name_collision() {
    let d = tmp("collision");
    let a = d.join("t.graven");
    let b = d.join("evo").join("t.graven");
    std::fs::create_dir_all(b.parent().unwrap()).unwrap();
    write(&a, V1);
    write(&b, V1);
    update_ok(&a);
    update_ok(&b);
    let out = d.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let e = graven_compiler::build::Build::new().quiet().schema(&a).schema(&b).out_dir(&out).compile().unwrap_err().to_string();
    assert!(e.contains("same file"), "{e}");
    assert!(ls(&out).is_empty(), "nothing generated");
}

/// Two differently named tables with the same schema id are reported by `check` (across the
/// schemas given) and by the build; a namespace declared in two files is an error.
#[test]
fn schema_id_collisions_are_reported() {
    let d = tmp("id-collision");
    let a = d.join("a.graven");
    let b = d.join("b.graven");
    write(&a, "namespace a.v1;\ntable Order { id @0: u64; }\n");
    write(&b, "namespace b.v1;\ntable Invoice { id @0: u64; }\n");
    update_ok(&a);
    update_ok(&b);
    assert_eq!(code(&gravenc(&["check", "--locked", s(&a), s(&b)])), 0);

    // Force Invoice's id to Order's, as a hash collision would.
    let a_lock = std::fs::read_to_string(lock_of(&a)).unwrap();
    let id = a_lock.lines().find(|l| l.starts_with("schema_id")).unwrap().to_string();
    let b_lock = std::fs::read_to_string(lock_of(&b)).unwrap();
    let b_id = b_lock.lines().find(|l| l.starts_with("schema_id")).unwrap();
    write(&lock_of(&b), &b_lock.replace(b_id, &id));

    let o = gravenc(&["check", "--locked", s(&a), s(&b)]);
    assert_eq!(code(&o), 1);
    assert!(stderr(&o).contains("is also the id of `a.v1.Order`"), "{}", stderr(&o));
    let out = d.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let e = Build::new().schema(&a).schema(&b).out_dir(&out).compile().unwrap_err().to_string();
    assert!(e.contains("could not be told apart"), "{e}");

    // One namespace, one file: the same namespace in two files compiled together is an error.
    let v2 = d.join("a2.graven");
    write(&v2, "namespace a.v1;\ntable Order { id @0: u64; note @1: string?; }\n");
    update_ok(&v2);
    let o = gravenc(&["check", "--locked", s(&a), s(&v2)]);
    assert_eq!(code(&o), 1);
    assert!(stderr(&o).contains("is declared in exactly one file"), "{}", stderr(&o));
}

#[test]
fn gen_side_option_controls_service_codegen() {
    let d = tmp("gen-side");
    let schema = d.join("svc.graven");
    write(
        &schema,
        "namespace rpc.side.v1;\n\
         table Req { id @0: u64; }\n\
         table Resp { ok @0: bool; }\n\
         service Ping {\n\
             ping @1: (Req) -> Resp;\n\
         }\n",
    );
    update_ok(&schema);

    let out_none = d.join("none");
    let o = gravenc(&["gen", s(&schema), "-o", s(&out_none)]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let rs_none = std::fs::read_to_string(out_none.join("svc.rs")).unwrap();
    assert!(!rs_none.contains("PingCall"), "default gen must not emit server items");
    assert!(!rs_none.contains("PingClient"), "default gen must not emit client items");

    let out_srv = d.join("srv");
    let o = gravenc(&["gen", s(&schema), "-o", s(&out_srv), "--side", "server"]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let rs_srv = std::fs::read_to_string(out_srv.join("svc.rs")).unwrap();
    assert!(rs_srv.contains("pub enum PingCall"), "server side must emit PingCall");
    assert!(!rs_srv.contains("PingClient"), "server only must not emit PingClient");

    let out_cli = d.join("cli");
    let o = gravenc(&["gen", s(&schema), "-o", s(&out_cli), "--side", "client"]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let rs_cli = std::fs::read_to_string(out_cli.join("svc.rs")).unwrap();
    assert!(rs_cli.contains("pub struct PingClient"), "client side must emit PingClient");
    assert!(!rs_cli.contains("PingCall"), "client only must not emit PingCall");

    let out_both = d.join("both");
    let o = gravenc(&["gen", s(&schema), "-o", s(&out_both), "--side", "client,server"]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let rs_both = std::fs::read_to_string(out_both.join("svc.rs")).unwrap();
    assert!(rs_both.contains("pub struct PingClient"));
    assert!(rs_both.contains("pub enum PingCall"));
    assert!(rs_both.contains("PING_SERVICE_ID"));
    assert!(rs_both.contains("ping_timeouts"));

    let o = gravenc(&["gen", s(&schema), "-o", s(&out_both), "--side", "bogus"]);
    assert_eq!(code(&o), 2, "invalid --side must exit with usage error");
}

#[test]
fn gen_lang_ts_and_guard_rails() {
    let d = tmp("gen-lang-ts");
    let schema = d.join("svc.graven");
    write(
        &schema,
        "namespace ts.cli.v1;\n\
         table Req { id @0: u64; }\n\
         table Resp { ok @0: bool; }\n\
         service Ping {\n\
             ping @1: (Req) -> Resp;\n\
         }\n",
    );
    update_ok(&schema);

    let out_js = d.join("js");
    let o = gravenc(&["gen", s(&schema), "-o", s(&out_js), "--lang", "js"]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    for f in ["svc.js", "svc.d.ts", "graven.js", "graven.d.ts"] {
        assert!(out_js.join(f).is_file(), "must emit {f}");
    }
    let header = format!("// @generated by gravenc {}", env!("CARGO_PKG_VERSION"));
    for f in ["svc.js", "svc.d.ts", "graven.js", "graven.d.ts"] {
        let text = std::fs::read_to_string(out_js.join(f)).unwrap();
        assert!(text.starts_with(&header), "{f} must start with the gravenc header");
    }

    let out_rs = d.join("rs");
    let o = gravenc(&["gen", s(&schema), "-o", s(&out_rs), "--lang", "rust"]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    assert!(out_rs.join("svc.rs").is_file(), "must emit svc.rs");

    let o = gravenc(&["gen", s(&schema), "-o", s(&out_js), "--lang", "bogus"]);
    assert_eq!(code(&o), 2, "invalid --lang must exit with usage error");

    // `--lang ts` was the first name of this output; it points to `--lang js`.
    let o = gravenc(&["gen", s(&schema), "-o", s(&out_js), "--lang", "ts"]);
    assert_eq!(code(&o), 2);
    assert!(stderr(&o).contains("--lang js"), "{}", stderr(&o));

    let o = gravenc(&["gen", s(&schema), "-o", s(&out_js), "--lang", "js", "--side", "server"]);
    assert_eq!(code(&o), 1, "--lang js --side server must be refused");
    assert!(stderr(&o).contains("the JavaScript server edge is not available yet"), "{}", stderr(&o));

    let graven_schema = d.join("graven.graven");
    write(&graven_schema, "namespace ts.collide.v1;\ntable Box { x @0: u32; }\n");
    update_ok(&graven_schema);
    let o = gravenc(&["gen", s(&graven_schema), "-o", s(&d.join("js-collide")), "--lang", "js"]);
    assert_ne!(code(&o), 0, "graven.graven must be refused under --lang js");
    assert!(stderr(&o).contains("graven.js"), "{}", stderr(&o));
}
