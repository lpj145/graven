//! Compatibility corpus (spec §9): each directory `tests/compat/<case>/` has
//! `old.graven`, `new.graven` and `expect.txt`. The test compiles `old.graven` → lock, checks
//! `new.graven` against that lock and compares the findings with `expect.txt`:
//!
//! - `ok`: no findings;
//! - one line per finding, in the order of [`compat`]:
//!   `error <rule> <Type>[.<field>]` or `warning <rule> <Type>[.<field>] confirmed|unconfirmed`;
//! - `error check <excerpt>`: checking the new schema against the lock itself fails, with
//!   a diagnostic whose message contains `<excerpt>`.
//!
//! Chained case: with `mid.graven`, the path is `old` → `mid` → `new`, each step as
//! `gravenc update` does it (lock written to and reread from disk; `mid` may not have a blocking
//! finding) and `expect.txt` applies to the last step. This catches two-step bypasses that
//! a pairwise comparison does not see.

use std::path::Path;

use graven_compiler::compat::{Finding, blocking, compat};
use graven_compiler::lock::{Lock, lock_from_ir, read_lock, reserved_types, write_lock};
use graven_compiler::*;

fn compile_old(src: &str) -> Ir {
    let s = parse(src).unwrap_or_else(|e| panic!("parse old: {e}"));
    check(&s, &|_: &str, _: u16| None).unwrap_or_else(|d| panic!("check old: {d:?}"))
}

fn check_new(src: &str, old: &Lock) -> Result<Ir, Vec<Diag>> {
    let s = parse(src).unwrap_or_else(|e| panic!("parse new: {e}"));
    check(&s, &reserved_types(old))
}

fn line(f: &Finding) -> String {
    let (kind, rule, ty, field, conf) = match f {
        Finding::Error { rule, table, field, .. } => ("error", rule, table, field, ""),
        Finding::Warning { rule, table, field, confirmed, .. } => {
            ("warning", rule, table, field, if *confirmed { " confirmed" } else { " unconfirmed" })
        }
    };
    match field {
        Some(fd) => format!("{kind} {rule} {ty}.{fd}{conf}"),
        None => format!("{kind} {rule} {ty}{conf}"),
    }
}

fn run_case(dir: &Path) -> Result<(), String> {
    let read = |n: &str| std::fs::read_to_string(dir.join(n)).unwrap_or_else(|e| panic!("{}/{n}: {e}", dir.display()));
    let tmp = std::env::temp_dir().join(format!("graven-compat-{}-{}", std::process::id(), dir.file_name().unwrap().to_string_lossy()));
    let roundtrip = |l: &Lock| -> Lock {
        write_lock(&tmp, l).unwrap();
        let back = read_lock(&tmp).unwrap().unwrap();
        std::fs::remove_file(&tmp).unwrap();
        back
    };
    let mut old_lock = roundtrip(&lock_from_ir(&compile_old(&read("old.graven")), None));
    if dir.join("mid.graven").exists() {
        let mid = check_new(&read("mid.graven"), &old_lock).map_err(|d| format!("check do mid.graven falhou: {d:?}"))?;
        let f = compat(&old_lock, &mid);
        if f.iter().any(Finding::is_blocking) {
            return Err(format!("passo old → mid bloqueado: {:?}", f.iter().map(line).collect::<Vec<_>>()));
        }
        old_lock = roundtrip(&lock_from_ir(&mid, Some(&old_lock)));
    }
    let expect: Vec<String> = read("expect.txt").lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    let got: Vec<String> = match check_new(&read("new.graven"), &old_lock) {
        Err(diags) => {
            let Some(needle) = expect.first().and_then(|l| l.strip_prefix("error check ")) else {
                return Err(format!("check do new.graven falhou: {diags:?}"));
            };
            if expect.len() == 1 && diags.iter().any(|d| d.msg.contains(needle)) {
                return Ok(());
            }
            return Err(format!("expected a diagnostic with {needle:?}, got {diags:?}"));
        }
        Ok(new) => {
            let findings = compat(&old_lock, &new);
            // Determinism: two runs give the same list.
            assert_eq!(findings, compat(&old_lock, &new));
            // Every message names the rule.
            for f in &findings {
                assert!(f.msg().contains(f.rule()), "message without the rule: {f}");
            }
            let lines: Vec<String> = findings.iter().map(line).collect();
            if lines.is_empty() { vec!["ok".to_string()] } else { lines }
        }
    };
    if got == expect { Ok(()) } else { Err(format!("esperado {expect:?}, veio {got:?}")) }
}

#[test]
fn compat_corpus() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat");
    let mut cases: Vec<_> = std::fs::read_dir(&root).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    cases.sort();
    assert!(cases.len() >= 40, "corpus encolheu: {}", cases.len());
    let mut failures = Vec::new();
    for c in &cases {
        if let Err(e) = run_case(c) {
            failures.push(format!("{}: {e}", c.file_name().unwrap().to_string_lossy()));
        }
    }
    assert!(failures.is_empty(), "{} of {} cases failed:\n{}", failures.len(), cases.len(), failures.join("\n"));
}

/// Minimum coverage: every error/warning rule appears in some `expect.txt`, and
/// every allowed change has an `ok` case named after it.
#[test]
fn corpus_covers_every_rule() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat");
    let mut expects = String::new();
    for e in std::fs::read_dir(&root).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            expects += &std::fs::read_to_string(p.join("expect.txt")).unwrap();
        }
    }
    for rule in graven_compiler::compat::ERROR_RULES.iter().chain(&graven_compiler::compat::WARNING_RULES) {
        if *rule == "layout-changed" {
            continue; // defensive; covered by `layout_changed_is_defensive`
        }
        assert!(expects.contains(&format!(" {rule} ")), "rule {rule} has no case in the corpus");
    }
    assert!(expects.contains("confirmed\n") && expects.contains("unconfirmed\n"));
    for allowed in graven_compiler::compat::ALLOWED_RULES {
        let dir = root.join(allowed.replace('-', "_"));
        assert_eq!(std::fs::read_to_string(dir.join("expect.txt")).unwrap().trim(), "ok", "permitido {allowed}");
    }
}

/// `bank_v1_v2/old.graven` is the real schema; the corpus v2 has the layout of the `evo/bank_v2.graven` fixture.
#[test]
fn bank_corpus_matches_real_schema_and_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let real = std::fs::read_to_string(root.join("../graven-schemas/schemas/bank.graven")).unwrap();
    let v1 = std::fs::read_to_string(root.join("tests/compat/bank_v1_v2/old.graven")).unwrap();
    assert_eq!(v1, real, "bank_v1_v2/old.graven diverged from graven-schemas/schemas/bank.graven");

    use graven_schemas::evo::bank_v2::transaction as tx;
    let v1_lock = lock_from_ir(&compile_old(&real), None);
    for case in ["bank_v1_v2/new.graven", "bank_v2_v3/old.graven"] {
        let src = std::fs::read_to_string(root.join("tests/compat").join(case)).unwrap();
        let ir = check_new(&src, &v1_lock).unwrap_or_else(|d| panic!("{case}: {d:?}"));
        let l = lock_from_ir(&ir, Some(&v1_lock));
        let t = l.tables.iter().find(|t| t.name == "Transaction").unwrap();
        assert_eq!(t.fixed_len, tx::FIXED_LEN, "{case}");
        assert_eq!(t.fields[7].slot, Some(tx::FEE), "{case}");
        assert_eq!(t.fields[7].bit, Some((tx::FEE_BIT.byte, tx::FEE_BIT.shift)), "{case}");
        assert_eq!(t.fields[6].presence, "reserved");
        assert_eq!(t.schema_id, format!("{:#018x}", graven_schemas::evo::bank_v2::Transaction::SCHEMA_ID));
    }
}

#[test]
fn layout_changed_is_defensive() {
    let src = "namespace t; table T { a @0: u32; b @1: u8?; }";
    let ir = compile_old(src);
    let mut old = lock_from_ir(&ir, None);
    assert!(compat(&old, &ir).is_empty());
    // Hand-edited lock (or a changed layout algorithm): the slot of `b` does not match.
    old.tables[0].fields[1].slot = Some(9);
    let f = compat(&old, &ir);
    assert_eq!(f.iter().map(line).collect::<Vec<_>>(), ["error layout-changed T.b"]);
}

#[test]
fn blocking_and_display() {
    let old = lock_from_ir(&compile_old("namespace t; table T { a @0: u32; b @1: u32; }"), None);
    let new = check_new("namespace t; table T { a @0: u32; @1: reserved; }", &old).unwrap();
    let f = compat(&old, &new);
    assert!(blocking(&f), "an unconfirmed warning blocks");
    let text = f[0].to_string();
    assert!(text.starts_with("warning [required-reserved]"), "{text}");
    assert!(text.contains("allow_breaking_readers"), "{text}");
    // The position is the field's in the new schema.
    assert_eq!(f[0].span(), Span { line: 1, col: 35 });

    let new = check_new("namespace t; table T { a @0: u32; #[allow_breaking_readers] @1: reserved; }", &old).unwrap();
    let f = compat(&old, &new);
    assert_eq!(f.len(), 1);
    assert!(!blocking(&f), "a confirmed warning does not block");

    let new = check_new("namespace t; table T { a @0: u64; b @1: u32; }", &old).unwrap();
    let f = compat(&old, &new);
    assert!(blocking(&f));
    assert!(f[0].to_string().starts_with("error [type-changed]"), "{}", f[0]);
    assert!(f[0].is_error());
}
