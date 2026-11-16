# Graven RPC frames v1 (normative specification)

Status: **draft** (alpha). This document, together with the conformance vectors in
`testvectors/rpc-v1/`, defines RPC frames v1: the bytes of service calls and their replies.

**One message, two containers.** A Graven message is the same bytes everywhere: a
[format v1](graven-format-v1.md) message, read in place and checked by `verify`. At rest it
travels in an **envelope** (format v1 §8: magic, version, `schema_id`, length, CRC32C),
because stored bytes have no context and nothing else protects them. In flight it travels in a
**frame**, which carries only what the call does not already know. A frame never contains an
envelope. Moving a message between the two never re-encodes it: a message received in a
frame is stored by writing the same bytes inside an envelope, and a stored record is sent by
removing its envelope.

## 0. Terminology

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT** and **MAY** are to be
interpreted as described in RFC 2119.

- **frame**: a header followed by one format v1 message (none for `End`).
- **caller** / **server**: the party that initiates a call / sends replies.
- **shape**: the call/reply pattern declared by a service method (§3).
- All multi-byte integers are **little-endian**.

---

## 1. Scope

This specification defines:

1. the frame header, its version and kinds;
2. the valid reply frame sequences for each method shape;
3. the normative decoding check order and error reporting;
4. the fixed `graven.rpc` schema for protocol faults.

Out of scope: transport protocols (HTTP, TCP, UDP, queues), multiplexing (`call_id`),
deadlines on the wire, retries, authentication, and cancellation.

**Transport requirement.** Frames carry no checksum. They MUST travel over a transport that
delivers bytes in order and intact: TLS, a local socket or pipe, or a queue that checks its
records. Over a transport that can corrupt bytes silently, `verify` still rejects a message
whose structure breaks, but a corrupted value inside a well-formed message goes unnoticed.

---

## 2. Frame layout

Every frame starts with 7 bytes:

```text
offset  size  field
0       1     version_kind   high 4 bits: frame version (1); low 4 bits: kind (1..=6)
1       2     variant        union tag of the message when its type is a union; 0 otherwise
3       4     len            message length in bytes (0 for End); at most 2^31 - 1
```

A `Call` frame continues with where it goes:

```text
7       2     method         method ID (@n, 1..=65535)
9       8     service_id     FNV-1a 64-bit hash of the qualified service name
```

The message follows the header: at offset 17 in a `Call`, at offset 7 in every other frame.
A frame is `17 + len` bytes for a `Call`, `7 + len` otherwise.

### 2.1 Frame kinds

| `kind` | Name | Sent by | Message |
|---|---|---|---|
| `1` | `Call` | caller | the method input `I` |
| `2` | `Ok` | server | the method output `T` |
| `3` | `Err` | server | the method error `E` |
| `4` | `Item` | server | one item `T` of a stream |
| `5` | `End` | server | none (`len = 0`) |
| `6` | `Fault` | server | a `graven.rpc.Fault` (§5) |

### 2.2 Messages, types and variants

- The message is a bare format v1 message: no envelope, no `schema_id`. Its type follows
  from the method (and, for a reply, from the frame kind): the caller knows which method it
  called, and the server reads `method` and `service_id` from the `Call`.
- When the type is a **union**, `variant` holds the chosen tag and the message is that
  variant's table. When the type is a table (including `Fault`), `variant` MUST be `0`. An
  `End` frame has `variant = 0` and `len = 0`.
- A reader that does not recognize a union tag reads the message as `Unknown(variant)`
  without failing.
- Reply frames do not repeat `method` or `service_id`: a reply belongs to the call it
  answers, and the transport carries one call's replies.
- Frames are self-delimiting and MAY be concatenated back-to-back on a byte stream.

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

1. If `buf.len() < 7`, fail with `Incomplete { needed: 7 }`.
2. Let `version = buf[0] >> 4`. If `version != 1`, fail with
   `UnsupportedFrameVersion(version)`.
3. Let `kind = buf[0] & 0x0f`. If `kind` is not in `1..=6`, fail with `BadKind(kind)`.
4. Read `variant` (`buf[1..3]`) and `len` (`buf[3..7]`). If `len > 2^31 - 1`, or
   `kind == End` with `variant != 0` or `len != 0`, fail with `BadFrameHeader`.
5. Let `h = 17` for `Call`, `7` otherwise. If `buf.len() < h + len`, fail with
   `Incomplete { needed: h + len }`.

The frame is then `h + len` bytes, its message `buf[h .. h + len]`. Bytes after the frame
belong to the next frame and MUST be ignored by the single-frame decoder.

Checking the message (`verify` against the type of the method, the variant, the service and
the method of a `Call`) is the job of generated service code, which rejects a bad call with a
`Fault` (§5), not of the frame decoder.

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

- New `FaultCode` values may be added in the future; a reader that does not recognize a
  code treats it as `Internal`.
- `message` is a diagnostic for developers, never text for end users. On `Invalid`, the
  server sends codes only, in `broken`, and leaves `message` absent: the caller turns
  `rule`, `min`, `max` and `reason` into a message in its own user's language.
- On a `Fault` frame, `variant` MUST be `0`.
- A server that cannot decode a `Call` replies with a `Fault` (`Malformed` or
  `Unsupported`), unless the call's header says it is for a one-way method: one-way methods
  never get a reply.
