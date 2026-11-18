# Proposal 0006: rebuilding messages, and alignment

Status: **accepted and implemented** in 0.3.4. Adds an attribute and runtime functions;
the wire format does not change. Every message written under this proposal is a valid
format v1 message, and every v1 reader reads it.

## Motivation

**Changing a stored message.** Messages are written in one forward pass and never changed.
The first application built on Graven stored products and had to rename one: the only way
was to write a new message by hand, field by field, copying every field that did not change.
That is tedious, and it breaks silently when the schema gains a field nobody remembered to
copy.

**Alignment.** Format v1 has no alignment: a reader takes any value at any position, so a
message is read where it lies, inside a frame, after an envelope or in a memory-mapped file.
That stays. It costs in two places:

- **lists of numbers**: a `[f32]` cannot be handed out as a `&[f32]` in Rust or a
  `Float32Array` in JavaScript, which need aligned memory, so it is read element by element,
  and SIMD code has to copy it first;
- **records in a file**: concatenated envelopes start wherever the previous one ended, so a
  small record can straddle two pages (two page faults through `mmap`, two cache lines) and
  records cannot be read with direct I/O, which needs block-aligned offsets.

The specification already leaves room for both: data may be anywhere, and unreferenced bytes
are valid (§7.5, §10.2). A writer may insert padding, and a v1 reader must accept it.

## Design

### 1. Rebuild

For every table the generators add:

- `XW::rebuild(builder, &old, |w| …)` (Rust) / `XW.rebuild(old, (w) => …)` (JavaScript):
  writes a new message of `X`. The closure writes the fields that change; every field it did
  not write is then copied from `old`, a verified view. It returns the envelope, as `to_bytes`
  does.
- `w.fill_from(&old)` / `w.fillFrom(old)`: the same copy, on any writer, so a nested table
  can be rebuilt too:

  ```rust
  let bytes = ProductW::rebuild(&mut b, &old, |w| {
      w.name("Café 1 kg");
      let mut a = w.supplier();       // a nested table: change one field, keep the rest
      a.city("Lisboa");
      a.fill_from(&old.supplier());
  })?;
  ```

Rules:

- A field counts as written when its presence bit is set (scalars, enums, structs, `bool`),
  its pointer is non-zero (strings, bytes, lists, tables) or its union tag is non-zero.
- Copying keeps presence: an absent field stays absent, a defaulted field that was absent stays
  absent (it is not written with its default), and an unknown enum value is copied as is.
- Strings, bytes and scalar lists are copied as bytes; lists of tables, nested tables and
  unions are rebuilt element by element with `fill_from`.
- **What `old` does not describe is not copied:** fields added by a newer schema and union
  variants this schema does not know. Rebuild with the schema that owns the data. A
  generator for another language that implements rebuild documents the same limit.
- The result is in the canonical layout (§10.2), and goes through the same paths as any
  other write (`to_bytes_validated` checks rules the same way).

### 2. Aligned lists: `#[align(to = N)]`

```graven
table Samples {
  rate @0: u32;
  #[align(to = 16)]
  values @1: [f32];
  #[align(to = 64)]
  blob @2: bytes?;
}
```

- Valid on `bytes` and on lists of numeric scalars (`[u8]` … `[u128]`, `[i8]` … `[i128]`,
  `[f32]`, `[f64]`). `N` is a power of two from 2 to 4096.
- The writer inserts zero bytes before the list's length so that the **first element** (the
  first byte of `bytes`) lands at an offset that is a multiple of `N`, counted from the
  **start of the container**: the envelope's first byte when the builder writes envelopes
  (Rust `Builder::new_enveloped`, JavaScript `Builder`), the message's first byte otherwise.
  The padding is unreferenced, so readers need nothing new.
- The attribute is recorded in the lockfile (`align`). Adding, changing or removing it is
  compatible (old and new readers read both layouts) and `gravenc diff` lists it.
- Moving a message to another container (an envelope into a frame) keeps it valid; the
  data is then aligned or not depending on where it lands, which readers check.

Reading:

- Rust: `ScalarList<T>::as_slice() -> Option<&[T]>` for numeric `T`: `Some` when the
  elements are aligned in memory for `T` and the target is little-endian, `None` otherwise
  (then `iter()` / `get()` as today). It works on any list, aligned on purpose or by chance.
- JavaScript: `ScalarList.asTypedArray()` returns a `Uint8Array` … `Float64Array` /
  `BigInt64Array` view of the elements, without copying, when they are aligned; otherwise
  `undefined`. (JavaScript typed arrays use the platform byte order; every engine in use is
  little-endian, and the runtime checks it once.)
- The alignment of the data in memory depends on where the container itself lies: a file
  mapped with `mmap`, or a buffer from `fetch(...).arrayBuffer()`, starts aligned; a frame
  inside a network buffer usually does not, and the reader falls back to `iter()`.

### 3. Aligned records

```rust
let mut b = Builder::new_enveloped(&LocalPool).with_record_align(4096);
```

```ts
const b = new Builder().withRecordAlign(4096);
```

- On a builder that writes envelopes, `with_record_align(R)` (`R` a power of two from 2 to
  65536) pads the end of each message with zero bytes so that the whole envelope
  (`24 + payload_len`) is a multiple of `R`. Envelopes written one after the other, into a
  file that starts at offset 0, then all start at a multiple of `R`: a record is at a known
  page or block boundary, and does not straddle two when it fits in one.
- The padding is inside the payload, after the message, covered by `payload_len` and the
  CRC: unreferenced bytes, valid v1. Readers, `envelopes()` and `gravenc scan` need nothing
  new.
- Together with `#[align]`, the elements of an aligned list are aligned in memory when the
  file is mapped, which is what makes `as_slice()` work on a memory-mapped file.

## What this costs

- Padding bytes: at most `N − 1` per aligned list and `R − 1` per record. Small records with
  a large `R` waste most of each block; choose `R` from the record size (a cache line, 64, for
  small records; a page, 4096, for large ones).
- The dynamic writer (`graven-dynamic`, used by `gravenc from-json`) does not apply
  `#[align]` yet: what it writes is valid, without the padding.
- In JavaScript, a message sealed into a batch of length-prefixed frames (`seal()`, not
  `sealEnvelope()`) as the first of the batch is moved after writing, so its lists keep their
  alignment only up to 16. Envelopes and single messages are not affected.
- The layout with padding is not the one the conformance vectors pin, so a schema that uses
  `#[align]` is canonical only among writers that implement this proposal. Vectors are
  unchanged: none of them uses the attribute.

## Not in this proposal

- **Patching a message in place** (overwriting a string of the same or a smaller size,
  appending a larger one and re-pointing its slot, then recomputing the envelope's length
  and CRC). It is valid v1 and saves a rebuild on large records, but leaves unreferenced
  bytes that need a later compaction. It waits for a case that rebuild does not serve.
- Alignment of scalars inside a table's fixed part: modern processors read unaligned
  scalars at the same speed unless they straddle a cache line, and the fixed part is small.
- Lists of structs: struct fields are packed, so an aligned start does not align the fields.
