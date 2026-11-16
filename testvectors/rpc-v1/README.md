# Graven RPC frames v1: conformance vectors

These files pin the behavior specified in [`docs/spec/graven-rpc-v1.md`](../../docs/spec/graven-rpc-v1.md).
<!-- ANCHOR: body -->
Every RPC frame decoder must read each `.bin` and obtain the result described in the matching
`.json`: either `{"frames": [...]}`, one entry per frame in order, or `{"error": "..."}` for the
first frame that fails.

Each frame entry has `kind`, `variant`, `message` (the message bytes as lowercase hex, empty for
`End`) and, for a `Call`, `method` and `service_id` (hex). A `Fault` entry adds the decoded
`graven.rpc.Fault`: `fault_code`, `fault_message` and `fault_broken` (when present). A `Fault`
whose message is not a valid `graven.rpc.Fault` gives `{"error": "Fault(<error>)"}`.

The messages of `Call`, `Ok`, `Err` and `Item` vectors are arbitrary bytes: a frame decoder
does not verify them.

## Valid frames

- `frame_call`: a `Call` (17-byte header) for method 1 of `conformance.v1.Service`.
- `frame_call_union`: a `Call` whose input is a union (`variant = 2`).
- `frame_ok`: an `Ok` reply (7-byte header).
- `frame_ok_empty_message`: an `Ok` reply with an empty message.
- `frame_err_union`: an `Err` reply with union `variant = 2`.
- `frame_item_union_unknown_tag`: an `Item` with an unknown union tag `99`.
- `frame_end`: an `End` frame: 7 bytes, `variant = 0`, `len = 0`.
- `frame_fault_invalid_range`: an `Invalid` fault whose `BrokenRule` carries `min` and `max`.
- `frame_fault_invalid_check`: an `Invalid` fault whose `BrokenRule` carries a check `reason`.
- `frame_fault_future_code`: a fault with an unknown future `FaultCode = 99`.
- `frame_stream`: two `Item` frames and an `End`, back to back.
- `frame_stream_fault`: an `Item` followed by a `Fault`.

## Invalid frames (normative check order, spec §4)

- `frame_short_header`: 6 bytes -> `Incomplete(7)` (step 1).
- `frame_bad_version`: first byte `0x01`, the first byte of the older layout -> `UnsupportedFrameVersion(0)` (step 2).
- `frame_bad_version_2`: version 2, with a bad kind and length too -> `UnsupportedFrameVersion(2)` (step 2).
- `frame_bad_kind`: kind 7, with a bad length too -> `BadKind(7)` (step 3).
- `frame_bad_kind_zero`: kind 0 -> `BadKind(0)` (step 3).
- `frame_len_too_large`: `len` over 2³¹ − 1 -> `BadFrameHeader` (step 4).
- `frame_end_nonzero_variant`: an `End` with `variant = 1` -> `BadFrameHeader` (step 4).
- `frame_end_nonzero_len`: an `End` with `len = 1` -> `BadFrameHeader` (step 4).
- `frame_cut_call_header`: a `Call` cut inside its 17-byte header -> `Incomplete(<frame length>)` (step 5).
- `frame_cut_message`: an `Item` one byte short -> `Incomplete(<frame length>)` (step 5).
- `frame_fault_bad_message`: a `Fault` whose message is one byte -> `Fault(OutOfBounds)`.
