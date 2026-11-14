//! Frozen conformance vectors in `testvectors/v1/` (repo root).
//!
//! `vectors_read_as_expected` and `no_orphan_files` are driven by the files on disk, so
//! they keep checking the frozen bytes even if `cases()` changes.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use graven::envelope;
use graven::Error;
use graven_conformance::vectors::{CaseJson, ExpectJson, cases};
use graven_conformance::{TableDef, Value, conformance_root, conformance_root_v2, read};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testvectors/v1")
}

/// Every vector name the plan requires (the brief's minimum list).
const REQUIRED: &[&str] = &[
    "minimal",
    "all_fields",
    "zero_is_present",
    "empty_lists",
    "streaming_leaves",
    "unknown_enum",
    "unknown_union_tag",
    "v2_read_by_v1",
    "v1_read_by_v2",
    "reserved_ignored",
    "enveloped_all_fields",
    "truncated_slot",
    "bad_offset",
    "string_len_oob",
    "bad_utf8",
    "missing_required",
    "self_cycle",
    "budget_amplification",
    "env_bad_magic",
    "env_bad_version",
    "env_bad_header",
    "env_cut_header",
    "env_cut_payload",
    "env_cut_crc",
    "env_bitflip_payload",
    "env_bitflip_crc",
    "env_wrong_schema",
    // §10.3 (Plano 3)
    "bool_struct_and_lists",
    "strlist_null_elem",
    "union_tag_null_ptr_optional",
    "union_tag_null_ptr_required",
    "union_unknown_tag_bad_ptr",
    "reserved_slot_partial",
    "ptr_slot_partial",
    "zero_size_slot_at_end",
    "table_list_elem_wider",
    "table_list_elem_narrower",
    "table_list_elem_zero",
    "env_concat",
    "env_payload_len_short",
    "env_payload_len_long",
    "env_payload_len_short_inplace",
];

/// Reader schema by qualified name (the JSON `schema` field).
fn schema(name: &str) -> &'static TableDef {
    match name {
        "conformance.v1.Root" => conformance_root(),
        "conformance.v1.RootV2" => conformance_root_v2(),
        "conformance.v1.Extra" => graven_conformance::conformance_extra(),
        "conformance.v1.Holder" => graven_conformance::conformance_holder(),
        other => panic!("unknown reader schema `{other}`"),
    }
}

/// Dynamic equivalent of a generated `from_bytes`: envelope (if any) → schema id check →
/// full read + verify.
fn evaluate(bytes: &[u8], schema_name: &str, enveloped: bool) -> Result<Value, Error> {
    let def = schema(schema_name);
    if !enveloped {
        return read(bytes, def);
    }
    let env = envelope::decode(bytes)?;
    let expected = envelope::schema_id(schema_name);
    if env.schema_id != expected {
        return Err(Error::SchemaMismatch { expected, found: env.schema_id });
    }
    read(env.payload, def)
}

/// `Missing { field: "a_u8" }` ⇒ `Missing`.
fn variant_name(e: &Error) -> String {
    let s = format!("{e:?}");
    s.split(['{', '(']).next().unwrap().trim().to_string()
}

fn files_on_disk() -> BTreeSet<String> {
    let Ok(rd) = fs::read_dir(dir()) else { return BTreeSet::new() };
    rd.map(|e| e.unwrap().file_name().into_string().unwrap()).collect()
}

#[test]
fn required_vectors_present() {
    let files = files_on_disk();
    let missing: Vec<_> = REQUIRED
        .iter()
        .filter(|n| !files.contains(&format!("{n}.bin")) || !files.contains(&format!("{n}.json")))
        .collect();
    assert!(missing.is_empty(), "vectors missing from {}: {missing:?}", dir().display());
    let names: BTreeSet<_> = cases().iter().map(|c| c.name).collect();
    for n in REQUIRED {
        assert!(names.contains(n), "cases() lacks `{n}`");
    }
}

#[test]
fn vectors_match_disk() {
    for c in cases() {
        let bin = fs::read(dir().join(format!("{}.bin", c.name)))
            .unwrap_or_else(|e| panic!("{}.bin: {e}", c.name));
        assert!(bin == c.bytes, "{}.bin differs from cases() (writer no longer deterministic/compatible)", c.name);
        let json = fs::read_to_string(dir().join(format!("{}.json", c.name)))
            .unwrap_or_else(|e| panic!("{}.json: {e}", c.name));
        let on_disk: CaseJson = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{}.json: {e}", c.name));
        assert_eq!(on_disk, c.json(), "{}.json", c.name);
    }
}

#[test]
fn vectors_read_as_expected() {
    let files = files_on_disk();
    let bins: Vec<_> = files.iter().filter_map(|f| f.strip_suffix(".bin")).collect();
    assert!(!bins.is_empty(), "no vectors in {}", dir().display());
    for name in bins {
        let bytes = fs::read(dir().join(format!("{name}.bin"))).unwrap();
        let json = fs::read_to_string(dir().join(format!("{name}.json"))).unwrap();
        let j: CaseJson = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{name}.json: {e}"));
        let got = evaluate(&bytes, &j.schema, j.enveloped);
        match (&j.expect, got) {
            (ExpectJson::Value(want), Ok(v)) => assert_eq!(&v, want, "{name}: value"),
            (ExpectJson::Error(want), Err(e)) => assert_eq!(&variant_name(&e), want, "{name}: error ({e:?})"),
            (want, got) => panic!("{name}: expected {want:?}, got {got:?}"),
        }
    }
}

#[test]
fn no_orphan_files() {
    let files = files_on_disk();
    let mut expected: BTreeSet<String> = ["README.md".to_string()].into();
    for c in cases() {
        expected.insert(format!("{}.bin", c.name));
        expected.insert(format!("{}.json", c.name));
    }
    let orphans: Vec<_> = files.difference(&expected).collect();
    let absent: Vec<_> = expected.difference(&files).collect();
    assert!(orphans.is_empty(), "files without a Case: {orphans:?}");
    assert!(absent.is_empty(), "Cases without files: {absent:?}");
}

/// The §10.3 vectors added later: they exist in `cases()` and on disk, and the total
/// is the 27 original frozen vectors + 15.
#[test]
fn new_vectors_present() {
    const NEW: [&str; 15] = [
        "bool_struct_and_lists",
        "strlist_null_elem",
        "union_tag_null_ptr_optional",
        "union_tag_null_ptr_required",
        "union_unknown_tag_bad_ptr",
        "reserved_slot_partial",
        "ptr_slot_partial",
        "zero_size_slot_at_end",
        "table_list_elem_wider",
        "table_list_elem_narrower",
        "table_list_elem_zero",
        "env_concat",
        "env_payload_len_short",
        "env_payload_len_long",
        "env_payload_len_short_inplace",
    ];
    let cs = graven_conformance::vectors::cases();
    assert_eq!(cs.len(), 27 + NEW.len());
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testvectors/v1");
    for n in NEW {
        assert!(cs.iter().any(|c| c.name == n), "{n} fora de cases()");
        assert!(dir.join(format!("{n}.bin")).exists() && dir.join(format!("{n}.json")).exists(), "{n} fora do disco");
    }
}
