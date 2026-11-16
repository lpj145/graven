# Proposal 0005: one message, two containers

Status: **accepted and implemented** in 0.3.1. Changes the frame layout of
[RPC frames v1](../spec/graven-rpc-v1.md) while Graven is in alpha; there is no frame
format v2, and the first layout is not kept. Format v1 (messages, envelope, vectors) does not
change.

## Motivation

A Graven message is written for two places with opposite needs.

**At rest** (a file, a log, a queue that keeps data), bytes have no context. A reader opens
them months later and must find out what they are, whether they are complete and whether
they are intact. The envelope answers that: magic and version, `schema_id`, length, CRC32C.

**In flight** (a call and its reply), bytes always have context. The caller knows which
method it called, so it knows the type of every reply; the transport (TLS, a local socket)
already guarantees the bytes arrive intact. What travels with each message is paid on every
message.

The first frame layout put a whole envelope inside every frame, so the network carries the storage
container too. For small messages the containers outweigh the data: a stream item of 15
bytes travels in 55, of which 40 are frame header and envelope. And the per-item CRC defeats
transport compression, because it is different on every item:

| 1,000 stream items | Bytes per item | gzip, total | Read (JavaScript) |
|---|---|---|---|
| first layout: frame + envelope per item (15 B message) | 55 | 9.6 KB | 0.24 ms |
| message with only a length prefix | 19 | 3.2 KB | 0.11 ms |
| the same data as NDJSON | 22.7 | 2.6 KB | 0.12 ms |

(One machine, Node 24; the benchmark is in the history of this proposal.)

Mixing the two containers is also ambiguous: a frame body of the first layout was a valid
record of an envelope file, and nothing says which container a given set of bytes belongs to.

## The rule

**One message, two containers.**

- The **message** is the same bytes everywhere: format v1, read in place, checked by
  `verify`.
- The **envelope** is the container at rest. It never appears inside a frame.
- The **frame** is the container in flight. It carries only what the call does not already
  know.

Moving a message from one container to the other never re-encodes it. A server that stores
an item it received writes the same bytes inside an envelope (24 bytes and a CRC); a server
that sends a stored record strips the envelope and sends the message.

## The frame

All integers little-endian. Every frame starts with the same 7 bytes:

```text
offset  size  field
0       1     version_kind   high 4 bits: frame version (1); low 4 bits: kind (1..=6)
1       2     variant        union tag of the message when its type is a union; 0 otherwise
3       4     len            message length in bytes (0 for End)
```

A `Call` adds who it is for, then the message:

```text
7       2     method         method ID (@n, 1..=65535)
9       8     service_id     FNV-1a 64-bit hash of the qualified service name
17      len   message        format v1 message (no envelope)
```

Every other frame is followed directly by its message:

```text
7       len   message        format v1 message (no envelope); none for End
```

| `kind` | Name | Message |
|---|---|---|
| 1 | `Call` | the method's input |
| 2 | `Ok` | the output |
| 3 | `Err` | the declared error |
| 4 | `Item` | one item of a stream |
| 5 | `End` | none (`len = 0`, `variant = 0`) |
| 6 | `Fault` | `graven.rpc.Fault` (unchanged, now without an envelope) |

What goes away and why:

- **Envelope inside frames** (magic, version, `schema_id`, CRC32C): the version is in the
  first byte, the type follows from the method and `variant`, and integrity is the
  transport's (below).
- **`service_id` and `method` on replies:** a reply belongs to the call it answers; the
  transport carries one call's replies. Only the `Call` says where it goes.
- **`reserved`:** the version nibble is where the format changes.

Sizes: a `Call` costs 17 bytes plus its message, every reply frame 7. The examples of
[services](../services.md) become:

| | first layout | now |
|---|---|---|
| `get_item("espresso")` call | 58 B | 35 B |
| its `Ok` reply | 63 B | 30 B |
| 1,000 stream items of 15 B | 55 KB | 22 KB |

Version and kind share a byte so that every frame says its version at no cost. A frame of the
first layout started with `01`, which now reads as version 0 and is refused.

## Integrity

Frames require a transport that delivers bytes **in order and intact**: TLS, a local
socket or pipe, or a queue that checks its records. This is a requirement on the transport,
not a specification of it: Graven still says nothing about HTTP, sockets or retries. A
corrupted message that still parses is caught by `verify` only when it breaks the format; a
transport that can corrupt bytes silently must not carry frames.

Envelopes keep their CRC: at rest there is no transport to rely on.

## Decoding (normative check order)

A frame decoder performs these checks in order on `buf` and stops at the first failure:

1. `buf.len() < 7` ⇒ `Incomplete { needed: 7 }`.
2. version (`buf[0] >> 4`) `≠ 1` ⇒ `UnsupportedFrameVersion(version)`.
3. kind (`buf[0] & 0x0f`) not in `1..=6` ⇒ `BadKind(kind)`.
4. `len > 2³¹ − 1`, or `kind = End` with `variant ≠ 0` or `len ≠ 0` ⇒ `BadFrameHeader`.
5. header length `h` = 17 for `Call`, 7 otherwise; `buf.len() < h + len` ⇒
   `Incomplete { needed: h + len }`.

The frame is then `h + len` bytes. Checking the message (`verify`, the type of the method,
the variant) is the generated code's job, as before.

## Unchanged

Format v1, envelope, the 42 format vectors, `graven.rpc.Fault` and `BrokenRule`, the method
shapes and reply sequences, timeouts staying out of the wire, and the client and
server APIs of the generated code. Only bytes in flight change.

## What changed in the code

- **Spec and vectors:** [graven-rpc-v1](../spec/graven-rpc-v1.md) rewritten;
  `testvectors/rpc-v1/` regenerated (23 vectors), each `.json` listing every frame with its
  message as hex.
- **Rust:** `graven::rpc` frames without envelope; `encode_frame(header, message, out)`,
  `FrameHeader::call` / `FrameHeader::reply`, `encode_end(out)`, `fault_frame_bytes(fault)`.
  `ProtocolError::{ServiceMismatch, MethodMismatch, SchemaMismatch}` are gone: a reply has no
  service, method or schema id to compare. The generated client and server edge keep their
  API.
- **JavaScript:** the same in the runtime (`encodeFrame(header, message)`, `callHeader`,
  `replyHeader`, `encodeEnd()`, `encodeFaultFrame(fault)`); the generated client keeps its
  API. The fuzzer compares every decoded frame with the Rust reference.
- `graven.rpc.BrokenRule` is always written with its full fixed part (the short form only
  existed to keep the first layout's vectors unchanged).

## Open questions

- **A checksum per stream?** An `End` could carry one CRC32C of every message of the stream:
  one per stream rather than one per item, so compression still works. Left out until a
  transport that needs it shows up.
- **Variable-length `len`?** A varint would save 2–3 bytes on small items, at the cost of a
  second integer encoding in the format. Left out: fixed widths keep every implementation
  simple.
