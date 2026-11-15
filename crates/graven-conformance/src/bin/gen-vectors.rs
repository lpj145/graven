//! Writes and checks conformance vectors in `testvectors/v1/` and `testvectors/rpc-v1/`.
//!
//! `cargo run -p graven-conformance --bin gen-vectors [-- --check | --force]`
//!
//! - default: writes missing files; refuses (exit 1, nothing written) if an existing file
//!   would change, because frozen vectors never change.
//! - `--check`: writes nothing; exit 1 if any file is missing or differs.
//! - `--force`: overwrites differing files (only while a format version is not frozen).
//!
//! Files in either directory without a case (other than `README.md`) are reported as
//! orphans (exit 1); they are never deleted.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use graven_conformance::rpc_vectors::rpc_cases;
use graven_conformance::vectors::cases;

fn process_dir(
    label: &str,
    dir: &Path,
    files: Vec<(String, Vec<u8>)>,
    check: bool,
    force: bool,
) -> ExitCode {
    let (mut new, mut changed, mut same) = (Vec::new(), Vec::new(), 0usize);
    for (name, data) in &files {
        match fs::read(dir.join(name)) {
            Ok(old) if old == *data => same += 1,
            Ok(_) => changed.push(name.as_str()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => new.push(name.as_str()),
            Err(e) => {
                eprintln!("gen-vectors ({label}): cannot read {}: {e}", dir.join(name).display());
                return ExitCode::FAILURE;
            }
        }
    }
    let expected: BTreeSet<&str> = files.iter().map(|(n, _)| n.as_str()).chain(["README.md"]).collect();
    let orphans: Vec<String> = fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter(|n| !expected.contains(n.as_str()))
                .collect()
        })
        .unwrap_or_default();

    for n in &changed {
        eprintln!("DIFFERS ({label}): {n}");
    }
    for n in &orphans {
        eprintln!("ORPHAN ({label}, no case): {n}");
    }

    if check {
        for n in &new {
            eprintln!("MISSING ({label}): {n}");
        }
        println!(
            "gen-vectors --check ({label}): {same} identical, {} missing, {} different, {} orphans",
            new.len(),
            changed.len(),
            orphans.len()
        );
        return if new.is_empty() && changed.is_empty() && orphans.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    if !changed.is_empty() && !force {
        eprintln!(
            "gen-vectors ({label}): {} frozen file(s) would change; refusing (use --force only for an unfrozen format)",
            changed.len()
        );
        return ExitCode::FAILURE;
    }

    if !changed.is_empty() {
        eprintln!("################################################################");
        eprintln!("WARNING: --force is OVERWRITING {} existing vector file(s) in {label}.", changed.len());
        eprintln!("################################################################");
    }
    fs::create_dir_all(dir).expect("create testvectors directory");
    let mut written = 0usize;
    for (name, data) in &files {
        if new.contains(&name.as_str()) || changed.contains(&name.as_str()) {
            write(&dir.join(name), data);
            written += 1;
        }
    }
    println!(
        "gen-vectors ({label}): {written} files changed ({} new, {} overwritten, {same} unchanged) in {}",
        new.len(),
        changed.len(),
        dir.display()
    );
    if orphans.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let force = args.iter().any(|a| a == "--force");
    let check = args.iter().any(|a| a == "--check");
    if let Some(bad) = args.iter().find(|a| *a != "--force" && *a != "--check") {
        eprintln!("gen-vectors: unknown argument `{bad}` (use --check or --force)");
        return ExitCode::from(2);
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");

    let mut v1_files = Vec::new();
    for c in cases() {
        v1_files.push((format!("{}.bin", c.name), c.bytes.clone()));
        v1_files.push((format!("{}.json", c.name), c.json_text().into_bytes()));
    }
    if process_dir("v1", &root.join("testvectors/v1"), v1_files, check, force) != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }

    let mut rpc_files = Vec::new();
    for c in rpc_cases() {
        rpc_files.push((format!("{}.bin", c.name), c.bytes.clone()));
        rpc_files.push((format!("{}.json", c.name), c.json_text().into_bytes()));
    }
    process_dir("rpc-v1", &root.join("testvectors/rpc-v1"), rpc_files, check, force)
}

fn write(path: &Path, data: &[u8]) {
    fs::write(path, data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
