//! Conformance vectors for RPC frames v1 (`testvectors/rpc-v1/` at the repo root).

use bytes::BytesMut;
use graven::envelope::schema_id;
use graven::rpc::{
    BrokenRule, Fault, FaultCode, Frame, FrameError, FrameHeader, FrameKind, decode_fault, encode_end,
    encode_fault_frame, encode_frame, frames,
};
use serde::{Deserialize, Serialize};

pub const TEST_SERVICE_ID: u64 = schema_id("conformance.v1.Service");

/// Expected outcome of decoding an RPC frame vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcExpect {
    /// One or more valid frames in `bytes`.
    Frames(Vec<ExpectedFrame>),
    /// The first frame that fails to decode: the frame error, or `Fault(<error>)` when a
    /// `Fault` frame's message is not a valid `graven.rpc.Fault`.
    Error(String),
}

/// One decoded frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedFrame {
    pub kind: u8,
    pub variant: u16,
    /// `Call` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<u16>,
    /// `Call` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_id: Option<String>,
    /// The message the frame carries, as lowercase hex (empty for `End`).
    pub message: String,
    /// `Fault` only: the decoded `graven.rpc.Fault`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault_code: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault_broken: Option<ExpectedBroken>,
}

/// Expected `graven.rpc.BrokenRule` of a decoded `Fault`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedBroken {
    pub table_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    pub rule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl From<&BrokenRule<'_>> for ExpectedBroken {
    fn from(b: &BrokenRule<'_>) -> Self {
        ExpectedBroken {
            table_name: b.table_name.to_string(),
            field: b.field.map(str::to_string),
            index: b.index,
            rule: b.rule.to_string(),
            min: b.min.map(str::to_string),
            max: b.max.map(str::to_string),
            reason: b.reason.map(str::to_string),
        }
    }
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

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// `Incomplete(n)`, `UnsupportedFrameVersion(v)`, `BadKind(k)` or `BadFrameHeader`.
pub fn format_frame_error(err: &FrameError) -> String {
    match err {
        FrameError::Incomplete { needed } => format!("Incomplete({needed})"),
        FrameError::UnsupportedFrameVersion(v) => format!("UnsupportedFrameVersion({v})"),
        FrameError::BadKind(k) => format!("BadKind({k})"),
        FrameError::BadFrameHeader => "BadFrameHeader".to_string(),
        _ => format!("{err:?}"),
    }
}

fn expected_frame(f: &Frame<'_>) -> Result<ExpectedFrame, String> {
    let call = f.header.kind == FrameKind::Call;
    let mut e = ExpectedFrame {
        kind: f.header.kind.as_u8(),
        variant: f.header.variant,
        method: call.then_some(f.header.method),
        service_id: call.then(|| format!("{:#018x}", f.header.service_id)),
        message: hex(f.payload),
        fault_code: None,
        fault_message: None,
        fault_broken: None,
    };
    if f.header.kind == FrameKind::Fault {
        let fault = decode_fault(f.payload).map_err(|err| format!("Fault({})", crate::vectors::error_name(&err)))?;
        e.fault_code = Some(fault.code.as_u16());
        e.fault_message = fault.message.map(str::to_string);
        e.fault_broken = fault.broken.as_ref().map(ExpectedBroken::from);
    }
    Ok(e)
}

/// What the reference implementation reads from `bytes`: every frame, or the first error.
pub fn decode_expect(bytes: &[u8]) -> RpcExpect {
    let mut out = Vec::new();
    for f in frames(bytes) {
        match f {
            Ok(f) => match expected_frame(&f) {
                Ok(e) => out.push(e),
                Err(msg) => return RpcExpect::Error(msg),
            },
            Err(e) => return RpcExpect::Error(format_frame_error(&e)),
        }
    }
    RpcExpect::Frames(out)
}

fn frame(header: FrameHeader, message: &[u8]) -> Vec<u8> {
    let mut out = BytesMut::new();
    encode_frame(header, message, &mut out).expect("encode_frame succeeds");
    out.to_vec()
}

fn fault_frame(fault: &Fault<'_>) -> Vec<u8> {
    let mut out = BytesMut::new();
    encode_fault_frame(fault, &mut out).expect("encode_fault_frame succeeds");
    out.to_vec()
}

fn end() -> Vec<u8> {
    let mut out = BytesMut::new();
    encode_end(&mut out);
    out.to_vec()
}

/// A valid case: the expectation is what the reference decodes, checked against `kinds`.
fn valid(name: &'static str, bytes: Vec<u8>, kinds: &[FrameKind]) -> RpcCase {
    let expect = decode_expect(&bytes);
    match &expect {
        RpcExpect::Frames(fs) => {
            let got: Vec<u8> = fs.iter().map(|f| f.kind).collect();
            let want: Vec<u8> = kinds.iter().map(|k| k.as_u8()).collect();
            assert_eq!(got, want, "{name}: unexpected frames");
        }
        RpcExpect::Error(e) => panic!("{name}: expected valid frames, got {e}"),
    }
    RpcCase { name, bytes, expect }
}

fn invalid(name: &'static str, bytes: Vec<u8>, error: String) -> RpcCase {
    assert_eq!(decode_expect(&bytes), RpcExpect::Error(error.clone()), "{name}");
    RpcCase { name, bytes, expect: RpcExpect::Error(error) }
}

pub fn rpc_cases() -> Vec<RpcCase> {
    use FrameKind as K;
    let reply = FrameHeader::reply;
    let call = frame(FrameHeader::call(TEST_SERVICE_ID, 1, 0), b"call-body");
    let item = frame(reply(K::Item, 99), b"item-body");
    let end = end();
    let invalid_fault = Fault {
        code: FaultCode::Invalid,
        message: None,
        broken: Some(BrokenRule {
            table_name: "AdjustStock",
            field: Some("delta"),
            index: Some(0),
            rule: "range",
            min: Some("-100"),
            max: Some("100"),
            reason: None,
        }),
    };
    let check_fault = Fault {
        code: FaultCode::Invalid,
        message: None,
        broken: Some(BrokenRule {
            table_name: "Signup",
            field: Some("email"),
            index: None,
            rule: "check(email)",
            min: None,
            max: None,
            reason: Some("missing_at"),
        }),
    };
    let future_fault = Fault { code: FaultCode::Unknown(99), message: Some("future fault"), broken: None };

    let with = |base: &[u8], at: usize, v: u8| {
        let mut b = base.to_vec();
        b[at] = v;
        b
    };

    vec![
        // Valid frames.
        valid("frame_call", call.clone(), &[K::Call]),
        valid("frame_call_union", frame(FrameHeader::call(TEST_SERVICE_ID, 7, 2), b"union-input"), &[K::Call]),
        valid("frame_ok", frame(reply(K::Ok, 0), b"ok-body"), &[K::Ok]),
        valid("frame_ok_empty_message", frame(reply(K::Ok, 0), b""), &[K::Ok]),
        valid("frame_err_union", frame(reply(K::Err, 2), b"err-body"), &[K::Err]),
        valid("frame_item_union_unknown_tag", item.clone(), &[K::Item]),
        valid("frame_end", end.clone(), &[K::End]),
        valid("frame_fault_invalid_range", fault_frame(&invalid_fault), &[K::Fault]),
        valid("frame_fault_invalid_check", fault_frame(&check_fault), &[K::Fault]),
        valid("frame_fault_future_code", fault_frame(&future_fault), &[K::Fault]),
        valid("frame_stream", [item.clone(), item.clone(), end.clone()].concat(), &[K::Item, K::Item, K::End]),
        valid("frame_stream_fault", [item.clone(), fault_frame(&future_fault)].concat(), &[K::Item, K::Fault]),
        // Invalid frames, in the normative check order.
        invalid("frame_short_header", call[..6].to_vec(), "Incomplete(7)".into()),
        // An older frame layout started with byte 0x01: version 0.
        invalid("frame_bad_version", with(&call, 0, 0x01), "UnsupportedFrameVersion(0)".into()),
        invalid("frame_bad_version_2", with(&with(&call, 0, 0x27), 6, 0xff), "UnsupportedFrameVersion(2)".into()),
        invalid("frame_bad_kind", with(&with(&call, 0, 0x17), 6, 0xff), "BadKind(7)".into()),
        invalid("frame_bad_kind_zero", with(&call, 0, 0x10), "BadKind(0)".into()),
        invalid("frame_len_too_large", with(&item, 6, 0x80), "BadFrameHeader".into()),
        invalid("frame_end_nonzero_variant", with(&end, 1, 1), "BadFrameHeader".into()),
        invalid("frame_end_nonzero_len", with(&end, 3, 1), "BadFrameHeader".into()),
        invalid("frame_cut_call_header", call[..12].to_vec(), format!("Incomplete({})", call.len())),
        invalid("frame_cut_message", item[..item.len() - 1].to_vec(), format!("Incomplete({})", item.len())),
        invalid("frame_fault_bad_message", frame(reply(K::Fault, 0), b"\x01"), "Fault(OutOfBounds)".into()),
    ]
}
