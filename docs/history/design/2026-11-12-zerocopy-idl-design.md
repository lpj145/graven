# zerocopy-idl — Design

> **Historical document.** Written on 2026-11-12, when the project was called zidl (later
> Graven). It records the original intent and decisions; several were refined later. The
> normative text is the [format specification](../../spec/graven-format-v1.md).

Date: 2026-11-12 · Status: draft for approval

## 1. Intent

A general-purpose IDL (games, banking, streaming, IoT) for describing messages that travel
over the network and are written to disk. The motivation: FlatBuffers, Cap'n Proto,
Protobuf and FBE have poor trade-offs for anyone who wants to **build the message front to
back, straight into a `BytesMut` taken from a pool** (the `bytes` crate). The FlatBuffers
builder writes back to front and is painful to use.

The goal of the project is to research and test until we reach a format that is "close to
perfect", measured by **beating FlatBuffers and Cap'n Proto in benchmarks**.

## 2. Priorities (the user's, in this order)

1. **Schema evolution** — producer and consumer on different versions coexist without a
   coordinated deploy.
2. **Zero-copy reads** — fields read straight from the buffer, without deserializing.
3. **Size** — bytes on the wire and on disk.

Hard constraint: **forward-only** writing into a pooled `BytesMut`.

## 3. Goals, non-goals, anti-goals

**Goals**
- Binary format + Rust runtime + schema compiler (`.zidl`) that generates Rust.
- Forward writing with a pool, supporting data that is already available **and** data that
  arrives bit by bit (lists of unknown length).
- Evolution: add a field, remove a field, make a field optional, add a variant to an
  enum/union.
- A benchmark suite against FlatBuffers and Cap'n Proto.

**Non-goals (for now)**
- Other languages (JS/browser comes later; the format must be friendly to it — see §9).
- RPC, compression, runtime reflection, in-place mutation of finished messages.
- Messages > 2 GiB (large streams = split into frames).

**Anti-goals (a failure even if it "works")**
- Any UB or panic when reading a malformed or malicious buffer.
- Per-message allocation on the hot write path with a warm pool.
- An incompatible schema change passing silently through the compiler.
- A builder API that requires a construction order the compiler does not check (the borrow
  checker must prevent misuse).

## 4. Binary format

Every decision in this section is **the assistant's choice**, accepted by the user in the
conversation, to be validated by benchmark.

### 4.1 Conventions
- **Little-endian** always.
- **No alignment padding.** Fields are packed; reads via `T::from_le_bytes(buf[o..o+N])`.
  Hypothesis: the cost of unaligned reads ≈ 0 on x86-64/ARM64/`DataView`. If the benchmark
  shows a relevant read penalty, revisit (optional per-table alignment).
- **Offsets**: `i32` relative to the position of the slot itself. `0` = absent. Signed
  because lists built bit by bit produce backward pointers (§5.3). Practical limit:
  message ≤ 2 GiB.

### 4.2 Table (the evolvable type)
```
[u16 fixed_len][fixed part: fixed_len bytes]
```
- `fixed_len` is the size of the fixed part **as written by the producer**.
- The compiler assigns each field, in ID order, a fixed position in the fixed part. The
  position of an ID never changes between versions.
- Reading a field whose position + size > `fixed_len` ⇒ field absent (§4.3). Bytes beyond
  what the reader knows are ignored.

Slot kinds in the fixed part:
| Type | Slot |
|---|---|
| `u8..u128`, `i8..i128`, `f32`, `f64` | N bytes |
| `bool` | 2 bits (presence + value) in a bit byte (§4.3) |
| enum | size of the declared base type (`u8`/`u16`/`u32`) |
| `struct` (frozen) | inline, fixed size |
| `string`, `bytes`, `[T]`, nested table | `i32` offset |
| union | `u16 tag` + `i32 offset` (tag 0 = none) |

### 4.3 Presence on the wire ("0 is a value too")
Absence must be **on the wire**, not only in the schema: the wire does not carry which
fields were deprecated, and there is no guarantee every reader was updated (old files on
disk, IoT devices, lagging shards).

- **Every scalar field** (integers, floats, enums, inline structs, bools) has a **presence
  bit**, from v1 on, whether required or optional. `bool` uses 2 bits (presence + value).
- Variable-size fields (string, bytes, lists, tables) use offset `0` = absent. A union uses
  `tag 0` = absent.
- Semantics **1 = present**. Consequence: an all-zero fixed part = "nothing present". The
  builder's template is **all zeros** (no `memcpy` of defaults); the default is applied by
  the reader.
- **Bit allocation**: the compiler allocates the next free bit of the open bit byte; if
  there is none, it creates a new bit byte **immediately before** the field's slot.
  Deterministic in ID order, stable across versions. Bits can be shared across versions:
  an old writer always writes 0 in the bits it does not know, which means "absent" —
  correct.
- **Field read rule** (in this order):
  1. slot or bit byte outside `fixed_len` ⇒ absent;
  2. slot partially inside `fixed_len` ⇒ `Err` (malformed);
  3. otherwise, the presence bit (or offset/tag ≠ 0) decides.

### 4.3.1 Required vs optional = the reader's expectation
- The wire is identical for both; the difference is only the reader's API.
- `Msg::read(buf)?` validates **once** that every required field *according to the
  reader's schema* is present: an `AND` with a precomputed mask per bit byte + a check of
  the required pointer slots (and of `fixed_len`). Failure ⇒ `Err(Missing { field })`.
- After that, accessors of required fields return `T` (no `Option`, no extra cost).
  Optional (`T?`) returns `Option<T>`; a present `0` is distinct from absent.
- A field with an explicit default (`= x`) returns `x` when absent and is not part of the
  required mask.
- Nested tables and list elements are validated when accessed (`Result`), or all at once
  via `verify()` (§4.7). Fine API decisions: the builder's.
- Writing does not enforce required fields (only a `debug_assert` in `finish()`).

### 4.4 Variable-size data
- `string`: `[u32 len][utf-8]` — validated as UTF-8 on read (with an optional unsafe
  `_unchecked` variant).
- `bytes`: `[u32 len][bytes]` — reads return `&[u8]` (or a sliced `Bytes`, zero-copy).
- `[scalar]` / `[struct]`: `[u32 count][packed elements]`.
- `[table]`: `[u32 count][u16 elem_fixed_len][count × fixed part]` — inline elements,
  no per-element header; `elem_fixed_len` gives the correct stride across versions.
- `[string]` / `[bytes]` / `[[T]]`: `[u32 count][count × i32 offset]`.

### 4.5 Root
The buffer starts directly at the root table (`[u16 fixed_len]...`). No magic number in the
core; framing (size, type, version) is the job of an optional layer outside the initial
scope.

### 4.6 Enums, unions and structs
- An enum declares a base type fixed forever. Unknown value ⇒ `Unknown(raw)`.
- Union: variants are **tables only** (for a string or list, wrap it in a table). Unknown
  tag ⇒ `Unknown(tag)`; the payload is behind an offset, so an old reader simply does not
  visit it.
- A frozen `struct` contains **only** scalars, enums, bools and other structs — never
  offsets. So `[struct]` can be copied byte for byte and never needs fix-up.

### 4.7 Read safety
- Every access is bounds-checked; an offset outside the buffer ⇒ `Err`, never UB/panic.
- Reading is lazy (on-demand accessors), so malformed pointers only cost on the field
  accessed.
- An optional `verify()` walks the whole message with a **depth limit and a limit on bytes
  visited** (protects against cycles created by backward offsets and against
  amplification). Default limits: the builder's decision.

## 5. Writing

### 5.1 Pool
```rust
pub trait BufPool { fn take(&self) -> BytesMut; fn give(&self, b: BytesMut); }
```
The runtime provides a simple implementation (e.g. `Mutex<Vec<BytesMut>>`, or
thread-local); details are left to the builder. `finish()` returns `Bytes` (`freeze`)
ready to send without a copy.

### 5.2 Model
- Opening a table reserves and fills (template) its fixed part at the end of the buffer.
- Scalar setters write into the already reserved slot.
- Variable-size data is **appended at the end** and the slot receives the offset
  (back-patching a region already reserved — the buffer only grows forward).
- Data already available (`&str`, `&[u8]`, `&[T]`): appended directly.
- Opening a child (nested table, list) borrows the parent `&mut`; the parent is usable
  again only when the child is closed — an order guaranteed by the borrow checker.

### 5.3 Lists
- **Known count** (`list_with_capacity(n)`): reserves `n` contiguous fixed parts; the
  elements' variable-size data comes after (forward offsets).
- **Unknown count** (`list_streaming()`): the elements' fixed parts are built in a
  **second `BytesMut` from the same pool**; variable-size data goes into the main buffer.
  On `close()`, the fixed parts are copied at once (one `memcpy`) and the auxiliary buffer
  returns to the pool. The slots of those elements point **backward** (negative offset).
  This is the only copy in the system.
- **Fix-up after the memcpy**: while in the auxiliary buffer, each pointer slot holds the
  offset computed as if the element were at a virtual base; on `close()`, all pointer
  slots shift by the same delta. The codegen emits, per table, the static list of pointer
  slot positions (strings, lists, tables, unions) — the fix-up is O(number of pointers),
  with no reflection.
- Lists of scalars/structs with unknown count: appended directly (nothing interleaved),
  no auxiliary buffer.
- A known-count list closed with fewer than `n` elements: the `count` is corrected to the
  real one and the rest stays as dead bytes. The format allows unreferenced bytes; `verify`
  ignores them.

## 6. Schema language (`.zidl`)

Sketch (the final syntax is the builder's decision, keeping the concepts):
```
namespace bank.v1;

enum Currency : u16 { BRL = 0, USD = 1, EUR = 2 }

struct Vec3 { x: f32, y: f32, z: f32 }          // frozen, inline, never evolves

table Entry {
  account @0: u64;
  amount  @1: i64;
  memo    @2: string?;
}

union Payload { Transfer @1: Transfer, Refund @2: Refund }

table Transaction {
  id       @0: u128;
  currency @1: Currency = BRL;                   // default
  entries  @2: [Entry];
  note     @3: string?;
  @4: reserved;                                  // removed field
  payload  @5: Payload;
}
```
- Explicit IDs (`@n`), never reused; a removed field ⇒ `reserved` (the slot stays, and the
  writer leaves it **absent** — a size cost accepted in exchange for O(1) access without a
  vtable).
- `T?` = optional; `= x` = explicit default.

### 6.1 Compatibility (lockfile)
The compiler keeps `<schema>.zidl.lock` with the computed layout. On recompilation:

**Error**
- changing a field's type/size, reusing an ID, reordering, removing without `reserved`;
- changing an enum's base type or a frozen `struct`;
- adding a **required field without a default** after v1 (every new reader would fail to
  read old messages) — a new field is `T?` or has `= x`;
- optional → required without a default (same reason).

**Warning (breaking for old readers, but an explicit failure, never wrong data)**
- `reserved` on a field that was required: old readers get `Err(Missing)`; readers that
  had it as optional get `None`.

**Allowed**
- a new optional/defaulted ID at the end, `reserved` on an optional field,
  required → optional, a new enum/union variant.

## 7. Code structure (Cargo workspace)

- `zidl` — runtime: read primitives, builder, pool, verify. Aim for `no_std + alloc` (the
  `bytes` crate supports it) for IoT.
- `zidl-compiler` — parser, layout computation, lockfile, Rust codegen; the `zidlc` binary
  + a `build.rs` helper.
- `zidl-bench` — criterion; equivalent schemas in `.zidl`, `.fbs`, `.capnp`; the official
  `flatbuffers` and `capnp` crates.

## 8. Benchmarks and the winning criterion

**Scenarios**
1. Financial transaction: scalars, monetary `u64`/`i64`, strings, a list of entries built
   bit by bit.
2. Game frame: hundreds of small entities with `Vec3`/floats.
3. IoT telemetry: a tiny message (size dominates).
4. Streaming: large `bytes` blobs.

**Metrics**: write time (warm pool), read time (all fields; one random field),
bytes/message, allocations/message (instrumented allocator), cross-version reads (v1 reads
v2 and v2 reads v1 — correctness, not just time).

**Winning**: win on writing and size; tie or win on reading; zero allocations per message
on the hot path.

**Fair comparison** (every variant reported, none hidden): reads with and without a
verifier/traversal limit in every format; Cap'n Proto size packed and unpacked; zidl
strings with UTF-8 validation (default) and `_unchecked`.

## 9. Preparing for JS (future)
A JS reader = a lazy view over an `ArrayBuffer` via `DataView`; `u64/i64` ⇒ `BigInt`;
strings via `TextDecoder`. Nothing in the format may require alignment or features
unavailable in JS. A WASM build of the Rust runtime is a later alternative.

## 10. Suggested phases
1. Runtime + hand-written "generated" code for the benchmark schemas.
2. Benchmarks vs FlatBuffers/Cap'n Proto — validate the format's hypotheses (no padding,
   i32 offsets, shared bits) **before** investing in the compiler.
3. `.zidl` compiler + Rust codegen + lockfile.
4. Evolution test matrix and fuzzing of the reader/verify.

## 11. Left to the builder
Pool implementation, `verify` default limits, the concrete `.zidl` syntax, API names,
internal module organization, the fuzzing tool.
