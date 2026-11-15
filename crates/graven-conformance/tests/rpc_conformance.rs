//! RPC frame format v1 conformance vectors in `testvectors/rpc-v1/`.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use graven::rpc::{decode_fault, decode_frame, frames, FrameKind};
use graven_conformance::rpc_vectors::{ExpectedFrame, RpcExpect, format_frame_error, rpc_cases};

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

        match expect {
            RpcExpect::Frames(expected) => {
                let decoded: Vec<_> = frames(&disk_bytes)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap_or_else(|e| panic!("{}: unexpected error {e}", case.name));
                let actual: Vec<ExpectedFrame> = decoded
                    .iter()
                    .map(|f| {
                        let (fault_code, fault_message) = if f.header.kind == FrameKind::Fault {
                            let fault = decode_fault(f.payload).expect("valid fault payload");
                            (Some(fault.code.as_u16()), fault.message.map(str::to_string))
                        } else {
                            (None, None)
                        };
                        ExpectedFrame {
                            kind: f.header.kind.as_u8(),
                            method: f.header.method,
                            variant: f.header.variant,
                            service_id: format!("{:#018x}", f.header.service_id),
                            schema_id: format!("{:#018x}", f.schema_id),
                            payload_len: f.payload.len(),
                            fault_code,
                            fault_message,
                        }
                    })
                    .collect();
                assert_eq!(actual, expected, "mismatch for {}", case.name);
            }
            RpcExpect::Error(expected_err) => {
                let err = decode_frame(&disk_bytes)
                    .expect_err(&format!("{}: expected error `{expected_err}`", case.name));
                assert_eq!(format_frame_error(&err), expected_err, "mismatch for {}", case.name);
            }
        }
    }

    let on_disk: BTreeSet<String> = fs::read_dir(&dir)
        .expect("read testvectors/rpc-v1")
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(on_disk, expected_files, "orphan or missing files in testvectors/rpc-v1");
}
