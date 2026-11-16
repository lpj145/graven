//! Graven RPC frame v1 encoder, decoder, and protocol fault helpers (`docs/spec/graven-rpc-v1.md`).
//!
//! Frame layout (16-byte little-endian header followed by a format v1 envelope, except `End`):
//!
//! ```text
//! offset  size  field
//! 0       1     frame_version = 1
//! 1       1     kind (1=Call, 2=Ok, 3=Err, 4=Item, 5=End, 6=Fault)
//! 2       2     method (@n)
//! 4       2     variant (union tag if body type is union, 0 otherwise)
//! 6       2     reserved = 0
//! 8       8     service_id
//! 16      ...   body (one envelope, or empty for End)
//! ```

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use bytes::{BufMut, Bytes};
pub use bytes::BytesMut;
use core::fmt;
use core::future::Future;
use core::iter::FusedIterator;
use core::marker::PhantomData;
use core::ops::Range;

use crate::envelope::{self, schema_id};
use crate::{Bit, BufPool, Builder, Error, TableRef, ValidateError, VerifyCtx};

/// RPC frame format version implemented by this module.
pub const FRAME_VERSION: u8 = 1;
/// Size of the fixed RPC frame header in bytes.
pub const HEADER_LEN: usize = 16;

/// Schema id of `graven.rpc.BrokenRule`.
pub const BROKEN_RULE_SCHEMA_ID: u64 = schema_id("graven.rpc.BrokenRule");
/// Schema id of `graven.rpc.Fault`.
pub const FAULT_SCHEMA_ID: u64 = schema_id("graven.rpc.Fault");

/// Identifies the role of an RPC frame.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Call = 1,
    Ok = 2,
    Err = 3,
    Item = 4,
    End = 5,
    Fault = 6,
}

impl FrameKind {
    /// Parses a wire `kind` byte (`1..=6`).
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(FrameKind::Call),
            2 => Some(FrameKind::Ok),
            3 => Some(FrameKind::Err),
            4 => Some(FrameKind::Item),
            5 => Some(FrameKind::End),
            6 => Some(FrameKind::Fault),
            _ => None,
        }
    }

    /// Returns the wire `u8` representation.
    #[inline]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for FrameKind {
    type Error = FrameError;

    #[inline]
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::from_u8(value).ok_or(FrameError::BadKind(value))
    }
}

/// The 16-byte header at the start of every RPC frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub kind: FrameKind,
    pub method: u16,
    pub variant: u16,
    pub service_id: u64,
}

/// A decoded RPC frame borrowed from the input buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    pub header: FrameHeader,
    /// Envelope `schema_id`, or `0` for `FrameKind::End`.
    pub schema_id: u64,
    /// Raw Graven message bytes inside the envelope, or empty for `FrameKind::End`.
    pub payload: &'a [u8],
    len: usize,
}

impl Frame<'_> {
    /// Total bytes occupied by this frame in the input (`16` for `End`, or `16 + 24 + payload.len()`).
    #[allow(clippy::len_without_is_empty)]
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }
}

/// Failure modes when decoding an RPC frame header or its envelope body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameError {
    /// More input bytes are needed to finish the frame (`needed` is total bytes from frame start).
    Incomplete { needed: usize },
    /// Frame `frame_version` byte is not `1`.
    UnsupportedFrameVersion(u8),
    /// Frame `kind` byte is not in `1..=6`.
    BadKind(u8),
    /// Non-zero reserved bytes or non-zero `variant` on an `End` frame.
    BadFrameHeader,
    /// Failure while decoding the envelope body (`BadMagic`, `UnsupportedVersion`, `BadHeader`, `ChecksumMismatch`).
    Envelope(Error),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::Incomplete { needed } => {
                write!(f, "incomplete rpc frame: need at least {needed} bytes")
            }
            FrameError::UnsupportedFrameVersion(v) => {
                write!(f, "unsupported rpc frame version {v}")
            }
            FrameError::BadKind(k) => write!(f, "unknown rpc frame kind {k}"),
            FrameError::BadFrameHeader => f.write_str("malformed rpc frame header"),
            FrameError::Envelope(e) => write!(f, "rpc frame body envelope error: {e}"),
        }
    }
}

impl core::error::Error for FrameError {}

/// Serializes `header` into its 16-byte little-endian representation.
#[inline]
pub fn header_bytes(header: FrameHeader) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[0] = FRAME_VERSION;
    h[1] = header.kind.as_u8();
    h[2..4].copy_from_slice(&header.method.to_le_bytes());
    h[4..6].copy_from_slice(&header.variant.to_le_bytes());
    // h[6..8] reserved = 0
    h[8..16].copy_from_slice(&header.service_id.to_le_bytes());
    h
}

/// Decodes and validates a single RPC frame at the start of `buf`.
///
/// Follows the normative check order in `docs/spec/graven-rpc-v1.md` §4:
/// 1. `buf.len() < 16` -> `Incomplete { needed: 16 }`
/// 2. `buf[0] != 1` -> `UnsupportedFrameVersion(buf[0])`
/// 3. `buf[1]` not in `1..=6` -> `BadKind(buf[1])`
/// 4. `reserved != 0` or `(kind == End && variant != 0)` -> `BadFrameHeader`
/// 5. For `End`, returns immediately with `len = 16`. Otherwise decodes `&buf[16..]` as an envelope.
///
/// Trailing bytes after `frame.len()` belong to subsequent frames and are ignored.
pub fn decode_frame(buf: &[u8]) -> Result<Frame<'_>, FrameError> {
    if buf.len() < HEADER_LEN {
        return Err(FrameError::Incomplete { needed: HEADER_LEN });
    }
    if buf[0] != FRAME_VERSION {
        return Err(FrameError::UnsupportedFrameVersion(buf[0]));
    }
    let kind = FrameKind::from_u8(buf[1]).ok_or(FrameError::BadKind(buf[1]))?;
    let method = u16::from_le_bytes(buf[2..4].try_into().unwrap());
    let variant = u16::from_le_bytes(buf[4..6].try_into().unwrap());
    let reserved = u16::from_le_bytes(buf[6..8].try_into().unwrap());
    let service_id = u64::from_le_bytes(buf[8..16].try_into().unwrap());

    if reserved != 0 || (kind == FrameKind::End && variant != 0) {
        return Err(FrameError::BadFrameHeader);
    }

    let header = FrameHeader { kind, method, variant, service_id };
    if kind == FrameKind::End {
        return Ok(Frame { header, schema_id: 0, payload: &[], len: HEADER_LEN });
    }

    match envelope::decode(&buf[HEADER_LEN..]) {
        Ok(env) => Ok(Frame {
            header,
            schema_id: env.schema_id,
            payload: env.payload,
            len: HEADER_LEN + env.len(),
        }),
        Err(Error::Incomplete { needed }) => {
            Err(FrameError::Incomplete { needed: HEADER_LEN.saturating_add(needed) })
        }
        Err(e) => Err(FrameError::Envelope(e)),
    }
}

/// Appends one RPC frame to `out`.
///
/// For `FrameKind::End`, `header.variant` must be `0` and `payload` must be empty, otherwise
/// `Error::BadHeader` is returned and `out` is untouched.
pub fn encode_frame(
    header: FrameHeader,
    schema_id: u64,
    payload: &[u8],
    out: &mut BytesMut,
) -> Result<(), Error> {
    if header.kind == FrameKind::End {
        if header.variant != 0 || !payload.is_empty() {
            return Err(Error::BadHeader);
        }
        out.put_slice(&header_bytes(header));
        return Ok(());
    }
    if payload.len() as u64 > i32::MAX as u64 {
        return Err(Error::TooLarge);
    }
    out.reserve(HEADER_LEN + envelope::HEADER_LEN + payload.len() + envelope::TRAILER_LEN);
    out.put_slice(&header_bytes(header));
    envelope::encode(schema_id, payload, out)
}

/// Appends an `End` frame (16 bytes, no body) to `out`.
#[inline]
pub fn encode_end(service_id: u64, method: u16, out: &mut BytesMut) {
    out.put_slice(&header_bytes(FrameHeader {
        kind: FrameKind::End,
        method,
        variant: 0,
        service_id,
    }));
}

/// Iterates over RPC frames concatenated in `buf`.
///
/// Yields `Err` once if decoding fails, after which iteration terminates.
#[inline]
pub fn frames(buf: &[u8]) -> Frames<'_> {
    Frames { buf, failed: false }
}

/// Iterator returned by [`frames`].
#[derive(Debug, Clone)]
pub struct Frames<'a> {
    buf: &'a [u8],
    failed: bool,
}

impl<'a> Iterator for Frames<'a> {
    type Item = Result<Frame<'a>, FrameError>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.buf.is_empty() || self.failed {
            return None;
        }
        match decode_frame(self.buf) {
            Ok(frame) => {
                self.buf = &self.buf[frame.len..];
                Some(Ok(frame))
            }
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}

impl FusedIterator for Frames<'_> {}

/// Standard fault codes in `graven.rpc.FaultCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FaultCode {
    Unsupported,
    Malformed,
    Invalid,
    DeadlineExceeded,
    Internal,
    Unavailable,
    /// Unrecognized future fault code (`u16`); applications SHOULD handle unknown codes as `Internal`.
    Unknown(u16),
}

impl FaultCode {
    /// Decodes a wire `u16` fault code. Unknown values map to `FaultCode::Unknown(v)`.
    #[inline]
    pub const fn from_u16(v: u16) -> Self {
        match v {
            1 => FaultCode::Unsupported,
            2 => FaultCode::Malformed,
            3 => FaultCode::Invalid,
            4 => FaultCode::DeadlineExceeded,
            5 => FaultCode::Internal,
            6 => FaultCode::Unavailable,
            other => FaultCode::Unknown(other),
        }
    }

    /// Returns the wire `u16` value.
    #[inline]
    pub const fn as_u16(self) -> u16 {
        match self {
            FaultCode::Unsupported => 1,
            FaultCode::Malformed => 2,
            FaultCode::Invalid => 3,
            FaultCode::DeadlineExceeded => 4,
            FaultCode::Internal => 5,
            FaultCode::Unavailable => 6,
            FaultCode::Unknown(v) => v,
        }
    }

    /// Normalizes unrecognized codes to [`FaultCode::Internal`], per the spec.
    #[inline]
    pub const fn normalized(self) -> Self {
        match self {
            FaultCode::Unknown(_) => FaultCode::Internal,
            other => other,
        }
    }
}

/// Validation failure details carried by `graven.rpc.BrokenRule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrokenRule<'a> {
    pub table_name: &'a str,
    pub field: Option<&'a str>,
    pub index: Option<u32>,
    /// `"range"`, `"len"`, `"finite"`, `"known"` or `"check(name)"`.
    pub rule: &'a str,
    /// `range`/`len`: the declared lower bound, as a decimal number.
    pub min: Option<&'a str>,
    /// `range`/`len`: the declared upper bound, as a decimal number.
    pub max: Option<&'a str>,
    /// `check(name)`: the code the check returned.
    pub reason: Option<&'a str>,
}

/// Protocol fault payload corresponding to `graven.rpc.Fault`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault<'a> {
    pub code: FaultCode,
    pub message: Option<&'a str>,
    pub broken: Option<BrokenRule<'a>>,
}

const FAULT_FIXED_LEN: u16 = 11;
/// Fixed part of `BrokenRule` up to `rule` (@3): the layout of the first RPC frames v1
/// release, written when `min`, `max` and `reason` are absent so those bytes stay unchanged.
const BROKEN_RULE_FIXED_LEN_V0: u16 = 17;
/// Fixed part of `BrokenRule` with `min`, `max` and `reason` (@4–@6).
const BROKEN_RULE_FIXED_LEN: u16 = 29;

/// Zero-state buffer pool for one-shot message encoding under `no_std`.
#[doc(hidden)]
pub struct NoPool;

impl BufPool for NoPool {
    fn take(&self) -> BytesMut {
        BytesMut::new()
    }
    fn give(&self, _b: BytesMut) {}
}

fn verify_and_read_str<'a>(t: &TableRef<'a>, off: u16, ctx: &mut VerifyCtx) -> Result<Option<&'a str>, Error> {
    match t.str(off)? {
        Some(s) => {
            ctx.charge(4 + s.len())?;
            Ok(Some(s))
        }
        None => Ok(None),
    }
}

/// Verifies and decodes a `graven.rpc.Fault` message payload.
pub fn decode_fault(payload: &[u8]) -> Result<Fault<'_>, Error> {
    let mut ctx = VerifyCtx::new(payload.len());
    ctx.enter()?;
    let t = TableRef::at(payload, 0)?;
    ctx.charge(2 + t.fixed_len() as usize)?;

    let raw_code = t
        .scalar::<u16>(1, Bit { byte: 0, shift: 0 })?
        .ok_or(Error::Missing { field: "code" })?;
    let message = verify_and_read_str(&t, 3, &mut ctx)?;
    let broken = match t.table(7)? {
        Some(b) => {
            ctx.enter()?;
            ctx.charge(2 + b.fixed_len() as usize)?;
            let table_name = verify_and_read_str(&b, 0, &mut ctx)?
                .ok_or(Error::Missing { field: "table_name" })?;
            let field = verify_and_read_str(&b, 4, &mut ctx)?;
            let index = b.scalar::<u32>(9, Bit { byte: 8, shift: 0 })?;
            let rule = verify_and_read_str(&b, 13, &mut ctx)?
                .ok_or(Error::Missing { field: "rule" })?;
            let min = verify_and_read_str(&b, 17, &mut ctx)?;
            let max = verify_and_read_str(&b, 21, &mut ctx)?;
            let reason = verify_and_read_str(&b, 25, &mut ctx)?;
            ctx.leave();
            Some(BrokenRule { table_name, field, index, rule, min, max, reason })
        }
        None => None,
    };
    ctx.leave();

    Ok(Fault {
        code: FaultCode::from_u16(raw_code),
        message,
        broken,
    })
}

/// Serializes a `graven.rpc.Fault` payload (raw Graven message, without envelope or frame header).
pub fn encode_fault_payload(fault: &Fault<'_>) -> Result<Vec<u8>, Error> {
    let pool = NoPool;
    let mut b = Builder::new(&pool);
    {
        let mut w = b.root(FAULT_FIXED_LEN);
        w.set::<u16>(1, Bit { byte: 0, shift: 0 }, fault.code.as_u16());
        if let Some(msg) = fault.message {
            w.set_str(3, msg);
        }
        if let Some(br) = fault.broken {
            let extended = br.min.is_some() || br.max.is_some() || br.reason.is_some();
            let len = if extended { BROKEN_RULE_FIXED_LEN } else { BROKEN_RULE_FIXED_LEN_V0 };
            let mut bw = w.table(7, len);
            bw.set_str(0, br.table_name);
            if let Some(f) = br.field {
                bw.set_str(4, f);
            }
            if let Some(idx) = br.index {
                bw.set::<u32>(9, Bit { byte: 8, shift: 0 }, idx);
            }
            bw.set_str(13, br.rule);
            for (off, v) in [(17, br.min), (21, br.max), (25, br.reason)] {
                if let Some(v) = v {
                    bw.set_str(off, v);
                }
            }
        }
    }
    Ok(b.finished()?.to_vec())
}

/// Appends a `Fault` RPC frame encoding `fault` to `out`.
pub fn encode_fault_frame(
    service_id: u64,
    method: u16,
    fault: &Fault<'_>,
    out: &mut BytesMut,
) -> Result<(), Error> {
    let payload = encode_fault_payload(fault)?;
    encode_frame(
        FrameHeader {
            kind: FrameKind::Fault,
            method,
            variant: 0,
            service_id,
        },
        FAULT_SCHEMA_ID,
        &payload,
        out,
    )
}

/// Builds a complete `Fault` RPC frame as owned bytes.
pub fn fault_frame_bytes(service_id: u64, method: u16, fault: &Fault<'_>) -> Vec<u8> {
    let mut out = BytesMut::new();
    encode_fault_frame(service_id, method, fault, &mut out)
        .expect("fault payload always fits in i32::MAX bytes");
    out.to_vec()
}

/// Formats a [`crate::Invalid`] rule into its `graven.rpc.BrokenRule.rule` wire string
/// (`"range"`, `"len"`, `"finite"`, `"known"`, or `"check(name)"`).
pub fn format_rule(rule: crate::Rule) -> String {
    format!("{rule}")
}

/// Owned copy of [`BrokenRule`] for `'static` error values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedBrokenRule {
    pub table_name: String,
    pub field: Option<String>,
    pub index: Option<u32>,
    pub rule: String,
    pub min: Option<String>,
    pub max: Option<String>,
    pub reason: Option<String>,
}

/// Owned copy of [`Fault`] for `'static` error values (such as [`CallError::Fault`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedFault {
    pub code: FaultCode,
    pub message: Option<String>,
    pub broken: Option<Box<OwnedBrokenRule>>,
}

impl Fault<'_> {
    /// Clones borrowed fault strings into an owned [`OwnedFault`].
    pub fn to_owned(&self) -> OwnedFault {
        OwnedFault {
            code: self.code,
            message: self.message.map(ToString::to_string),
            broken: self.broken.map(|b| Box::new(OwnedBrokenRule {
                table_name: b.table_name.to_string(),
                field: b.field.map(ToString::to_string),
                index: b.index,
                rule: b.rule.to_string(),
                min: b.min.map(ToString::to_string),
                max: b.max.map(ToString::to_string),
                reason: b.reason.map(ToString::to_string),
            })),
        }
    }
}

impl OwnedFault {
    /// Views this owned fault as a borrowed [`Fault`].
    pub fn as_borrowed(&self) -> Fault<'_> {
        Fault {
            code: self.code,
            message: self.message.as_deref(),
            broken: self.broken.as_ref().map(|b| BrokenRule {
                table_name: &b.table_name,
                field: b.field.as_deref(),
                index: b.index,
                rule: &b.rule,
                min: b.min.as_deref(),
                max: b.max.as_deref(),
                reason: b.reason.as_deref(),
            }),
        }
    }
}

impl fmt::Display for OwnedFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rpc fault {:?}", self.code)?;
        if let Some(msg) = &self.message {
            write!(f, ": {msg}")?;
        }
        if let Some(b) = &self.broken {
            write!(f, ": {}", b.table_name)?;
            if let Some(field) = &b.field {
                write!(f, ".{field}")?;
            }
            if let Some(i) = b.index {
                write!(f, "[{i}]")?;
            }
            write!(f, " breaks `{}`", b.rule)?;
            for (k, v) in [("min", &b.min), ("max", &b.max), ("reason", &b.reason)] {
                if let Some(v) = v {
                    write!(f, " {k}={v}")?;
                }
            }
        }
        Ok(())
    }
}

impl core::error::Error for OwnedFault {}

/// Failure when validating or decoding a reply at the client side.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProtocolError {
    Frame(FrameError),
    Payload(Error),
    EmptyReply,
    UnexpectedKind(FrameKind),
    ServiceMismatch { expected: u64, found: u64 },
    MethodMismatch { expected: u16, found: u16 },
    SchemaMismatch { expected: u64, found: u64 },
    MissingVariant,
    UnexpectedVariant(u16),
    TrailingBytes(usize),
    UnexpectedOneWayReply,
    /// The reply ended before its `End` or `Fault` frame.
    Truncated,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::Frame(e) => write!(f, "frame decode error: {e}"),
            ProtocolError::Payload(e) => write!(f, "payload decode error: {e}"),
            ProtocolError::EmptyReply => f.write_str("empty reply where a frame was expected"),
            ProtocolError::UnexpectedKind(k) => write!(f, "unexpected frame kind `{k:?}`"),
            ProtocolError::ServiceMismatch { expected, found } => {
                write!(f, "reply service_id mismatch: expected {expected:#018x}, found {found:#018x}")
            }
            ProtocolError::MethodMismatch { expected, found } => {
                write!(f, "reply method mismatch: expected @{expected}, found @{found}")
            }
            ProtocolError::SchemaMismatch { expected, found } => {
                write!(f, "reply schema_id mismatch: expected {expected:#018x}, found {found:#018x}")
            }
            ProtocolError::MissingVariant => f.write_str("expected union variant in frame header (found 0)"),
            ProtocolError::UnexpectedVariant(v) => write!(f, "unexpected non-zero union variant {v} for table body"),
            ProtocolError::TrailingBytes(n) => write!(f, "unexpected {n} trailing byte(s) after terminal frame"),
            ProtocolError::UnexpectedOneWayReply => f.write_str("one-way method received non-empty reply bytes"),
            ProtocolError::Truncated => f.write_str("stream reply ended before its End or Fault frame"),
        }
    }
}

impl core::error::Error for ProtocolError {}

/// Failure encountered while reading a [`StreamResponse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamError<S = ()> {
    /// The server ended the stream with a protocol fault.
    Fault(OwnedFault),
    /// The reply violated the RPC protocol or failed verification.
    Protocol(ProtocolError),
    /// The application's [`ReplyStream`] failed while delivering bytes.
    Send(S),
}

impl<S: fmt::Display> fmt::Display for StreamError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamError::Fault(flt) => write!(f, "{flt}"),
            StreamError::Protocol(p) => write!(f, "{p}"),
            StreamError::Send(s) => write!(f, "rpc send error: {s}"),
        }
    }
}

impl<S: fmt::Debug + fmt::Display> core::error::Error for StreamError<S> {}

/// Failure returned by a generated RPC client method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError<E, S = ()> {
    /// Declared method error (`FrameKind::Err`).
    Err(E),
    /// Protocol fault returned by the server (`FrameKind::Fault`).
    Fault(OwnedFault),
    /// Server reply violated the RPC protocol or failed verification.
    Protocol(ProtocolError),
    /// The application's [`Sender`] failed.
    Send(S),
}

impl<E, S> From<StreamError<S>> for CallError<E, S> {
    fn from(value: StreamError<S>) -> Self {
        match value {
            StreamError::Fault(f) => CallError::Fault(f),
            StreamError::Protocol(p) => CallError::Protocol(p),
            StreamError::Send(s) => CallError::Send(s),
        }
    }
}

impl<E: fmt::Debug, S: fmt::Display> fmt::Display for CallError<E, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::Err(e) => write!(f, "rpc method error: {e:?}"),
            CallError::Fault(flt) => write!(f, "{flt}"),
            CallError::Protocol(p) => write!(f, "rpc protocol error: {p}"),
            CallError::Send(s) => write!(f, "rpc send error: {s}"),
        }
    }
}

impl<E: fmt::Debug, S: fmt::Debug + fmt::Display> core::error::Error for CallError<E, S> {}

/// Server-side rejection when decoding or validating an incoming `Call` frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    service_id: u64,
    method: u16,
    one_way: bool,
    fault: Box<OwnedFault>,
}

impl Rejected {
    #[doc(hidden)]
    pub fn malformed(service_id: u64, method: u16, one_way: bool, message: impl Into<String>) -> Self {
        Rejected {
            service_id,
            method,
            one_way,
            fault: Box::new(OwnedFault {
                code: FaultCode::Malformed,
                message: Some(message.into()),
                broken: None,
            }),
        }
    }

    #[doc(hidden)]
    pub fn unsupported(service_id: u64, method: u16, message: impl Into<String>) -> Self {
        Rejected {
            service_id,
            method,
            one_way: false,
            fault: Box::new(OwnedFault {
                code: FaultCode::Unsupported,
                message: Some(message.into()),
                broken: None,
            }),
        }
    }

    #[doc(hidden)]
    pub fn from_read_error(service_id: u64, method: u16, one_way: bool, err: crate::Error) -> Self {
        Self::malformed(service_id, method, one_way, format!("{err}"))
    }

    #[doc(hidden)]
    pub fn from_validate_error(service_id: u64, method: u16, one_way: bool, err: ValidateError) -> Self {
        match err {
            ValidateError::Invalid(inv) => Rejected {
                service_id,
                method,
                one_way,
                fault: Box::new(OwnedFault {
                    code: FaultCode::Invalid,
                    // Codes only: the caller turns them into text in its own language.
                    message: None,
                    broken: Some(Box::new(OwnedBrokenRule {
                        table_name: inv.table.to_string(),
                        field: inv.field.map(ToString::to_string),
                        index: inv.index,
                        rule: format_rule(inv.rule),
                        min: inv.rule.bounds().0.map(ToString::to_string),
                        max: inv.rule.bounds().1.map(ToString::to_string),
                        reason: inv.rule.reason().map(ToString::to_string),
                    })),
                }),
            },
            ValidateError::Read(e) => Self::from_read_error(service_id, method, one_way, e),
        }
    }

    /// Best-effort `(service_id, method)` extracted from an incoming frame header when `buf` has 16 bytes
    /// and `frame_version == 1`; `(0, 0)` otherwise.
    #[doc(hidden)]
    pub fn from_frame_error(buf: &[u8], err: FrameError) -> Self {
        Self::from_frame_error_for(buf, err, 0, &[])
    }

    /// Like [`from_frame_error`](Self::from_frame_error), for the service `service_id` whose
    /// one-way methods are `one_way`: a broken frame addressed to one of them gets no reply.
    #[doc(hidden)]
    pub fn from_frame_error_for(buf: &[u8], err: FrameError, service_id: u64, one_way: &[u16]) -> Self {
        let (sid, method) = if buf.len() >= HEADER_LEN && buf[0] == FRAME_VERSION {
            let method = u16::from_le_bytes(buf[2..4].try_into().unwrap());
            let sid = u64::from_le_bytes(buf[8..16].try_into().unwrap());
            (sid, method)
        } else {
            (0, 0)
        };
        let is_one_way = sid == service_id && buf.get(1) == Some(&FrameKind::Call.as_u8()) && one_way.contains(&method);
        Self::malformed(sid, method, is_one_way, format!("{err}"))
    }

    /// The `FaultCode` describing this rejection (`Malformed`, `Unsupported`, or `Invalid`).
    pub fn code(&self) -> FaultCode {
        self.fault.code
    }

    /// Encodes this rejection as a `Fault` frame, or `None` if the target method is one-way.
    pub fn to_frame(&self) -> Option<Vec<u8>> {
        if self.one_way {
            None
        } else {
            let f = self.fault.as_borrowed();
            Some(fault_frame_bytes(self.service_id, self.method, &f))
        }
    }
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.fault)
    }
}

impl core::error::Error for Rejected {}

/// Trait implemented by generated writer marker types (`TW<'static>` / `UW<'static>`)
/// for RPC payload serialization.
pub trait RpcPayload {
    type Writer<'b>;

    #[doc(hidden)]
    fn encode_with(f: impl FnOnce(&mut Self::Writer<'_>)) -> Result<(u16, u64, Vec<u8>), Error>;
}

/// Trait implemented by generated message/union types (`T<'static>` / `U<'static>`) for verified
/// zero-copy RPC response viewing.
pub trait RpcView {
    type View<'a>;

    #[doc(hidden)]
    fn verify_payload(variant: u16, schema_id: u64, payload: &[u8]) -> Result<(), ProtocolError>;

    #[doc(hidden)]
    fn view_verified(variant: u16, payload: &[u8]) -> Self::View<'_>;
}

/// Typed reply builder for a unary method on the server edge.
pub struct Reply<O, E = ()> {
    service_id: u64,
    method: u16,
    _marker: PhantomData<fn() -> (O, E)>,
}

impl<O, E> Reply<O, E> {
    #[doc(hidden)]
    #[inline]
    pub fn new(service_id: u64, method: u16) -> Self {
        Reply { service_id, method, _marker: PhantomData }
    }

    /// Encodes an `Ok` frame using the method's declared output type `O`.
    pub fn ok(self, f: impl FnOnce(&mut O::Writer<'_>)) -> Vec<u8>
    where
        O: RpcPayload,
    {
        let (variant, schema_id, payload) = O::encode_with(f).expect("rpc reply fits in i32::MAX");
        let mut out = BytesMut::new();
        encode_frame(
            FrameHeader {
                kind: FrameKind::Ok,
                method: self.method,
                variant,
                service_id: self.service_id,
            },
            schema_id,
            &payload,
            &mut out,
        )
        .expect("rpc ok frame fits in i32::MAX");
        out.to_vec()
    }

    /// Encodes an `Err` frame using the method's declared error type `E`.
    /// Only callable on methods declared with `Result<T, E>`.
    pub fn err(self, f: impl FnOnce(&mut E::Writer<'_>)) -> Vec<u8>
    where
        E: RpcPayload,
    {
        let (variant, schema_id, payload) = E::encode_with(f).expect("rpc err fits in i32::MAX");
        let mut out = BytesMut::new();
        encode_frame(
            FrameHeader {
                kind: FrameKind::Err,
                method: self.method,
                variant,
                service_id: self.service_id,
            },
            schema_id,
            &payload,
            &mut out,
        )
        .expect("rpc err frame fits in i32::MAX");
        out.to_vec()
    }

    /// Encodes a `Fault` frame for this call.
    pub fn fault(self, code: FaultCode, message: Option<&str>) -> Vec<u8> {
        fault_frame_bytes(
            self.service_id,
            self.method,
            &Fault { code, message, broken: None },
        )
    }
}

/// Typed reply builder for a streaming method on the server edge.
pub struct StreamReply<O, E = ()> {
    service_id: u64,
    method: u16,
    sent_item: bool,
    _marker: PhantomData<fn() -> (O, E)>,
}

impl<O, E> StreamReply<O, E> {
    #[doc(hidden)]
    #[inline]
    pub fn new(service_id: u64, method: u16) -> Self {
        StreamReply { service_id, method, sent_item: false, _marker: PhantomData }
    }

    /// Encodes one `Item` frame using the method's declared item type `O`.
    pub fn item(&mut self, f: impl FnOnce(&mut O::Writer<'_>)) -> Vec<u8>
    where
        O: RpcPayload,
    {
        self.sent_item = true;
        let (variant, schema_id, payload) = O::encode_with(f).expect("rpc item fits in i32::MAX");
        let mut out = BytesMut::new();
        encode_frame(
            FrameHeader {
                kind: FrameKind::Item,
                method: self.method,
                variant,
                service_id: self.service_id,
            },
            schema_id,
            &payload,
            &mut out,
        )
        .expect("rpc item frame fits in i32::MAX");
        out.to_vec()
    }

    /// Encodes the terminating `End` frame (16 bytes).
    pub fn end(self) -> Vec<u8> {
        let mut out = BytesMut::with_capacity(HEADER_LEN);
        encode_end(self.service_id, self.method, &mut out);
        out.to_vec()
    }

    /// Encodes an `Err` frame before any `Item` has been emitted.
    ///
    /// # Panics
    /// Panics if called after [`item`](Self::item).
    pub fn err(self, f: impl FnOnce(&mut E::Writer<'_>)) -> Vec<u8>
    where
        E: RpcPayload,
    {
        assert!(!self.sent_item, "StreamReply::err cannot be called after an Item was sent");
        let (variant, schema_id, payload) = E::encode_with(f).expect("rpc stream err fits in i32::MAX");
        let mut out = BytesMut::new();
        encode_frame(
            FrameHeader {
                kind: FrameKind::Err,
                method: self.method,
                variant,
                service_id: self.service_id,
            },
            schema_id,
            &payload,
            &mut out,
        )
        .expect("rpc stream err frame fits in i32::MAX");
        out.to_vec()
    }

    /// Encodes a terminating `Fault` frame for this call.
    pub fn fault(self, code: FaultCode, message: Option<&str>) -> Vec<u8> {
        fault_frame_bytes(
            self.service_id,
            self.method,
            &Fault { code, message, broken: None },
        )
    }
}

/// Owned, verified RPC response payload; call [`view`](Self::view) for zero-copy field access.
pub struct Response<T> {
    bytes: Bytes,
    range: Range<usize>,
    variant: u16,
    _marker: PhantomData<fn() -> T>,
}

impl<T: RpcView> Response<T> {
    fn from_bytes_slice(bytes: Bytes, range: Range<usize>, variant: u16) -> Self {
        Response { bytes, range, variant, _marker: PhantomData }
    }

    /// Borrows a verified view over the owned payload.
    #[inline]
    pub fn view(&self) -> T::View<'_> {
        T::view_verified(self.variant, &self.bytes[self.range.clone()])
    }

    /// Returns the union variant tag (`0` when the payload type is a table).
    #[inline]
    pub fn variant(&self) -> u16 {
        self.variant
    }
}

impl<T> fmt::Debug for Response<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("len", &(self.range.end - self.range.start))
            .field("variant", &self.variant)
            .finish()
    }
}

/// The reply bytes of a streaming call, delivered as they arrive.
///
/// Implemented by the application (over an HTTP body, a socket, a channel…). Chunks may split
/// frames anywhere; the client reassembles them.
pub trait ReplyStream {
    type Error;

    /// The next chunk of reply bytes, or `Ok(None)` once the reply is complete. Empty chunks
    /// are allowed and skipped; a source that only ever returns empty chunks never ends, like
    /// one that never returns.
    fn next_chunk(&mut self) -> impl Future<Output = Result<Option<Vec<u8>>, Self::Error>>;
}

/// A reply that has already arrived in full, delivered as one chunk.
pub struct Buffered<E> {
    bytes: Option<Vec<u8>>,
    _marker: PhantomData<fn() -> E>,
}

impl<E> Buffered<E> {
    /// Wraps a complete reply.
    pub fn new(bytes: Vec<u8>) -> Self {
        Buffered { bytes: Some(bytes), _marker: PhantomData }
    }
}

impl<E> ReplyStream for Buffered<E> {
    type Error = E;

    fn next_chunk(&mut self) -> impl Future<Output = Result<Option<Vec<u8>>, E>> {
        let chunk = self.bytes.take().filter(|b| !b.is_empty());
        async move { Ok(chunk) }
    }
}

/// How a generated client sends its calls; implemented by the application.
///
/// Graven does not move bytes: an implementation sends the `Call` frame over whatever the
/// application uses and returns what came back. `timeout_ms` is the method's declared timeout
/// (or the caller's override), for the implementation to enforce.
pub trait Sender {
    type Error;

    /// Sends one `Call` frame and returns the reply frames (empty for a one-way method).
    fn send(
        &self,
        call: Vec<u8>,
        timeout_ms: Option<u32>,
    ) -> impl Future<Output = Result<Vec<u8>, Self::Error>>;

    /// Sends a `Call` frame of a streaming method and returns its reply as it arrives.
    ///
    /// The default waits for the whole reply with [`send`](Self::send). Override it to read
    /// items while the server is still sending them (a stream that stays open, for example).
    fn send_stream(
        &self,
        call: Vec<u8>,
        timeout_ms: Option<u32>,
    ) -> impl Future<Output = Result<impl ReplyStream<Error = Self::Error>, Self::Error>> {
        let sent = self.send(call, timeout_ms);
        async move { sent.await.map(Buffered::new) }
    }
}

impl<T: Sender + ?Sized> Sender for &T {
    type Error = T::Error;

    fn send(
        &self,
        call: Vec<u8>,
        timeout_ms: Option<u32>,
    ) -> impl Future<Output = Result<Vec<u8>, Self::Error>> {
        (**self).send(call, timeout_ms)
    }

    fn send_stream(
        &self,
        call: Vec<u8>,
        timeout_ms: Option<u32>,
    ) -> impl Future<Output = Result<impl ReplyStream<Error = Self::Error>, Self::Error>> {
        (**self).send_stream(call, timeout_ms)
    }
}

/// One whole frame cut from a reply, with its decoded header.
struct RawFrame {
    bytes: Bytes,
    header: FrameHeader,
    schema_id: u64,
    payload_len: usize,
}

impl RawFrame {
    fn payload_range(&self) -> Range<usize> {
        let start = HEADER_LEN + envelope::HEADER_LEN;
        start..start + self.payload_len
    }

    fn payload(&self) -> &[u8] {
        &self.bytes[self.payload_range()]
    }

    fn fault(&self) -> StreamError<core::convert::Infallible> {
        match decode_verified_fault(self.header.variant, self.schema_id, self.payload()) {
            Ok(flt) => StreamError::Fault(flt),
            Err(p) => StreamError::Protocol(p),
        }
    }
}

fn decode_verified_fault(variant: u16, schema_id: u64, payload: &[u8]) -> Result<OwnedFault, ProtocolError> {
    if variant != 0 {
        return Err(ProtocolError::UnexpectedVariant(variant));
    }
    if schema_id != FAULT_SCHEMA_ID {
        return Err(ProtocolError::SchemaMismatch {
            expected: FAULT_SCHEMA_ID,
            found: schema_id,
        });
    }
    decode_fault(payload).map(|flt| flt.to_owned()).map_err(ProtocolError::Payload)
}

fn widen<S>(e: StreamError<core::convert::Infallible>) -> StreamError<S> {
    match e {
        StreamError::Fault(f) => StreamError::Fault(f),
        StreamError::Protocol(p) => StreamError::Protocol(p),
        StreamError::Send(never) => match never {},
    }
}

/// The `Item`s of a streaming reply, read as the reply's bytes arrive.
pub struct StreamResponse<T, R> {
    src: R,
    buf: BytesMut,
    src_done: bool,
    pending: Option<RawFrame>,
    service_id: u64,
    method: u16,
    done: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<T: RpcView, R: ReplyStream> StreamResponse<T, R> {
    /// The next item, `None` after the `End` frame, or the error that ended the stream.
    /// Nothing is returned after an error.
    pub async fn next(&mut self) -> Option<Result<Response<T>, StreamError<R::Error>>> {
        if self.done {
            return None;
        }
        let r = self.step().await;
        if !matches!(r, Some(Ok(_))) {
            self.done = true;
        }
        r
    }

    async fn step(&mut self) -> Option<Result<Response<T>, StreamError<R::Error>>> {
        let frame = match self.pending.take() {
            Some(f) => f,
            None => match self.read_frame().await {
                Ok(Some(f)) => f,
                Ok(None) => return Some(Err(StreamError::Protocol(ProtocolError::Truncated))),
                Err(e) => return Some(Err(e)),
            },
        };
        if let Err(e) = self.check_call(&frame) {
            return Some(Err(e));
        }
        match frame.header.kind {
            FrameKind::Item => {
                let variant = frame.header.variant;
                if let Err(e) = T::verify_payload(variant, frame.schema_id, frame.payload()) {
                    return Some(Err(StreamError::Protocol(e)));
                }
                let range = frame.payload_range();
                Some(Ok(Response::from_bytes_slice(frame.bytes, range, variant)))
            }
            FrameKind::End => self.check_last().err().map(Err),
            FrameKind::Fault => Some(Err(self.check_last().err().unwrap_or_else(|| widen(frame.fault())))),
            other => Some(Err(StreamError::Protocol(ProtocolError::UnexpectedKind(other)))),
        }
    }

    /// Buffers bytes until one whole frame is available and cuts it off; `None` when the reply
    /// ended with nothing buffered.
    async fn read_frame(&mut self) -> Result<Option<RawFrame>, StreamError<R::Error>> {
        loop {
            if !self.buf.is_empty() {
                match decode_frame(&self.buf) {
                    Ok(f) => {
                        let (header, schema_id, payload_len, len) = (f.header, f.schema_id, f.payload.len(), f.len());
                        let bytes = self.buf.split_to(len).freeze();
                        return Ok(Some(RawFrame { bytes, header, schema_id, payload_len }));
                    }
                    Err(FrameError::Incomplete { .. }) if !self.src_done => {}
                    Err(FrameError::Incomplete { .. }) => {
                        return Err(StreamError::Protocol(ProtocolError::Truncated));
                    }
                    Err(e) => return Err(StreamError::Protocol(ProtocolError::Frame(e))),
                }
            } else if self.src_done {
                return Ok(None);
            }
            match self.src.next_chunk().await {
                Ok(Some(chunk)) => self.buf.extend_from_slice(&chunk),
                Ok(None) => self.src_done = true,
                Err(e) => return Err(StreamError::Send(e)),
            }
        }
    }

    fn check_call(&self, frame: &RawFrame) -> Result<(), StreamError<R::Error>> {
        if frame.header.service_id != self.service_id {
            return Err(StreamError::Protocol(ProtocolError::ServiceMismatch {
                expected: self.service_id,
                found: frame.header.service_id,
            }));
        }
        if frame.header.method != self.method {
            return Err(StreamError::Protocol(ProtocolError::MethodMismatch {
                expected: self.method,
                found: frame.header.method,
            }));
        }
        Ok(())
    }

    /// After a final frame, nothing else may already be buffered. The source is not read
    /// further: a reply that stays open after its last frame is the application's concern.
    fn check_last(&self) -> Result<(), StreamError<R::Error>> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(StreamError::Protocol(ProtocolError::TrailingBytes(self.buf.len())))
        }
    }

    /// Reads the first frame. A stream (`Item` or `End`) is kept for [`next`](Self::next); an
    /// `Err` frame is returned to the caller; a `Fault` becomes the error.
    async fn open(src: R, service_id: u64, method: u16) -> Result<(Self, Option<RawFrame>), StreamError<R::Error>> {
        let mut s = StreamResponse {
            src,
            buf: BytesMut::new(),
            src_done: false,
            pending: None,
            service_id,
            method,
            done: false,
            _marker: PhantomData,
        };
        let frame = match s.read_frame().await? {
            Some(f) => f,
            None => return Err(StreamError::Protocol(ProtocolError::EmptyReply)),
        };
        s.check_call(&frame)?;
        match frame.header.kind {
            FrameKind::Item | FrameKind::End => {
                s.pending = Some(frame);
                Ok((s, None))
            }
            FrameKind::Err => {
                s.check_last()?;
                Ok((s, Some(frame)))
            }
            FrameKind::Fault => {
                s.check_last()?;
                Err(widen(frame.fault()))
            }
            other => Err(StreamError::Protocol(ProtocolError::UnexpectedKind(other))),
        }
    }
}

/// Helper used by generated clients to build a `Call` frame.
#[doc(hidden)]
pub fn encode_call_frame<I: RpcPayload>(
    service_id: u64,
    method: u16,
    f: impl FnOnce(&mut I::Writer<'_>),
) -> Vec<u8> {
    let (variant, schema_id, payload) = I::encode_with(f).expect("rpc call fits in i32::MAX");
    let mut out = BytesMut::new();
    encode_frame(
        FrameHeader { kind: FrameKind::Call, method, variant, service_id },
        schema_id,
        &payload,
        &mut out,
    )
    .expect("rpc call frame fits in i32::MAX");
    out.to_vec()
}

fn check_terminal_header<E, S>(bytes: &[u8], expected_service: u64, expected_method: u16) -> Result<Frame<'_>, CallError<E, S>> {
    if bytes.is_empty() {
        return Err(CallError::Protocol(ProtocolError::EmptyReply));
    }
    let frame = decode_frame(bytes).map_err(|e| CallError::Protocol(ProtocolError::Frame(e)))?;
    if frame.len() != bytes.len() {
        return Err(CallError::Protocol(ProtocolError::TrailingBytes(bytes.len() - frame.len())));
    }
    if frame.header.service_id != expected_service {
        return Err(CallError::Protocol(ProtocolError::ServiceMismatch {
            expected: expected_service,
            found: frame.header.service_id,
        }));
    }
    if frame.header.method != expected_method {
        return Err(CallError::Protocol(ProtocolError::MethodMismatch {
            expected: expected_method,
            found: frame.header.method,
        }));
    }
    Ok(frame)
}

fn decode_fault_from_frame<E, S>(frame: &Frame<'_>) -> CallError<E, S> {
    match decode_verified_fault(frame.header.variant, frame.schema_id, frame.payload) {
        Ok(flt) => CallError::Fault(flt),
        Err(p) => CallError::Protocol(p),
    }
}

/// Helper used by generated clients to decode a `Result<T, E>` unary reply.
#[doc(hidden)]
pub fn decode_unary<T: RpcView, E: RpcView, S>(
    service_id: u64,
    method: u16,
    bytes: Vec<u8>,
) -> Result<Response<T>, CallError<Response<E>, S>> {
    let bytes = Bytes::from(bytes);
    let payload_range;
    let variant;
    let is_err;
    {
        let frame = check_terminal_header(&bytes, service_id, method)?;
        let len = frame.payload.len();
        let start = HEADER_LEN + envelope::HEADER_LEN;
        payload_range = start..start + len;
        variant = frame.header.variant;
        match frame.header.kind {
            FrameKind::Ok => {
                T::verify_payload(variant, frame.schema_id, frame.payload).map_err(CallError::Protocol)?;
                is_err = false;
            }
            FrameKind::Err => {
                E::verify_payload(variant, frame.schema_id, frame.payload).map_err(CallError::Protocol)?;
                is_err = true;
            }
            FrameKind::Fault => return Err(decode_fault_from_frame(&frame)),
            other => return Err(CallError::Protocol(ProtocolError::UnexpectedKind(other))),
        }
    }
    let resp = Response::from_bytes_slice(bytes, payload_range, variant);
    if is_err { Err(CallError::Err(resp)) } else { Ok(Response::from_bytes_slice(resp.bytes, resp.range, resp.variant)) }
}

/// Helper used by generated clients to decode an infallible (`-> T`) unary reply.
#[doc(hidden)]
pub fn decode_unary_ok<T: RpcView, S>(
    service_id: u64,
    method: u16,
    bytes: Vec<u8>,
) -> Result<Response<T>, CallError<(), S>> {
    let bytes = Bytes::from(bytes);
    let payload_range;
    let variant;
    {
        let frame = check_terminal_header(&bytes, service_id, method)?;
        let len = frame.payload.len();
        let start = HEADER_LEN + envelope::HEADER_LEN;
        payload_range = start..start + len;
        variant = frame.header.variant;
        match frame.header.kind {
            FrameKind::Ok => {
                T::verify_payload(variant, frame.schema_id, frame.payload).map_err(CallError::Protocol)?;
            }
            FrameKind::Fault => return Err(decode_fault_from_frame(&frame)),
            other => return Err(CallError::Protocol(ProtocolError::UnexpectedKind(other))),
        }
    }
    Ok(Response::from_bytes_slice(bytes, payload_range, variant))
}

/// Helper used by generated clients to start reading a `Result<stream T, E>` reply.
#[doc(hidden)]
pub async fn decode_stream_start<T: RpcView, E: RpcView, R: ReplyStream>(
    service_id: u64,
    method: u16,
    src: R,
) -> Result<StreamResponse<T, R>, CallError<Response<E>, R::Error>> {
    let (stream, err) = StreamResponse::open(src, service_id, method).await?;
    match err {
        None => Ok(stream),
        Some(frame) => {
            E::verify_payload(frame.header.variant, frame.schema_id, frame.payload()).map_err(CallError::Protocol)?;
            let range = frame.payload_range();
            Err(CallError::Err(Response::from_bytes_slice(frame.bytes, range, frame.header.variant)))
        }
    }
}

/// Helper used by generated clients to start reading a `-> stream T` reply.
#[doc(hidden)]
pub async fn decode_stream_ok<T: RpcView, R: ReplyStream>(
    service_id: u64,
    method: u16,
    src: R,
) -> Result<StreamResponse<T, R>, CallError<(), R::Error>> {
    match StreamResponse::open(src, service_id, method).await? {
        (stream, None) => Ok(stream),
        (_, Some(_)) => Err(CallError::Protocol(ProtocolError::UnexpectedKind(FrameKind::Err))),
    }
}

/// Helper used by generated clients to check that a one-way reply buffer is empty.
#[doc(hidden)]
pub fn decode_oneway_reply<S>(bytes: Vec<u8>) -> Result<(), CallError<(), S>> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(CallError::Protocol(ProtocolError::UnexpectedOneWayReply))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn sample_call_frame() -> BytesMut {
        let mut out = BytesMut::new();
        encode_frame(
            FrameHeader {
                kind: FrameKind::Call,
                method: 3,
                variant: 0,
                service_id: 0x1122_3344_5566_7788,
            },
            0xdead_beef_cafe_babe,
            b"hello",
            &mut out,
        )
        .unwrap();
        out
    }

    #[test]
    fn decode_check_order_normative() {
        let valid = sample_call_frame();

        // 1. Input shorter than 16 -> Incomplete { needed: 16 } even if byte 0/1 look bad.
        for len in 0..16 {
            let mut short = vec![99u8; len];
            if len >= 2 {
                short[1] = 200;
            }
            assert_eq!(decode_frame(&short), Err(FrameError::Incomplete { needed: 16 }));
        }

        // 2. Bad frame_version takes priority over bad kind and reserved.
        let mut bad_ver = valid.clone();
        bad_ver[0] = 2;
        bad_ver[1] = 99;
        bad_ver[6] = 1;
        assert_eq!(decode_frame(&bad_ver), Err(FrameError::UnsupportedFrameVersion(2)));

        // 3. Bad kind takes priority over non-zero reserved.
        for bad_k in [0u8, 7, 255] {
            let mut bad_kind = valid.clone();
            bad_kind[1] = bad_k;
            bad_kind[6] = 1;
            assert_eq!(decode_frame(&bad_kind), Err(FrameError::BadKind(bad_k)));
        }

        // 4. Non-zero reserved -> BadFrameHeader (even with corrupt body).
        let mut bad_res = valid.clone();
        bad_res[6] = 1;
        bad_res[16] = b'X';
        assert_eq!(decode_frame(&bad_res), Err(FrameError::BadFrameHeader));

        // 4b. End with non-zero variant -> BadFrameHeader.
        let mut bad_end = BytesMut::new();
        encode_end(0x42, 1, &mut bad_end);
        bad_end[4] = 1;
        assert_eq!(decode_frame(&bad_end), Err(FrameError::BadFrameHeader));

        // 5. Cut envelope body -> Incomplete with total frame length needed.
        assert_eq!(
            decode_frame(&valid[..20]),
            Err(FrameError::Incomplete { needed: 16 + 20 })
        );
        assert_eq!(
            decode_frame(&valid[..valid.len() - 1]),
            Err(FrameError::Incomplete { needed: valid.len() })
        );

        // 5b. Bad magic -> Envelope(BadMagic).
        let mut bad_magic = valid.clone();
        bad_magic[16] = b'X';
        assert_eq!(decode_frame(&bad_magic), Err(FrameError::Envelope(Error::BadMagic)));

        // 5c. Bitflip in payload -> Envelope(ChecksumMismatch).
        let mut bad_crc = valid.clone();
        bad_crc[36] ^= 1;
        assert_eq!(decode_frame(&bad_crc), Err(FrameError::Envelope(Error::ChecksumMismatch)));
    }

    #[test]
    fn roundtrip_multiple_frames_and_end() {
        let mut stream = BytesMut::new();
        let h1 = FrameHeader {
            kind: FrameKind::Item,
            method: 2,
            variant: 7,
            service_id: 12345,
        };
        encode_frame(h1, 0xaaaa, b"one", &mut stream).unwrap();
        encode_end(12345, 2, &mut stream);
        let h2 = FrameHeader {
            kind: FrameKind::Ok,
            method: 9,
            variant: 0,
            service_id: 999,
        };
        encode_frame(h2, 0xbbbb, b"two", &mut stream).unwrap();

        let decoded: Vec<_> = frames(&stream).collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0].header, h1);
        assert_eq!(decoded[0].schema_id, 0xaaaa);
        assert_eq!(decoded[0].payload, b"one");
        assert_eq!(decoded[1].header.kind, FrameKind::End);
        assert_eq!(decoded[1].len(), HEADER_LEN);
        assert_eq!(decoded[1].payload, b"");
        assert_eq!(decoded[2].header, h2);
        assert_eq!(decoded[2].payload, b"two");
    }

    #[test]
    fn fault_roundtrip_and_unknown_code() {
        let fault = Fault {
            code: FaultCode::Invalid,
            message: Some("sku out of range"),
            broken: Some(BrokenRule {
                table_name: "AdjustStock",
                field: Some("delta"),
                index: Some(4),
                rule: "range",
                min: Some("-100"),
                max: Some("100"),
                reason: None,
            }),
        };
        let bytes = fault_frame_bytes(0x55, 3, &fault);
        let frame = decode_frame(&bytes).unwrap();
        assert_eq!(frame.header.kind, FrameKind::Fault);
        assert_eq!(frame.header.method, 3);
        assert_eq!(frame.header.variant, 0);
        assert_eq!(frame.header.service_id, 0x55);
        assert_eq!(frame.schema_id, FAULT_SCHEMA_ID);
        let decoded = decode_fault(frame.payload).unwrap();
        assert_eq!(decoded, fault);

        // Future FaultCode roundtrips as Unknown and normalizes to Internal.
        let future = Fault {
            code: FaultCode::Unknown(99),
            message: None,
            broken: None,
        };
        let fbytes = fault_frame_bytes(0x55, 3, &future);
        let fframe = decode_frame(&fbytes).unwrap();
        let fdec = decode_fault(fframe.payload).unwrap();
        assert_eq!(fdec.code, FaultCode::Unknown(99));
        assert_eq!(fdec.code.normalized(), FaultCode::Internal);
    }
}
