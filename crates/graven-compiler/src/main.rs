//! `gravenc`: CLI of the `.graven` schema compiler and its data tools.
//!
//! ```text
//! gravenc check  [--locked] <schema>...      validate schema(s) and compatibility with the lock
//! gravenc update <schema>... [--from <lock> | --init] apply the compatibility check and write <schema>.lock
//! gravenc gen    <schema> -o <dir>           generate <dir>/<file without .graven>.rs (like build.rs; creates <dir>)
//! gravenc diff   <schema> [--against <lock>] what changed since the lock, with each change's verdict
//! gravenc inspect <schema> <file> [--root T] [--raw]           decode messages for reading
//! gravenc to-json <schema> <file> [--root T] [--raw] [--pretty] decode messages as JSON
//! gravenc from-json <schema> --root T <in> -o <out> [--raw]   encode JSON as messages
//! gravenc scan   <file> [--schema S] [--truncate]           check a file of envelopes
//! ```
//!
//! Diagnostics go to stderr, one line `file:line:column: message` per problem
//! (`file:line:column: warning: message` for a confirmed warning, which does not fail).
//! Exit code: 0 ok, 1 error/finding, 2 usage error.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use graven_compiler::build::{
    Analysis, Build, SchemaIdEntry, analyze_set, lock_path, own_locks, schema_id_collisions, schema_ids, with_imports,
};
use graven_compiler::lock::{Lock, lock_from_ir, read_lock, write_lock};

mod tools;

const USAGE: &str = "\
usage:
  gravenc check  [--locked] <schema>...      validate schema(s) and compatibility with <schema>.lock
  gravenc update <schema>... [--from <lock> | --init]
                                             apply the compatibility check and write <schema>.lock
  gravenc gen    <schema> -o <dir> [--lang rust|js] [--side client|server|client,server]
                                             generate <dir>/<file without .graven>.rs, or .js + .d.ts (creates <dir>)
  gravenc diff   <schema> [--against <lock>] list what changed since the lock, with each change's verdict

  gravenc inspect   <schema> <file> [--root <Table>] [--raw]
                                             decode a file of envelopes (or one raw message) for reading
  gravenc to-json   <schema> <file> [--root <Table>] [--raw] [--pretty]
                                             decode messages as JSON, one document per message
  gravenc from-json <schema> --root <Table> <input.json|-> -o <out|-> [--raw]
                                             encode JSON documents as envelopes (or one raw message)
  gravenc scan   <file> [--schema <schema>] [--truncate]
                                             check a file of envelopes: intact records, incomplete tail,
                                             corruption; --truncate removes an incomplete tail

  --locked       (check) fail if <schema>.lock is missing or out of date (for CI)
  --from <lock>  (update, a single schema) compare against another lock instead of <schema>.lock
                 (e.g. v2.graven against v1.graven.lock, or a moved/renamed schema: the old
                 lock); the lock written is always <schema>.lock.
  --init         (update) create the first lock of a new schema, with no compatibility
                 history. Without `--init` or `--from`, `update` refuses a schema without
                 a lock: a deleted lock or a moved schema cannot reset the check.

exit: 0 ok, 1 error or blocking finding, 2 usage error.";

/// Execution failure (already reported on stderr).
struct Failed;

fn usage(msg: &str) -> ExitCode {
    eprintln!("gravenc: {msg}\n{USAGE}");
    ExitCode::from(2)
}

/// An argument: an option (`-x`, UTF-8) or a path (any `OsString`).
enum Arg<'a> {
    Flag(&'a str),
    Path(&'a OsStr),
}

/// Classifies `a`; an option that is not UTF-8 is a usage error (`Err`).
fn classify(a: &OsStr) -> Result<Arg<'_>, ExitCode> {
    match a.to_str() {
        Some(s) if s.starts_with('-') => Ok(Arg::Flag(s)),
        Some(_) => Ok(Arg::Path(a)),
        None if a.as_encoded_bytes().first() == Some(&b'-') => {
            Err(usage(&format!("invalid option (not UTF-8): {}", a.to_string_lossy())))
        }
        None => Ok(Arg::Path(a)),
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else { return usage("missing command") };
    let Some(cmd) = cmd.to_str() else {
        return usage(&format!("invalid command (not UTF-8): {}", cmd.to_string_lossy()));
    };
    let result = match cmd {
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        "check" => {
            let mut schemas = Vec::new();
            let mut locked = false;
            for a in rest {
                match classify(a) {
                    Err(code) => return code,
                    Ok(Arg::Flag("--locked")) => locked = true,
                    Ok(Arg::Flag(o)) => return usage(&format!("unknown option `{o}`")),
                    Ok(Arg::Path(p)) => schemas.push(PathBuf::from(p)),
                }
            }
            if schemas.is_empty() {
                return usage("check: missing schema");
            }
            check_all(&schemas, locked)
        }
        "update" => {
            let mut schemas = Vec::new();
            let mut from = None;
            let mut init = false;
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match classify(a) {
                    Err(code) => return code,
                    Ok(Arg::Flag("--from")) => match it.next() {
                        Some(l) if from.is_none() => from = Some(PathBuf::from(l)),
                        Some(_) => return usage("update: `--from` given twice"),
                        None => return usage("update: `--from` without the lock"),
                    },
                    Ok(Arg::Flag("--init")) => init = true,
                    Ok(Arg::Flag(o)) => return usage(&format!("unknown option `{o}`")),
                    Ok(Arg::Path(p)) => schemas.push(PathBuf::from(p)),
                }
            }
            if schemas.is_empty() {
                return usage("update: missing schema");
            }
            if from.is_some() && schemas.len() != 1 {
                return usage("update: `--from` applies to a single schema");
            }
            if init && from.is_some() {
                return usage("update: `--init` and `--from` are mutually exclusive");
            }
            run_all(&schemas, |s| update(s, from.as_deref(), init))
        }
        "gen" => {
            let mut schema = None;
            let mut out = None;
            let mut lang = graven_compiler::codegen::Lang::Rust;
            let mut opts = graven_compiler::codegen::GenOptions::default();
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match classify(a) {
                    Err(code) => return code,
                    Ok(Arg::Flag("-o")) => match it.next() {
                        Some(d) if out.is_none() => out = Some(PathBuf::from(d)),
                        Some(_) => return usage("gen: `-o` given twice"),
                        None => return usage("gen: `-o` without the directory"),
                    },
                    Ok(Arg::Flag("--lang")) => match it.next().and_then(|s| s.to_str()) {
                        Some("rust") => lang = graven_compiler::codegen::Lang::Rust,
                        Some("js") => lang = graven_compiler::codegen::Lang::Js,
                        Some("ts") => {
                            return usage("gen: `--lang ts` is now `--lang js`: it writes `.js` files with `.d.ts` declarations, for JavaScript and TypeScript projects");
                        }
                        Some(other) => return usage(&format!("gen: unknown `--lang` `{other}` (expected `rust` or `js`)")),
                        None => return usage("gen: `--lang` without a value (`rust` or `js`)"),
                    },
                    Ok(Arg::Flag("--side")) => match it.next().and_then(|s| s.to_str()) {
                        Some(val) => {
                            if let Err(msg) = parse_side(val, &mut opts) {
                                return usage(&msg);
                            }
                        }
                        None => return usage("gen: `--side` without a value (`client`, `server`, or `client,server`)"),
                    },
                    Ok(Arg::Flag(o)) => return usage(&format!("unknown option `{o}`")),
                    Ok(Arg::Path(p)) if schema.is_none() => schema = Some(PathBuf::from(p)),
                    Ok(Arg::Path(_)) => return usage("gen: a single schema only"),
                }
            }
            let (Some(schema), Some(out)) = (schema, out) else {
                return usage("gen: usage `gravenc gen <schema> -o <dir> [--lang rust|js] [--side client|server|client,server]`");
            };
            if lang == graven_compiler::codegen::Lang::Js && opts.server {
                eprintln!("gravenc: the JavaScript server edge is not available yet");
                return ExitCode::from(1);
            }
            generate(&schema, &out, lang, opts)
        }
        "diff" => {
            let mut schema = None;
            let mut against = None;
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match classify(a) {
                    Err(code) => return code,
                    Ok(Arg::Flag("--against")) => match it.next() {
                        Some(l) => against = Some(PathBuf::from(l)),
                        None => return usage("diff: `--against` without the lock"),
                    },
                    Ok(Arg::Flag(o)) => return usage(&format!("unknown option `{o}`")),
                    Ok(Arg::Path(p)) if schema.is_none() => schema = Some(PathBuf::from(p)),
                    Ok(Arg::Path(_)) => return usage("diff: a single schema only"),
                }
            }
            let Some(schema) = schema else { return usage("diff: missing schema") };
            tools::diff(&schema, against.as_deref())
        }
        "inspect" | "to-json" | "from-json" => {
            let mut paths = Vec::new();
            let (mut root, mut out, mut raw, mut pretty) = (None, None, false, false);
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match classify(a) {
                    Err(code) => return code,
                    Ok(Arg::Flag("--root")) => match it.next().and_then(|r| r.to_str()) {
                        Some(r) => root = Some(r.to_string()),
                        None => return usage(&format!("{cmd}: `--root` needs a table name")),
                    },
                    Ok(Arg::Flag("-o")) if cmd == "from-json" => match it.next() {
                        Some(o) => out = Some(PathBuf::from(o)),
                        None => return usage("from-json: `-o` without the output file"),
                    },
                    Ok(Arg::Flag("--raw")) => raw = true,
                    Ok(Arg::Flag("--pretty")) if cmd == "to-json" => pretty = true,
                    Ok(Arg::Flag("-")) => paths.push(PathBuf::from("-")),
                    Ok(Arg::Flag(o)) => return usage(&format!("unknown option `{o}`")),
                    Ok(Arg::Path(p)) => paths.push(PathBuf::from(p)),
                }
            }
            let [schema, file] = paths.as_slice() else {
                return usage(&format!("{cmd}: expected <schema> and an input file"));
            };
            match cmd {
                "inspect" => tools::inspect(schema, file, root.as_deref(), raw),
                "to-json" => tools::to_json(schema, file, root.as_deref(), raw, pretty),
                _ => {
                    let Some(root) = root else { return usage("from-json: `--root <Table>` is required") };
                    let Some(out) = out else { return usage("from-json: `-o <out|->` is required") };
                    tools::from_json(schema, file, &root, &out, raw)
                }
            }
        }
        "scan" => {
            let (mut file, mut schema, mut truncate) = (None, None, false);
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match classify(a) {
                    Err(code) => return code,
                    Ok(Arg::Flag("--schema")) => match it.next() {
                        Some(p) => schema = Some(PathBuf::from(p)),
                        None => return usage("scan: `--schema` without the schema"),
                    },
                    Ok(Arg::Flag("--truncate")) => truncate = true,
                    Ok(Arg::Flag(o)) => return usage(&format!("unknown option `{o}`")),
                    Ok(Arg::Path(p)) if file.is_none() => file = Some(PathBuf::from(p)),
                    Ok(Arg::Path(_)) => return usage("scan: a single file only"),
                }
            }
            let Some(file) = file else { return usage("scan: missing file") };
            tools::scan(&file, schema.as_deref(), truncate)
        }
        other => return usage(&format!("unknown command `{other}`")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failed) => ExitCode::from(1),
    }
}

/// Runs `f` on every schema (without stopping at the first error); fails if any failed.
fn run_all(schemas: &[PathBuf], f: impl Fn(&Path) -> Result<(), Failed>) -> Result<(), Failed> {
    let mut ok = true;
    for s in schemas {
        ok &= f(s).is_ok();
    }
    if ok { Ok(()) } else { Err(Failed) }
}

fn load_lock(path: &Path) -> Result<Option<Lock>, Failed> {
    read_lock(path).map_err(|e| {
        eprintln!("{e}");
        Failed
    })
}

/// The schemas plus the files of the namespaces they import, found under their directories.
fn resolve_set(schemas: &[PathBuf]) -> Result<Vec<PathBuf>, Failed> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for s in schemas {
        let d = s.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    with_imports(schemas, &dirs).map_err(|lines| {
        for l in lines {
            eprintln!("{l}");
        }
        Failed
    })
}

/// Prints the diagnostics or findings of one analysis. `Err` if something blocks.
fn report(schema: &Path, result: Result<Analysis, Vec<String>>) -> Result<Analysis, Failed> {
    let a = result.map_err(|lines| {
        for l in lines {
            eprintln!("{l}");
        }
        Failed
    })?;
    for l in a.finding_lines(schema) {
        eprintln!("{l}");
    }
    if a.blocking() { Err(Failed) } else { Ok(a) }
}

/// `check`: every schema given (with the namespaces they import), then schema id collisions.
fn check_all(schemas: &[PathBuf], locked: bool) -> Result<(), Failed> {
    let set = resolve_set(schemas)?;
    let results = analyze_set(&set, &own_locks);
    let mut ok = true;
    let mut ids = Vec::new();
    for (i, result) in results.into_iter().enumerate() {
        let schema = &set[i];
        let given = i < schemas.len();
        if !given {
            // An imported schema: its errors already show up in the files that import it.
            if let Ok(a) = &result
                && let Ok(Some(l)) = own_locks(schema)
            {
                ids.extend(schema_ids(schema, &a.ir, &l));
            }
            continue;
        }
        match check(schema, result, locked) {
            Ok(e) => ids.extend(e),
            Err(Failed) => ok = false,
        }
    }
    // Ids are compared across every schema involved (and within each one).
    for l in schema_id_collisions(&ids) {
        eprintln!("{l}");
        ok = false;
    }
    if ok { Ok(()) } else { Err(Failed) }
}

fn check(schema: &Path, result: Result<Analysis, Vec<String>>, locked: bool) -> Result<Vec<SchemaIdEntry>, Failed> {
    let lp = lock_path(schema);
    let lock = load_lock(&lp)?;
    let a = report(schema, result)?;
    // `--locked`: a missing or outdated lock is an error (as in the build).
    let level = if locked { "" } else { "note: " };
    let problem = match &lock {
        Some(l) if lock_from_ir(&a.ir, Some(l)) != *l => Some("lock out of date with respect to the schema"),
        None => Some("lock missing (new schema: `--init`; moved: `--from <old lock>`)"),
        _ => None,
    };
    if let Some(p) = problem {
        eprintln!("{}: {level}{p}; run: gravenc update {}", lp.display(), schema.display());
        if locked {
            return Err(Failed);
        }
    }
    println!("{}: ok", schema.display());
    let fresh = lock_from_ir(&a.ir, lock.as_ref());
    Ok(schema_ids(schema, &a.ir, lock.as_ref().unwrap_or(&fresh)))
}

fn update(schema: &Path, from: Option<&Path>, init: bool) -> Result<(), Failed> {
    let lp = lock_path(schema);
    let (current, baseline) = match from {
        Some(f) => {
            let base = load_lock(f)?.ok_or_else(|| {
                eprintln!("{}: `--from` lock not found", f.display());
                Failed
            })?;
            // With `--from`, the current lock only serves to say "unchanged": unreadable counts
            // as missing (it will be overwritten).
            (read_lock(&lp).ok().flatten(), Some(base))
        }
        None => {
            let current = load_lock(&lp)?;
            match (&current, init) {
                (None, false) => {
                    eprintln!(
                        "{}: lock missing; new schema: gravenc update --init {s}; moved/renamed schema: gravenc update {s} --from <old lock>",
                        lp.display(),
                        s = schema.display()
                    );
                    return Err(Failed);
                }
                (Some(_), true) => {
                    eprintln!("{}: `--init` with an existing lock; run without `--init`", lp.display());
                    return Err(Failed);
                }
                _ => {}
            }
            (current.clone(), current)
        }
    };
    let set = resolve_set(&[schema.to_path_buf()])?;
    let target = baseline.clone();
    let results = analyze_set(&set, &|f: &Path| if f == schema { Ok(target.clone()) } else { own_locks(f) });
    let a = report(schema, results.into_iter().next().expect("the schema itself"))?;
    let new = lock_from_ir(&a.ir, baseline.as_ref());
    if current.as_ref() == Some(&new) {
        println!("{}: unchanged", lp.display());
        return Ok(());
    }
    write_lock(&lp, &new).map_err(|e| {
        eprintln!("{}: could not write the lock: {e}", lp.display());
        Failed
    })?;
    println!("{}: written", lp.display());
    Ok(())
}

fn parse_side(val: &str, opts: &mut graven_compiler::codegen::GenOptions) -> Result<(), String> {
    if val.is_empty() {
        return Err("gen: empty `--side` value (expected `client`, `server`, or `client,server`)".into());
    }
    for part in val.split(',') {
        match part {
            "client" => opts.client = true,
            "server" => opts.server = true,
            other => return Err(format!("gen: invalid `--side` `{other}` (expected `client`, `server`, or `client,server`)")),
        }
    }
    Ok(())
}

fn generate(
    schema: &Path,
    out: &Path,
    lang: graven_compiler::codegen::Lang,
    opts: graven_compiler::codegen::GenOptions,
) -> Result<(), Failed> {
    std::fs::create_dir_all(out).map_err(|e| {
        eprintln!("{}: could not create the directory: {e}", out.display());
        Failed
    })?;
    // The schemas it imports are generated too: the generated code refers to them.
    let set = resolve_set(&[schema.to_path_buf()])?;
    let mut b = Build::new().quiet().out_dir(out).lang(lang);
    if opts.client {
        b = b.client();
    }
    if opts.server {
        b = b.server();
    }
    for s in &set {
        b = b.schema(s);
    }
    b.compile().map_err(|e| {
        eprintln!("{e}");
        Failed
    })?;
    let exts: &[&str] = match lang {
        graven_compiler::codegen::Lang::Rust => &["rs"],
        graven_compiler::codegen::Lang::Js => {
            println!("{}", out.join("graven.js").display());
            println!("{}", out.join("graven.d.ts").display());
            &["js", "d.ts"]
        }
    };
    for s in &set {
        let stem = s.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        for ext in exts {
            println!("{}", out.join(format!("{stem}.{ext}")).display());
        }
    }
    Ok(())
}
