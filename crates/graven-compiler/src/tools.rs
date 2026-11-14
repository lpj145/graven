//! Data tools: `inspect`, `to-json`, `from-json`, `scan` and `diff`.

use crate::Failed;
use graven::envelope::{self, MAGIC};
use graven::{Builder, Error, SimplePool};
use graven_compiler::build::{analyze_set, lock_path, own_locks, with_imports};
use graven_compiler::diff::diff as diff_locks;
use graven_compiler::dynamic::Schema;
use graven_compiler::ir::TableId;
use graven_compiler::json::{table_from_json, table_to_json};
use graven_compiler::lock::{lock_from_ir, read_lock};
use bytes::BytesMut;
use std::io::{Read, Write};
use std::path::Path;

fn fail(msg: impl std::fmt::Display) -> Failed {
    eprintln!("{msg}");
    Failed
}

fn load_schema(path: &Path) -> Result<Schema, Failed> {
    Schema::load(path).map_err(fail)
}

fn read_file(path: &Path) -> Result<Vec<u8>, Failed> {
    std::fs::read(path).map_err(|e| fail(format!("{}: could not read: {e}", path.display())))
}

fn root_table(s: &Schema, root: Option<&str>) -> Result<Option<TableId>, Failed> {
    match root {
        None => Ok(None),
        Some(r) => s.table(r).map(Some).ok_or_else(|| {
            let names: Vec<_> = s.ir.tables.iter().map(|t| t.name.as_str()).collect();
            fail(format!("{}: no table `{r}` (tables: {})", s.path.display(), names.join(", ")))
        }),
    }
}

/// One decoded message, or why it could not be decoded.
struct Record {
    offset: usize,
    len: usize,
    table: Option<TableId>,
    result: Result<serde_json::Value, String>,
    validation: Option<graven_dynamic::validate::BuiltinReport>,
}

fn explain(e: &Error) -> String {
    match e {
        Error::Incomplete { needed } => format!("Incomplete: the data ends early (needs {needed} bytes)"),
        Error::ChecksumMismatch => "ChecksumMismatch: the record is complete but its CRC does not match (corruption)".into(),
        Error::BadMagic => "BadMagic: not a Graven envelope".into(),
        other => format!("{other:?}"),
    }
}

/// Decodes `data`: a sequence of envelopes (detected by the magic) or, with `raw` or a
/// buffer that does not start with the magic, a single message of table `root`.
fn decode_all(s: &Schema, data: &[u8], root: Option<TableId>, raw: bool) -> Result<(Vec<Record>, Option<String>), Failed> {
    let mut out = Vec::new();
    let check_val = |t: TableId, v: &graven_dynamic::Value| {
        if s.validation().tables.is_empty() {
            None
        } else {
            Some(graven_dynamic::validate::Validator::validate_builtin(s.model(t), s.validation(), v))
        }
    };
    if raw || !data.starts_with(&MAGIC) {
        let Some(t) = root else {
            return Err(fail("the input is a raw message (no envelope): pass --root <Table>"));
        };
        let mut validation = None;
        let result = graven_dynamic::read(data, s.model(t))
            .map(|v| {
                validation = check_val(t, &v);
                table_to_json(&s.ir, t, &v)
            })
            .map_err(|e| explain(&e));
        out.push(Record { offset: 0, len: data.len(), table: Some(t), result, validation });
        return Ok((out, None));
    }
    let mut at = 0;
    while at < data.len() {
        match envelope::decode(&data[at..]) {
            Ok(env) => {
                let table = s.table_by_schema_id(env.schema_id);
                let mut validation = None;
                let result = match table {
                    Some(t) if root.is_none_or(|r| r == t) => graven_dynamic::read(env.payload, s.model(t))
                        .map(|v| {
                            validation = check_val(t, &v);
                            table_to_json(&s.ir, t, &v)
                        })
                        .map_err(|e| explain(&e)),
                    Some(t) => Err(format!("schema id is {}, not the --root table", s.qualified(t))),
                    None => Err(format!("unknown schema id {:#018x}: not a table of {}", env.schema_id, s.path.display())),
                };
                out.push(Record { offset: at, len: env.len(), table, result, validation });
                at += env.len();
            }
            Err(e) => return Ok((out, Some(format!("offset {at}: {}", explain(&e))))),
        }
    }
    Ok((out, None))
}

/// `gravenc inspect <schema> <file> [--root T] [--raw]`
pub fn inspect(schema: &Path, file: &Path, root: Option<&str>, raw: bool) -> Result<(), Failed> {
    let s = load_schema(schema)?;
    let root = root_table(&s, root)?;
    let data = read_file(file)?;
    let (records, stop) = decode_all(&s, &data, root, raw)?;
    let mut bad = false;
    for (i, r) in records.iter().enumerate() {
        let name = r.table.map(|t| s.qualified(t)).unwrap_or_else(|| "?".into());
        println!("#{i}  offset {}  {}  {} bytes", r.offset, name, r.len);
        match &r.result {
            Ok(j) => {
                println!("{}", serde_json::to_string_pretty(j).expect("JSON value"));
                if let Some(val) = &r.validation {
                    for inv in &val.invalid {
                        bad = true;
                        println!("  invalid: {inv}");
                    }
                    for name in &val.unchecked {
                        println!("  validation: check({name}) not checked");
                    }
                }
            }
            Err(e) => {
                bad = true;
                println!("  error: {e}");
            }
        }
    }
    if let Some(stop) = stop {
        println!("stopped at {stop}");
        bad = true;
    }
    if bad { Err(Failed) } else { Ok(()) }
}

/// `gravenc to-json <schema> <file> [--root T] [--raw] [--pretty]`: one JSON document per message.
pub fn to_json(schema: &Path, file: &Path, root: Option<&str>, raw: bool, pretty: bool) -> Result<(), Failed> {
    let s = load_schema(schema)?;
    let root = root_table(&s, root)?;
    let data = read_file(file)?;
    let (records, stop) = decode_all(&s, &data, root, raw)?;
    let mut stdout = std::io::stdout().lock();
    for r in &records {
        match &r.result {
            Ok(j) => {
                let text = if pretty { serde_json::to_string_pretty(j) } else { serde_json::to_string(j) };
                writeln!(stdout, "{}", text.expect("JSON value")).map_err(fail)?;
            }
            Err(e) => return Err(fail(format!("{}: offset {}: {e}", file.display(), r.offset))),
        }
    }
    match stop {
        Some(stop) => Err(fail(format!("{}: {stop}", file.display()))),
        None => Ok(()),
    }
}

/// `gravenc from-json <schema> --root T <input|-> -o <out|-> [--raw]`
pub fn from_json(schema: &Path, input: &Path, root: &str, out: &Path, raw: bool) -> Result<(), Failed> {
    let s = load_schema(schema)?;
    let t = root_table(&s, Some(root))?.expect("root given");
    let mut text = String::new();
    if input == Path::new("-") {
        std::io::stdin().read_to_string(&mut text).map_err(fail)?;
    } else {
        text = std::fs::read_to_string(input).map_err(|e| fail(format!("{}: could not read: {e}", input.display())))?;
    }
    let parsed: Vec<serde_json::Value> = serde_json::Deserializer::from_str(&text)
        .into_iter()
        .collect::<Result<_, _>>()
        .map_err(|e| fail(format!("{}: invalid JSON: {e}", input.display())))?;
    // A message always starts at a table, so a top-level array is read as a sequence of
    // messages, one per element (like JSON Lines).
    let docs: Vec<serde_json::Value> = parsed
        .into_iter()
        .flat_map(|d| match d {
            serde_json::Value::Array(items) => items,
            other => vec![other],
        })
        .collect();
    if raw && docs.len() != 1 {
        return Err(fail(format!("--raw writes a single message, but the input has {} documents", docs.len())));
    }
    let pool = SimplePool::new(64 * 1024);
    let mut bytes = BytesMut::new();
    for (i, d) in docs.iter().enumerate() {
        let v = table_from_json(&s.ir, t, d).map_err(|e| fail(format!("{}: document {i}: {e}", input.display())))?;
        let mut b = Builder::new(&pool);
        graven_dynamic::write(&v, s.model(t), &mut b).map_err(|e| fail(format!("document {i}: {e:?}")))?;
        let msg = b.finish().map_err(|e| fail(format!("document {i}: {e:?}")))?;
        if raw {
            bytes.extend_from_slice(&msg);
        } else {
            envelope::encode(s.schema_id(t), &msg, &mut bytes).map_err(|e| fail(format!("document {i}: {e:?}")))?;
        }
    }
    if out == Path::new("-") {
        std::io::stdout().lock().write_all(&bytes).map_err(fail)?;
    } else {
        std::fs::write(out, &bytes).map_err(|e| fail(format!("{}: could not write: {e}", out.display())))?;
        eprintln!("{}: {} message(s), {} bytes", out.display(), docs.len(), bytes.len());
    }
    Ok(())
}

/// `gravenc scan <file> [--schema S] [--truncate]`
pub fn scan(file: &Path, schema: Option<&Path>, truncate: bool) -> Result<(), Failed> {
    let s = match schema {
        Some(p) => Some(load_schema(p)?),
        None => None,
    };
    let data = read_file(file)?;
    let mut at = 0;
    let mut count = 0usize;
    let mut per_id: Vec<(u64, usize)> = Vec::new();
    let mut invalid: Vec<String> = Vec::new();
    let mut stop = None;
    while at < data.len() {
        match envelope::decode(&data[at..]) {
            Ok(env) => {
                count += 1;
                match per_id.iter_mut().find(|(id, _)| *id == env.schema_id) {
                    Some((_, n)) => *n += 1,
                    None => per_id.push((env.schema_id, 1)),
                }
                if let Some(s) = &s {
                    match s.table_by_schema_id(env.schema_id) {
                        Some(t) => {
                            if let Err(e) = graven_dynamic::read(env.payload, s.model(t)) {
                                invalid.push(format!("offset {at}: {} payload fails verification: {}", s.qualified(t), explain(&e)));
                            }
                        }
                        None => invalid.push(format!("offset {at}: unknown schema id {:#018x}", env.schema_id)),
                    }
                }
                at += env.len();
            }
            Err(e) => {
                stop = Some((at, e));
                break;
            }
        }
    }

    println!("{}: {count} record(s) intact, {at} bytes", file.display());
    for (id, n) in &per_id {
        let name = s.as_ref().and_then(|s| s.table_by_schema_id(*id).map(|t| s.qualified(t)));
        println!("  {:<32} {n}", name.unwrap_or_else(|| format!("{id:#018x}")));
    }
    let mut ok = invalid.is_empty();
    for l in &invalid {
        println!("  {l}");
    }

    match stop {
        None => println!("end of file reached cleanly"),
        Some((off, Error::Incomplete { .. })) => {
            let tail = data.len() - off;
            if truncate {
                let f = std::fs::OpenOptions::new()
                    .write(true)
                    .open(file)
                    .map_err(|e| fail(format!("{}: could not open for writing: {e}", file.display())))?;
                f.set_len(off as u64).map_err(|e| fail(format!("{}: could not truncate: {e}", file.display())))?;
                f.sync_all().map_err(|e| fail(format!("{}: could not sync: {e}", file.display())))?;
                println!("incomplete tail of {tail} bytes at offset {off} removed: the file now ends after the last intact record");
            } else {
                ok = false;
                println!(
                    "incomplete tail at offset {off}: {tail} bytes of a record that was cut short, which is what a crash during a write leaves behind.\n\
                     Every record before it is intact. Run with --truncate to cut the file at offset {off}."
                );
            }
        }
        Some((off, e)) => {
            ok = false;
            println!("corrupt data at offset {off}: {}.\nThe {count} record(s) before it are intact.", explain(&e));
            match resync(&data, off) {
                Some((next, after)) => println!(
                    "The next intact record starts at offset {next}; {after} intact record(s) follow from there \
                     ({} bytes in between cannot be read).",
                    next - off
                ),
                None => println!("No intact record was found after it."),
            }
            println!("--truncate only removes an incomplete tail, never corrupt data.");
            if truncate {
                eprintln!("{}: not truncated (corruption, not an incomplete tail)", file.display());
            }
        }
    }
    if ok { Ok(()) } else { Err(Failed) }
}

/// After corruption at `off`: the first later offset where an intact envelope starts (its
/// magic and CRC check out) and how many intact envelopes follow from there.
fn resync(data: &[u8], off: usize) -> Option<(usize, usize)> {
    let mut at = off + 1;
    while at + MAGIC.len() <= data.len() {
        let pos = data[at..].windows(MAGIC.len()).position(|w| w == MAGIC)? + at;
        if envelope::decode(&data[pos..]).is_ok() {
            let mut n = 0;
            let mut p = pos;
            while let Ok(env) = envelope::decode(&data[p..]) {
                n += 1;
                p += env.len();
            }
            return Some((pos, n));
        }
        at = pos + 1;
    }
    None
}

/// `gravenc diff <schema> [--against <lock>]`
pub fn diff(schema: &Path, against: Option<&Path>) -> Result<(), Failed> {
    let lp = against.map(Path::to_path_buf).unwrap_or_else(|| lock_path(schema));
    let base = read_lock(&lp)
        .map_err(fail)?
        .ok_or_else(|| fail(format!("{}: lock not found; nothing to compare against", lp.display())))?;
    let dir = schema.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    let set = with_imports(&[schema.to_path_buf()], &[dir]).map_err(|lines| fail(lines.join("\n")))?;
    let target = base.clone();
    let a = analyze_set(&set, &|f: &Path| if f == schema { Ok(Some(target.clone())) } else { own_locks(f) })
        .into_iter()
        .next()
        .expect("the schema itself")
        .map_err(|lines| fail(lines.join("\n")))?;
    let new = lock_from_ir(&a.ir, Some(&base));
    let report = diff_locks(&base, &new, &a.findings);

    println!("{} compared with {}", schema.display(), lp.display());
    if report.changes.is_empty() && report.other.is_empty() {
        println!("\nno changes");
        return Ok(());
    }
    let width = report.changes.iter().map(|c| c.what.chars().count()).max().unwrap_or(0);
    let mut scope = "";
    for c in &report.changes {
        if c.scope != scope {
            println!("\n{}", c.scope);
            scope = &c.scope;
        }
        println!("  {:<width$}   {}", c.what, c.verdict);
    }
    if !report.other.is_empty() {
        println!();
        for o in &report.other {
            println!("{o}");
        }
    }
    let errors = report.changes.iter().filter(|c| !c.verdict.starts_with("allowed") && !c.verdict.contains("(confirmed)")).count()
        + report.other.len();
    println!(
        "\n{} change(s): {} allowed, {} blocking",
        report.changes.len(),
        report.changes.iter().filter(|c| c.verdict.starts_with("allowed")).count(),
        if a.blocking() { errors.max(1) } else { 0 }
    );
    if a.blocking() { Err(Failed) } else { Ok(()) }
}
