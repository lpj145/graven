# Graven RPC frame format v1 (normative specification)

Status: **draft**. This document, together with the conformance vectors in
`testvectors/rpc-v1/`, defines RPC frame format v1. Any change to the wire layout or
normative decode rules in this document is RPC frame format v2 (a new `frame_version`).

Every payload inside a frame is an ordinary [Graven format v1](graven-format-v1.md)
envelope (§8). Message encoding, schema evolution, and conformance vectors in
`testvectors/v1/` are unchanged.

## 0. Terminology

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT** and **MAY** are to be
interpreted as described in RFC 2119.

- **frame**: a 16-byte fixed header followed by an optional Graven format v1 envelope.
- **caller** / **server**: the party that initiates a call / sends replies.
- **shape**: the call/reply pattern declared by a service method (§3).
- Byte examples and field layouts use **little-endian** byte order for multi-byte integers.

---

## 1. Scope

This specification defines:

1. the 16-byte frame header and `frame_version = 1`;
2. frame `kind` values and how payloads (or empty bodies) map to them;
3. the valid reply frame sequences for each method shape;
4. the normative decoding check order and error reporting;
5. the fixed `graven.rpc` schema for protocol faults.

Out of scope: transport protocols (HTTP, TCP, UDP, queues), multiplexing (`call_id`),
deadlines on the wire, retries, authentication, and cancellation.

---

## 2. Frame layout

A frame is a 16-byte header followed by a body. All multi-byte integers are
little-endian.

```text
offset  size  field
0       1     frame_version   = 1
1       1     kind            1..=6
2       2     method          method ID (@n, 1..=65535)
4       2     variant         union tag of the body when its type is a union; 0 otherwise
6       2     reserved        = 0
8       8     service_id      FNV-1a 64-bit hash of qualified service name
16      M     body            one Graven format v1 envelope (absent, M = 0, for End)
```

### 2.1 Frame kinds

| `kind` | Name | Sent by | Body |
|---|---|---|---|
| `1` | `Call` | caller | envelope holding method input `I` |
| `2` | `Ok` | server | envelope holding method output `T` |
| `3` | `Err` | server | envelope holding method error `E` |
| `4` | `Item` | server | envelope holding one item `T` of a stream |
| `5` | `End` | server | none (`0` bytes) |
| `6` | `Fault` | server | envelope holding `graven.rpc.Fault` |

### 2.2 Bodies and self-delimitation

- For every kind other than `End`, the body is a complete Graven format v1 envelope
  (`"GRVN"`, `format_version = 1`, flags/reserved `0`, 64-bit `schema_id`, 32-bit
  `payload_len`, `payload_len` message bytes, and 32-bit CRC32C trailer).
- An `End` frame has no body (`M = 0`) and MUST have `variant = 0`.
- Frames are self-delimiting: an `End` frame is `16` bytes, and any other frame is
  `16 + 24 + payload_len` bytes. Multiple frames MAY be concatenated back-to-back on a
  byte stream.
- When a method's input `I`, output `T`, or error `E` is declared as a **union**, the
  frame header's `variant` holds the chosen union tag (`u16`), and the body envelope
  carries that variant's table (with the variant table's `schema_id`). When the declared
  type is a table (including `Fault`), `variant` MUST be written as `0`.
- Unknown union tags read cleanly on upgrade: a reader that does not recognize `variant`
  treats it as `Unknown(variant)` without failing frame decoding.
- Reply frames (`Ok`, `Err`, `Item`, `End`, `Fault`) echo the `service_id` and `method`
  of the `Call` frame they answer.

---

## 3. Calls and reply sequences

Every service method has one of five shapes:

| Declaration | Shape | Valid server reply sequence |
|---|---|---|
| `(I) -> T` | unary | `Ok` or `Fault` |
| `(I) -> Result<T, E>` | unary | `Ok`, `Err`, or `Fault` |
| `(I) -> stream T` | stream | zero or more `Item`, followed by `End` or `Fault` |
| `(I) -> Result<stream T, E>` | stream | `Err` (before any `Item`), or zero or more `Item` followed by `End` or `Fault` |
| `(I)` | one-way | none (no reply, not even `Fault`) |

No frame may follow `Ok`, `Err`, `End`, or `Fault` for a given call.

---

## 4. Decoding and check order (normative)

A frame decoder MUST perform the following checks in exact order on `buf` and stop at the
first failure:

1. If `buf.len() < 16`, fail with `Incomplete { needed: 16 }`.
2. If `buf[0] != 1`, fail with `UnsupportedFrameVersion(buf[0])`.
3. If `buf[1]` is not in `1..=6`, fail with `BadKind(buf[1])`.
4. Read `variant = le_u16(buf[4..6])` and `reserved = le_u16(buf[6..8])`.
   If `reserved != 0`, or `(kind == End && variant != 0)`, fail with `BadFrameHeader`.
5. If `kind == End`, decoding succeeds immediately with total frame length `16` and an
   empty body.
   Otherwise, decode the body starting at `&buf[16..]` as a Graven format v1 envelope
   ([spec §8.4](graven-format-v1.md)):
   - If envelope decoding reports `Incomplete { needed: n }`, fail with
     `Incomplete { needed: 16 + n }` (saturating on overflow).
   - If envelope decoding reports any other error (`BadMagic`, `UnsupportedVersion`,
     `BadHeader`, `ChecksumMismatch`), fail with that envelope error.
   - Otherwise, decoding succeeds with total frame length `16 + env.len()`.

Bytes after `frame.len()` belong to the next frame and MUST be ignored by the single-frame
decoder.

Checking that `service_id`, `method`, `variant`, and the envelope's `schema_id` match a
particular service schema is the responsibility of generated service code (and produces
application-level rejections / `Fault`), not of the frame decoder.

---

## 5. Protocol faults (`graven.rpc`)

Protocol faults use the following fixed Graven schema, published with its lockfile as
`crates/graven/schemas/rpc.graven`:

```graven
namespace graven.rpc;

enum FaultCode : u16 {
  Unsupported      = 1,   // unknown service_id, method, or variant
  Malformed        = 2,   // the frame or its body failed to decode or verify
  Invalid          = 3,   // the input broke a validation rule
  DeadlineExceeded = 4,   // the server stopped because the method's timeout passed
  Internal         = 5,   // the server failed; details stay on the server
  Unavailable      = 6,   // the server cannot take calls now; the caller may retry
}

table BrokenRule {
  table_name @0: string;
  field      @1: string?;
  index      @2: u32?;
  rule       @3: string;   // "range", "len", "finite", "known", or "check(name)"
  min        @4: string?;  // range/len: the declared lower bound, as a decimal number
  max        @5: string?;  // range/len: the declared upper bound, as a decimal number
  reason     @6: string?;  // check(name): the code the check returned
}

table Fault {
  code    @0: FaultCode;
  message @1: string?;
  broken  @2: BrokenRule?;  // present when code = Invalid
}
```

- `Fault`'s envelope `schema_id` is `schema_id("graven.rpc.Fault")` (64-bit FNV-1a).
- New `FaultCode` values may be added in the future; a reader that does not recognize a
  code treats it as `Internal`.
- `message` is a diagnostic for developers, never text for end users. On `Invalid`, the
  server sends codes only, in `broken`, and leaves `message` absent: the caller turns
  `rule`, `min`, `max` and `reason` into a message in its own user's language.
- `BrokenRule.min`, `max` and `reason` (@4–@6) were added after the first release of this
  schema, as optional fields under the rules of format v1 §9. A writer that sets none of them
  may write `BrokenRule` with the 17-byte fixed part of the first release; readers treat the
  missing fields as absent either way.
- On a `Fault` frame, header `variant` MUST be `0`. When rejecting a `Call` whose method
  could not be determined (or was malformed), `service_id` and `method` from the call's
  header (when available; otherwise `0`) are echoed in the `Fault` header.
