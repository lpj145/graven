# Graven RPC frame format v1: conformance vectors

These files pin the behavior specified in [`docs/spec/graven-rpc-v1.md`](../../docs/spec/graven-rpc-v1.md).
<!-- ANCHOR: body -->
Every RPC frame decoder must read each `.bin` and obtain the result described in the matching `.json`.

## Valid frames

- `frame_call`: `Call` (`kind = 1`) with an enveloped payload.
- `frame_ok`: `Ok` (`kind = 2`) with an enveloped payload.
- `frame_err_union`: `Err` (`kind = 3`) with union `variant = 2`.
- `frame_item_union_unknown_tag`: `Item` (`kind = 4`) with an unknown union tag `99`.
- `frame_end`: `End` (`kind = 5`) — 16 bytes, `variant = 0`, no body.
- `frame_fault_invalid`: `Fault` (`kind = 6`) carrying `graven.rpc.Fault` (`code = Invalid` with `BrokenRule`).
- `frame_fault_invalid_codes`: `Fault` (`kind = 6`) with `code = Invalid`, no `message`, and a
  `BrokenRule` that sets every field, including `min`, `max` and `reason` (@4–@6). The
  `.json` lists the decoded `BrokenRule` in `fault_broken`. Vectors frozen before
  `fault_broken` existed leave it out, and their `BrokenRule` is not compared.
- `frame_fault_future_code`: `Fault` (`kind = 6`) carrying an unknown future `FaultCode = 99`.
- `frame_multi`: concatenated `Item` + `End` frames in one buffer.

## Invalid frames (normative check order)

- `frame_short_header`: 15 bytes -> `Incomplete(16)` (step 1).
- `frame_bad_version`: `frame_version = 2` (with invalid `kind` and non-zero `reserved`) -> `UnsupportedFrameVersion(2)` (step 2).
- `frame_bad_kind`: `kind = 7` (with non-zero `reserved`) -> `BadKind(7)` (step 3).
- `frame_bad_reserved`: `reserved != 0` (with bad body magic) -> `BadFrameHeader` (step 4).
- `frame_end_nonzero_variant`: `kind = End` with `variant = 1` -> `BadFrameHeader` (step 4).
- `frame_cut_envelope`: 20-byte slice into envelope header -> `Incomplete(36)` (step 5).
- `frame_cut_body_payload`: frame cut 1 byte before CRC ends -> `Incomplete(<total>)` (step 5).
- `frame_bad_magic`: invalid envelope magic in body -> `Envelope(BadMagic)` (step 5).
- `frame_bad_crc`: bitflip in body payload -> `Envelope(ChecksumMismatch)` (step 5).
