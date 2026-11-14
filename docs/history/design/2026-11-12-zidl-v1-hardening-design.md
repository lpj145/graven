# zidl v1: formalize, standardize and test (design)

> **Historical document.** Written on 2026-11-12, when the project was called zidl (later
> Graven). It records the plan that produced the frozen v1 format; some details were refined
> later (for example, namespaces no longer carry versions, and the magic became `GRVN`). The
> normative text is the [format specification](../../spec/graven-format-v1.md).

Date: 2026-11-12 · Status: awaiting approval
Based on: `2026-11-12-zerocopy-idl-design.md` (the original design) and
`benchmarks/2026-11-12-round2.md` (current performance).

## 1. Intent and why

The format will run inside other people's code, on versions we do not control, and data will
stay on disk for years. The format and runtime already work and already beat the competitors in the
benchmarks, but today:

- the "generated" code is written by hand, and the evolution rules hold only by discipline;
- there is no normative spec, only a design with open decisions;
- a message on disk does not say which schema it contains and has no checksum;
- the public API exposes `unsafe`, and the generated readers were never fuzzed.

The user wants to **formalize, standardize and test** before building more on top. This is
not the time for new features.

## 2. Goals

1. A **normative v1 spec** of the binary format + conformance test vectors.
2. A standard **envelope** for disk and network: just `to_bytes` / `from_bytes` (with magic,
   version, schema id, CRC32C).
3. **`Verified<T>`**: take `unsafe` out of the public API.
4. A **`.zidl` compiler** (`zidlc`) with a **lockfile** that refuses incompatible changes,
   generating the Rust code that is written by hand today.
5. **Tests**: fuzzing of the generated readers and of the envelope, an evolution matrix
   generated from real `.zidl` files, Miri, fixes for the review items.

## 3. Non-goals (for now)

- An incremental decoder for TCP / `tokio_util::codec` (the user's decision: "something for
  to and from bytes is great").
- Bindings for other languages, SDK, RPC, compression.
- New performance optimizations.
- Lists of lists (`[[T]]`) — the runtime does not support them; out of v1.
- A native decimal type / domain types (timestamp, currency).
- CI: the project does not use git (the user's preference); "continuous" here means
  reproducible scripts + a saved corpus, not a pipeline.

## 4. Anti-goals (a failure even if it works)

- A performance regression beyond noise (~5%) relative to round 2 once the code is
  generated.
- Any panic or UB when reading malformed bytes, **including the envelope**.
- The compiler silently accepting an incompatible schema change.
- Generated code behaving differently from the hand-written code (the current
  `zidl-schemas` tests must pass without changing assertions).
- `unsafe` reachable from the public API.

## 5. Parts

The execution order is **A → B → C → D → E**. `Verified<T>` (C) comes before the compiler so
that codegen is born with the right pattern.

### A. Normative v1 spec + test vectors

- A new document `docs/spec/zidl-format-v1.md`, separate from the design. Normative
  language (MUST / MUST NOT). Content: all of §4 of the original design, closing the open
  decisions and describing **what the runtime actually does today** (where it differs from
  the design, the runtime wins, and the difference is recorded). It includes the layout
  algorithm (`zidl-layout::compute_layout`) in pseudocode, the read rules of §4.3, the
  format of each list kind, union, enum, struct, the limits (`i32`, 2 GiB) and the rules of
  `verify` (what MUST be rejected). It includes the envelope (part B) as its own section.
  The schema compatibility rules (§6.1 of the design) become a normative section.
- **Test vectors** in `testvectors/v1/`: each case is a `.bin` + a readable expectation file
  (`.json` or text; the builder's decision) with the schema, the expected value of each field
  **or** the expected error. Minimum coverage: every field type present/absent/default,
  evolution (writer v1 → reader v2 and vice versa), a streaming list (backward offsets), a
  union with an unknown tag, an unknown enum, and invalid cases (truncated slot, offset out
  of range, invalid UTF-8, cycle, exhausted budget, envelope with wrong CRC/wrong
  magic/unknown version/wrong schema).
- A Rust test (`conformance`) runs every vector. The `.bin` files are generated once by a
  program and then **frozen**: the test compares the bytes written by the builder with the
  `.bin` (the writer is deterministic) and reads the `.bin` with the reader. If writing
  changes one byte, the test fails.
- At the end of part A, format v1 is **frozen**: any future change requires a new version in
  the envelope.

### B. Envelope (`to_bytes` / `from_bytes`)

An optional layer over the raw message; messages without an envelope remain valid. Layout
(little-endian):

```
offset  size  field
0       4     magic = "ZIDL" (0x5A 0x49 0x44 0x4C)
4       1     format_version = 1
5       1     flags = 0 (reserved; a reader MUST reject unknown bits)
6       2     reserved = 0 (a reader MUST reject ≠ 0)
8       8     schema_id (u64)
16      4     payload_len (u32)
20      N     payload (raw zidl message, starting at the root table)
20+N    4     crc32c of bytes [0, 20+N)
```

- **schema_id**: identifies the *root type*, stable across compatible versions. Value =
  64-bit FNV-1a of the fully qualified name (`"bank.v1.Transaction"`), recorded in the
  lockfile on the first compilation and **read from the lockfile** from then on (renaming
  the type does not change the id). Incompatible change = new namespace (`bank.v2`) = new
  id. *(The assistant's decision.)*
- **CRC32C** (Castagnoli) over header + payload. Implementation: the `crc32c` crate if it
  builds on `no_std`; otherwise, an in-house slicing-by-8 implementation. *(The assistant's
  decision; the builder chooses and records it.)*
- Runtime API (`zidl::envelope`):
  - `encode(schema_id, payload: &[u8], out: &mut BytesMut)` and
    `decode(buf: &[u8]) -> Result<Envelope<'_>, Error>` where
    `Envelope { schema_id, payload: &[u8] }`. `decode` checks magic, version, flags,
    reserved, size and CRC — in this order — and never panics.
  - A copy-free variant from the `Builder`: the builder reserves the 20 header bytes before
    the root and writes header + CRC at the end, returning `Bytes`. The raw message must not
    be copied. *(A goal; if it requires an invasive change to the builder, the builder may
    first deliver the version with a copy and record the measured cost.)*
- Generated API per root type: `Transaction::from_bytes(buf) -> Result<Verified<Transaction>, Error>`
  (decode + check `schema_id` + full `verify()`) and `TransactionW::to_bytes(...)` or the
  builder equivalent. The exact name of the writing API: the builder's decision.
- New errors in `zidl::Error`: `BadMagic`, `UnsupportedVersion(u8)`, `BadHeader`,
  `Incomplete { needed: usize }`, `SchemaMismatch { expected: u64, found: u64 }`,
  `ChecksumMismatch`. `Error` becomes `#[non_exhaustive]`.
- **Incomplete ≠ corrupt** (a lesson from fjall's journal): the spec MUST distinguish the two.
  - `Incomplete { needed }`: the buffer ends before the full header or before
    `20 + payload_len + 4`. `needed` = total bytes the envelope requires. This is the normal
    result of an append-only file cut by a crash; whoever reads the file may truncate it
    there.
  - `ChecksumMismatch`: the envelope is complete and the CRC does not match. This is real
    corruption; it MUST NOT be treated as end of file.
  - Check order in `decode`: minimum header size (`Incomplete`) → magic → version →
    flags/reserved → `payload_len` fits in the buffer (`Incomplete`) → CRC. This way a
    corrupted header never turns into `Incomplete` by chance without first passing the magic.
  - `decode` on a buffer with **extra** bytes after the CRC: accept and expose how many bytes
    were consumed (`Envelope::len()`), so that whoever reads a file with several messages
    can iterate over them.
  - Mandatory test vectors: a file cut in each region (header, payload, CRC) ⇒ `Incomplete`;
    a flipped bit in the payload and in the CRC ⇒ `ChecksumMismatch`; a flipped bit in
    `payload_len` ⇒ `ChecksumMismatch` or `Incomplete`, never a panic or an accepted payload.
- **Outside the format**: `fsync`, atomic rename and truncating the file are the job of
  whoever writes (the storage), not of zidl.
- CRC32C kept (instead of xxh3) because it is available in every language a future
  implementation will need.

### C. `Verified<T>` and an API without `unsafe`

- `verify()` on the generated root types returns `Verified<'a, T>` (a wrapper that can only
  be built by `verify`). The string accessors without UTF-8 revalidation exist **only** on
  `Verified` (and on the children reached through it) and are safe.
  `TableRef::str_unchecked` stops being `pub`.
- In the `zidl` crate: `#![deny(unsafe_code)]`, with the only `from_utf8_unchecked` in a small
  module with `#[allow(unsafe_code)]` and a safety comment. Generated code:
  `#![forbid(unsafe_code)]`.
- The `zidl_trusted` benchmark switches to `Verified` and must not get worse.

### D. The `zidlc` compiler + lockfile

- A new crate `crates/zidl-compiler`: library + `zidlc` binary + a function for `build.rs`
  (`zidl_compiler::compile("schemas/bank.zidl")` generating into `OUT_DIR`).
- **Language**: the one in §6 of the original design (namespace, enum with a base type,
  frozen struct, table with `@id`, `T?`, `= default`, `@n: reserved`, unions of tables only,
  lists `[T]`, `string`, `bytes`). The syntax of the 4 existing `.zidl` files is valid as is.
  Parse errors with line and column.
- **Layout**: uses `zidl-layout` (already tested against the hand-written code).
- **Codegen**: generates the same code that is written by hand today in
  `zidl-schemas/src/{bank,game,iot,stream}.rs` (layout consts, `REQ`, `MASK`, reader,
  writer, `Elem`, `FixedW` for scalar-only tables, streaming variants, `verify`), plus what
  parts B and C add. The shared support that lives in `zidl-schemas/src/lib.rs` today
  (`Req`, `ReqMask`, `ListR`, `ListW`, `StreamW`, `Elem`, `FromTable`) moves to the `zidl`
  runtime (its own module, named at the builder's discretion).
- **Switch-over criterion**: `zidl-schemas` uses generated code; every existing test passes
  without changing assertions; `zidl-bench` runs and the numbers stay within ±5% of round 2.
  The hand-written files are removed. The evolution fixtures `evo/bank_v2`, `evo/bank_v3`
  become generated `.zidl` files.
- **Lockfile** `<schema>.zidl.lock`, readable and diffable text (format at the builder's
  discretion; suggestion: TOML). It keeps, per table: the list of IDs with type, size, slot,
  bit, required/optional/default, reserved; per enum: base type and values; per struct:
  fields and size; per root type: `schema_id`.
  - Recompiling compares the new schema with the lock and applies the rules of §6.1 of the
    original design (error / warning / allowed). Compatible ⇒ the lock is updated. Error ⇒
    nothing is generated; the message names the field, the rule and the line.
  - A `--locked` mode (and an equivalent option in `build.rs`): fail if the lock would need
    to change. This is what a release build uses.
  - Warnings (e.g. `reserved` on a field that was required) require explicit confirmation in
    the schema (syntax at the builder's discretion, e.g. an attribute
    `#[allow_breaking_readers]`); otherwise they become errors.
- **Coverage schema**: the 4 current schemas do not use unions, explicit defaults,
  `[string]`, `[bytes]`, `[struct]`, nested tables or optional `bool`. A new schema
  `schemas/conformance.zidl` covers every type of §6; it feeds the test vectors (part A uses
  a hand-written or provisional generated version; part D regenerates it and the frozen
  vectors MUST keep passing).
- **Compiler tests**: a corpus of (old lock, new schema) pairs → expected result, covering
  **every** rule of §6.1; snapshots of the generated code for the 4 schemas; fuzzing of the
  parser (it must not panic).

### E. Tests and robustness

- **Fuzzing** with `cargo-fuzz` (nightly for fuzzing only): targets `read_<schema>` (read +
  every accessor), `verify_<schema>`, `envelope_decode`, and a **differential** target: if
  `verify()` accepts, the `Verified` accessors return exactly the same as the safe ones.
  Initial corpus = the test vectors. Script `scripts/fuzz.sh`. Acceptance criterion: each
  target runs ≥ 10 min without findings (longer runs documented as a recommendation).
- A generated **evolution matrix**: v1/v2/v3 of `bank` from `.zidl` files, cross-reading in
  both directions, including through the envelope.
- **Miri** on `zidl`, `zidl-layout`, `zidl-schemas`, `zidl-compiler` (with proptest on reduced
  cases); script `scripts/miri.sh`.
- **Review items**: `n * elem_fixed_len` with `checked_mul` (error `TooLarge`, no panic, also
  on 32 bits); writer misuse panics (push beyond `n`, use after `finish`) documented as
  *programmer* panics — never data panics — and covered by `should_panic` tests.
- A **final report** in `docs/benchmarks/` (round 3) with numbers after the switch to
  generated code and the envelope (CRC cost by message size).

## 6. Constraints

- Rust edition 2024, the current workspace. The `zidl` runtime keeps targeting
  `no_std + alloc`; new runtime dependencies only if `no_std`.
- The compiler may use ordinary dependencies (it is a build tool).
- No git commits or branches (the user's recorded preference).
- The wire format does not change; the envelope is an addition.

## 7. Left to the builder

Module names and organization, the format of the vectors' expectation file, the lockfile
format, the syntax of the warning-confirmation attribute, the parser (by hand or a crate),
the name of the enveloped writing API, the CRC32C implementation.

## 8. The assistant's decisions (for the user's review)

- Order A → B → C → D → E (Verified before the compiler).
- A 20-byte envelope header, `schema_id` u64 via FNV-1a of the qualified name, fixed by the
  lockfile.
- CRC32C over header + payload, at the end.
- Generated `from_bytes` always performs a full `verify()` (safe by default).
- `[[T]]` and decimal out of v1.
- Fuzzing acceptance: ≥ 10 min per target.
