//! Writes the frozen conformance vectors to `testvectors/v1/` (repo root).
//!
//! `cargo run -p graven-conformance --bin gen-vectors [-- --check | --force]`
//!
//! - default: writes missing files; refuses (exit 1, nothing written) if an existing file
//!   would change, because frozen vectors never change.
//! - `--check`: writes nothing; exit 1 if any file is missing or differs.
//! - `--force`: overwrites differing files (only while a format version is not frozen).
//!
//! Files in the directory without a case (other than `README.md`) are reported as orphans
//! (exit 1); they are never deleted.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use graven_conformance::vectors::cases;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let force = args.iter().any(|a| a == "--force");
    let check = args.iter().any(|a| a == "--check");
    if let Some(bad) = args.iter().find(|a| *a != "--force" && *a != "--check") {
        eprintln!("gen-vectors: unknown argument `{bad}` (use --check or --force)");
        return ExitCode::from(2);
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testvectors/v1");

    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for c in cases() {
        files.push((format!("{}.bin", c.name), c.bytes.clone()));
        files.push((format!("{}.json", c.name), c.json_text().into_bytes()));
    }

    let (mut new, mut changed, mut same) = (Vec::new(), Vec::new(), 0usize);
    for (name, data) in &files {
        match fs::read(dir.join(name)) {
            Ok(old) if old == *data => same += 1,
            Ok(_) => changed.push(name.as_str()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => new.push(name.as_str()),
            Err(e) => {
                eprintln!("gen-vectors: cannot read {}: {e}", dir.join(name).display());
                return ExitCode::FAILURE;
            }
        }
    }
    let expected: BTreeSet<&str> = files.iter().map(|(n, _)| n.as_str()).chain(["README.md"]).collect();
    let orphans: Vec<String> = fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter(|n| !expected.contains(n.as_str()))
                .collect()
        })
        .unwrap_or_default();

    for n in &changed {
        eprintln!("DIFFERS: {n}");
    }
    for n in &orphans {
        eprintln!("ORPHAN (no case): {n}");
    }

    if check {
        for n in &new {
            eprintln!("MISSING: {n}");
        }
        println!("gen-vectors --check: {same} identical, {} missing, {} different, {} orphans", new.len(), changed.len(), orphans.len());
        return if new.is_empty() && changed.is_empty() && orphans.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    if !changed.is_empty() && !force {
        eprintln!("gen-vectors: {} frozen file(s) would change; refusing (a frozen vector never changes; use --force only for an unfrozen format)", changed.len());
        return ExitCode::FAILURE;
    }

    if !changed.is_empty() {
        eprintln!("################################################################");
        eprintln!("WARNING: --force is OVERWRITING {} existing vector file(s).", changed.len());
        eprintln!("v1 vectors are FROZEN: changing them means format v2, not a fix.");
        eprintln!("################################################################");
    }
    fs::create_dir_all(&dir).expect("create testvectors/v1");
    let mut written = 0usize;
    for (name, data) in &files {
        if new.contains(&name.as_str()) || changed.contains(&name.as_str()) {
            write(&dir.join(name), data);
            written += 1;
        }
    }
    println!(
        "gen-vectors: {written} files changed ({} new, {} overwritten, {same} unchanged) in {}",
        new.len(),
        changed.len(),
        dir.display()
    );
    if orphans.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn write(path: &Path, data: &[u8]) {
    fs::write(path, data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
