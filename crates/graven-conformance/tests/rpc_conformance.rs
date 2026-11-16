//! RPC frames v1 conformance vectors in `testvectors/rpc-v1/`.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use graven_conformance::rpc_vectors::{RpcExpect, decode_expect, rpc_cases};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testvectors/rpc-v1")
}

#[test]
fn rpc_vectors_match_spec_and_disk() {
    let dir = dir();
    let cases = rpc_cases();
    assert!(!cases.is_empty());

    let mut expected_files = BTreeSet::new();
    expected_files.insert("README.md".to_string());

    for case in &cases {
        let bin_name = format!("{}.bin", case.name);
        let json_name = format!("{}.json", case.name);
        expected_files.insert(bin_name.clone());
        expected_files.insert(json_name.clone());

        let disk_bytes = fs::read(dir.join(&bin_name))
            .unwrap_or_else(|e| panic!("missing {bin_name}: {e}"));
        let disk_json = fs::read_to_string(dir.join(&json_name))
            .unwrap_or_else(|e| panic!("missing {json_name}: {e}"));
        assert_eq!(disk_bytes, case.bytes, "{bin_name} differs from generator");

        let expect: RpcExpect = serde_json::from_str(&disk_json)
            .unwrap_or_else(|e| panic!("invalid json in {json_name}: {e}"));
        assert_eq!(expect, case.expect, "{json_name} differs from generator");

        assert_eq!(decode_expect(&disk_bytes), expect, "{} decodes differently", case.name);
    }

    let on_disk: BTreeSet<String> = fs::read_dir(&dir)
        .expect("read testvectors/rpc-v1")
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(on_disk, expected_files, "orphan or missing files in testvectors/rpc-v1");
}
