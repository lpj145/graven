//! Conformance vectors for RPC frame format v1 (`testvectors/rpc-v1/` at the repo root).

use bytes::BytesMut;
use graven::envelope::schema_id;
use graven::rpc::{
    BrokenRule, FAULT_SCHEMA_ID, Fault, FaultCode, FrameError, FrameHeader, FrameKind, decode_fault,
    encode_end, encode_fault_frame, encode_frame,
};
use serde::{Deserialize, Serialize};

pub const TEST_SERVICE_ID: u64 = schema_id("conformance.v1.Service");
const SAMPLE_SCHEMA_ID: u64 = schema_id("conformance.v1.Sample");

/// Expected outcome of decoding an RPC frame vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcExpect {
    /// One or more valid frames in `bytes`.
    Frames(Vec<ExpectedFrame>),
    /// Single-frame decode failure at the start of `bytes`.
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedFrame {
    pub kind: u8,
    pub method: u16,
    pub variant: u16,
    pub service_id: String,
    pub schema_id: String,
    pub payload_len: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RpcCase {
    pub name: &'static str,
    pub bytes: Vec<u8>,
    pub expect: RpcExpect,
}

impl RpcCase {
    pub fn json_text(&self) -> String {
        let mut s = serde_json::to_string_pretty(&self.expect).expect("RpcExpect serializes");
        s.push('\n');
        s
    }
}

fn hex_u64(v: u64) -> String {
    format!("{v:#018x}")
}

fn frame_bytes(header: FrameHeader, schema: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = BytesMut::new();
    encode_frame(header, schema, payload, &mut out).expect("encode_frame succeeds");
    out.to_vec()
}

pub fn format_frame_error(err: &FrameError) -> String {
    match err {
        FrameError::Incomplete { needed } => format!("Incomplete({needed})"),
        FrameError::UnsupportedFrameVersion(v) => format!("UnsupportedFrameVersion({v})"),
        FrameError::BadKind(k) => format!("BadKind({k})"),
        FrameError::BadFrameHeader => "BadFrameHeader".to_string(),
        FrameError::Envelope(e) => format!("Envelope({e:?})"),
        _ => format!("{err:?}"),
    }
}

pub fn rpc_cases() -> Vec<RpcCase> {
    let mut cases = Vec::new();

    // 1. Call
    let call_hdr = FrameHeader {
        kind: FrameKind::Call,
        method: 1,
        variant: 0,
        service_id: TEST_SERVICE_ID,
    };
    let call_bytes = frame_bytes(call_hdr, SAMPLE_SCHEMA_ID, b"call-body");
    cases.push(RpcCase {
        name: "frame_call",
        bytes: call_bytes.clone(),
        expect: RpcExpect::Frames(vec![ExpectedFrame {
            kind: 1,
            method: 1,
            variant: 0,
            service_id: hex_u64(TEST_SERVICE_ID),
            schema_id: hex_u64(SAMPLE_SCHEMA_ID),
            payload_len: 9,
            fault_code: None,
            fault_message: None,
        }]),
    });

    // 2. Ok
    let ok_hdr = FrameHeader {
        kind: FrameKind::Ok,
        method: 1,
        variant: 0,
        service_id: TEST_SERVICE_ID,
    };
    cases.push(RpcCase {
        name: "frame_ok",
        bytes: frame_bytes(ok_hdr, SAMPLE_SCHEMA_ID, b"ok-body"),
        expect: RpcExpect::Frames(vec![ExpectedFrame {
            kind: 2,
            method: 1,
            variant: 0,
            service_id: hex_u64(TEST_SERVICE_ID),
            schema_id: hex_u64(SAMPLE_SCHEMA_ID),
            payload_len: 7,
            fault_code: None,
            fault_message: None,
        }]),
    });

    // 3. Err with union variant
    let err_hdr = FrameHeader {
        kind: FrameKind::Err,
        method: 3,
        variant: 2,
        service_id: TEST_SERVICE_ID,
    };
    cases.push(RpcCase {
        name: "frame_err_union",
        bytes: frame_bytes(err_hdr, SAMPLE_SCHEMA_ID, b"err-body"),
        expect: RpcExpect::Frames(vec![ExpectedFrame {
            kind: 3,
            method: 3,
            variant: 2,
            service_id: hex_u64(TEST_SERVICE_ID),
            schema_id: hex_u64(SAMPLE_SCHEMA_ID),
            payload_len: 8,
            fault_code: None,
            fault_message: None,
        }]),
    });

    // 4. Item with unknown union variant tag
    let item_hdr = FrameHeader {
        kind: FrameKind::Item,
        method: 2,
        variant: 99,
        service_id: TEST_SERVICE_ID,
    };
    let item_bytes = frame_bytes(item_hdr, SAMPLE_SCHEMA_ID, b"item-body");
    cases.push(RpcCase {
        name: "frame_item_union_unknown_tag",
        bytes: item_bytes.clone(),
        expect: RpcExpect::Frames(vec![ExpectedFrame {
            kind: 4,
            method: 2,
            variant: 99,
            service_id: hex_u64(TEST_SERVICE_ID),
            schema_id: hex_u64(SAMPLE_SCHEMA_ID),
            payload_len: 9,
            fault_code: None,
            fault_message: None,
        }]),
    });

    // 5. End
    let mut end_buf = BytesMut::new();
    encode_end(TEST_SERVICE_ID, 2, &mut end_buf);
    let end_bytes = end_buf.to_vec();
    cases.push(RpcCase {
        name: "frame_end",
        bytes: end_bytes.clone(),
        expect: RpcExpect::Frames(vec![ExpectedFrame {
            kind: 5,
            method: 2,
            variant: 0,
            service_id: hex_u64(TEST_SERVICE_ID),
            schema_id: hex_u64(0),
            payload_len: 0,
            fault_code: None,
            fault_message: None,
        }]),
    });

    // 6. Fault Invalid with BrokenRule
    let fault = Fault {
        code: FaultCode::Invalid,
        message: Some("validation failed"),
        broken: Some(BrokenRule {
            table_name: "AdjustStock",
            field: Some("delta"),
            index: Some(0),
            rule: "range",
        }),
    };
    let mut fault_buf = BytesMut::new();
    encode_fault_frame(TEST_SERVICE_ID, 3, &fault, &mut fault_buf).expect("encode_fault_frame");
    let fault_frame = graven::rpc::decode_frame(&fault_buf).expect("decode fault frame");
    let fault_dec = decode_fault(fault_frame.payload).expect("decode fault payload");
    assert_eq!(fault_dec, fault);
    cases.push(RpcCase {
        name: "frame_fault_invalid",
        bytes: fault_buf.to_vec(),
        expect: RpcExpect::Frames(vec![ExpectedFrame {
            kind: 6,
            method: 3,
            variant: 0,
            service_id: hex_u64(TEST_SERVICE_ID),
            schema_id: hex_u64(FAULT_SCHEMA_ID),
            payload_len: fault_frame.payload.len(),
            fault_code: Some(3),
            fault_message: Some("validation failed".to_string()),
        }]),
    });

    // 7. Fault with unknown future FaultCode
    let future_fault = Fault {
        code: FaultCode::Unknown(99),
        message: Some("future fault"),
        broken: None,
    };
    let mut future_buf = BytesMut::new();
    encode_fault_frame(TEST_SERVICE_ID, 1, &future_fault, &mut future_buf).expect("encode future");
    let future_frame = graven::rpc::decode_frame(&future_buf).expect("decode future");
    cases.push(RpcCase {
        name: "frame_fault_future_code",
        bytes: future_buf.to_vec(),
        expect: RpcExpect::Frames(vec![ExpectedFrame {
            kind: 6,
            method: 1,
            variant: 0,
            service_id: hex_u64(TEST_SERVICE_ID),
            schema_id: hex_u64(FAULT_SCHEMA_ID),
            payload_len: future_frame.payload.len(),
            fault_code: Some(99),
            fault_message: Some("future fault".to_string()),
        }]),
    });

    // 8. Multi-frame stream (Item + End)
    let mut multi = item_bytes.clone();
    multi.extend_from_slice(&end_bytes);
    cases.push(RpcCase {
        name: "frame_multi",
        bytes: multi,
        expect: RpcExpect::Frames(vec![
            ExpectedFrame {
                kind: 4,
                method: 2,
                variant: 99,
                service_id: hex_u64(TEST_SERVICE_ID),
                schema_id: hex_u64(SAMPLE_SCHEMA_ID),
                payload_len: 9,
                fault_code: None,
                fault_message: None,
            },
            ExpectedFrame {
                kind: 5,
                method: 2,
                variant: 0,
                service_id: hex_u64(TEST_SERVICE_ID),
                schema_id: hex_u64(0),
                payload_len: 0,
                fault_code: None,
                fault_message: None,
            },
        ]),
    });

    // 9. Short header (15 bytes) -> Incomplete(16)
    cases.push(RpcCase {
        name: "frame_short_header",
        bytes: call_bytes[..15].to_vec(),
        expect: RpcExpect::Error("Incomplete(16)".to_string()),
    });

    // 10. Unsupported version (precedence over bad kind and non-zero reserved)
    let mut bad_ver = call_bytes.clone();
    bad_ver[0] = 2;
    bad_ver[1] = 99;
    bad_ver[6] = 1;
    cases.push(RpcCase {
        name: "frame_bad_version",
        bytes: bad_ver,
        expect: RpcExpect::Error("UnsupportedFrameVersion(2)".to_string()),
    });

    // 11. Bad kind (precedence over non-zero reserved)
    let mut bad_kind = call_bytes.clone();
    bad_kind[1] = 7;
    bad_kind[6] = 1;
    cases.push(RpcCase {
        name: "frame_bad_kind",
        bytes: bad_kind,
        expect: RpcExpect::Error("BadKind(7)".to_string()),
    });

    // 12. Non-zero reserved (precedence over bad body magic)
    let mut bad_res = call_bytes.clone();
    bad_res[6] = 1;
    bad_res[16] = b'X';
    cases.push(RpcCase {
        name: "frame_bad_reserved",
        bytes: bad_res,
        expect: RpcExpect::Error("BadFrameHeader".to_string()),
    });

    // 13. End with non-zero variant -> BadFrameHeader
    let mut bad_end = end_bytes.clone();
    bad_end[4] = 1;
    cases.push(RpcCase {
        name: "frame_end_nonzero_variant",
        bytes: bad_end,
        expect: RpcExpect::Error("BadFrameHeader".to_string()),
    });

    // 14. Cut envelope header (20 bytes) -> Incomplete(36)
    cases.push(RpcCase {
        name: "frame_cut_envelope",
        bytes: call_bytes[..20].to_vec(),
        expect: RpcExpect::Error("Incomplete(36)".to_string()),
    });

    // 15. Cut body payload/crc (1 byte short) -> Incomplete(call_bytes.len())
    cases.push(RpcCase {
        name: "frame_cut_body_payload",
        bytes: call_bytes[..call_bytes.len() - 1].to_vec(),
        expect: RpcExpect::Error(format!("Incomplete({})", call_bytes.len())),
    });

    // 16. Bad envelope magic -> Envelope(BadMagic)
    let mut bad_magic = call_bytes.clone();
    bad_magic[16] = b'X';
    cases.push(RpcCase {
        name: "frame_bad_magic",
        bytes: bad_magic,
        expect: RpcExpect::Error("Envelope(BadMagic)".to_string()),
    });

    // 17. Bitflip in envelope payload -> Envelope(ChecksumMismatch)
    let mut bad_crc = call_bytes.clone();
    bad_crc[36] ^= 1;
    cases.push(RpcCase {
        name: "frame_bad_crc",
        bytes: bad_crc,
        expect: RpcExpect::Error("Envelope(ChecksumMismatch)".to_string()),
    });

    cases
}
