# Graven binary format v1 (normative specification)

Status: **frozen**. This document, together with the conformance vectors in
[`testvectors/v1/`](../conformance-vectors.md), defines format v1 for good. Any change
to what is written here is format v2 (a new `format_version` in the envelope).

The behavior described here is the behavior **implemented** by the reference runtime
(`crates/graven`, `crates/graven-layout`) and by the conformance interpreter
(`crates/graven-conformance`). The interpreter walks the schema independently (it decides,
from the schema as data, which fields to read and in which order), but reuses the
runtime primitives (`TableRef`, `VerifyCtx`, `compute_layout`). When this text and a
vector disagree, the vector wins (§10.1).

## 0. Terminology

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT** and **MAY** are to be
interpreted as described in RFC 2119.

- **message**: a sequence of bytes that starts at the root table (no envelope). Also
  called a *buffer*; `len` is its length.
- **envelope**: an optional layer around a message (§8).
- **writer** / **reader**: whoever produces / consumes a message.
- **reader schema**: the schema the reader interprets the message with. It may be older
  or newer than the writer's (§9).
- **absolute position**: byte index from the start of the message (byte 0 = first byte
  of the root's `fixed_len`).
- **fixed-part offset**: byte index from the first byte of a table's fixed part (the
  byte right after `fixed_len`).
- Byte examples are hexadecimal; `[vector: name]` names the vector in `testvectors/v1/`
  that contains exactly those bytes.

The schema used in the examples is `conformance.v1`
(`crates/graven-conformance/schemas/conformance.graven`); its layout is in
[§3.4](#34-example-the-layout-of-conformancev1root).

---

## 1. Scope

This specification defines:

1. the format of a Graven message (tables, fields, variable-size data);
2. the read rules, including what a reader MUST reject and with which error;
3. full verification (`verify`) and its limits;
4. the optional envelope (magic, version, schema id, CRC32C);
5. the compatibility rules between versions of a schema;
6. the conformance criterion.

Out of scope: the full syntax of the `.graven` language (see the
[language guide](../graven-language.md)), the lockfile format, language APIs, on-disk
durability (`fsync`, atomic rename, truncation: those belong to the storage layer), RPC,
compression.

Not in format v1: lists of lists (`[[T]]`) and lists of unions (`[Union]`). A v1 schema
MUST NOT use them and a v1 compiler MUST reject them.

## 2. Conventions

### 2.1 Value encoding

- Every multi-byte quantity is **little-endian**.
- **There is no alignment and no padding.** Fields are packed; a reader MUST accept any
  value at any byte position.
- Integers: unsigned `u8 u16 u32 u64 u128`, two's-complement `i8 i16 i32 i64 i128`;
  1, 2, 4, 8, 16 bytes.
- Floats: `f32` / `f64` are IEEE-754 binary32 / binary64, 4 / 8 bytes. The bit pattern
  is preserved exactly (NaN payloads, `-0.0`); readers MUST NOT normalize.
  [vector: `all_fields` — `a_f32 = 0x3fc00000`, `a_f64 = 0xc0934a456d5cfaad`]
- `bool` has two encodings: 2 bits in a table's fixed part (§4.4), or 1 byte inside
  structs and list elements (§6.3, §5.3).
- Text (`string`) is UTF-8 with no terminator.

### 2.2 Pointers

A **pointer** is an `i32` stored in a 4-byte slot. Its value is relative to the absolute
position of the **first byte of the slot itself**:

```
target = absolute_position_of_slot + value
```

- `value = 0` means **absent** (a pointer never points at its own slot), except in
  `[string]`/`[bytes]` elements, where `0` means empty (§5.6).
- Negative values ("backward" pointers) are valid and MUST be accepted (§5.8).
- The reader MUST compute the target without overflow (for example, in 64 bits) and
  MUST reject a target `< 0` or `>= len` with `BadOffset`.
- A target inside the buffer whose content (length header, data) does not fit before
  `len` MUST be rejected with `OutOfBounds`.

[vector: `bad_offset` — the `name` pointer at position 72 is `2a 00 00 00` (42) and
points to 114, one byte past the end of a 114-byte message ⇒ `BadOffset`]

### 2.3 Maximum size

A message MUST NOT exceed `i32::MAX` (2 147 483 647) bytes; a writer MUST fail with
`TooLarge` instead of producing one. The reference runtime does not reject larger
buffers on read; readers are not required to reject them.

### 2.4 Root

A message starts directly with the root table: the root's `fixed_len` is in bytes
`0..2`. The raw message has no magic and no version (that is the envelope's job, §8). A
buffer shorter than 2 bytes, or whose root fixed part extends past `len`, MUST be
rejected with `OutOfBounds`.

---

## 3. Table

### 3.1 Wire shape

```
[u16 fixed_len][fixed part: fixed_len bytes]
```

- `fixed_len` is the size of the fixed part **according to the writer's schema**. The
  reference writer always writes the full `fixed_len` of its layout (§3.3), including
  reserved fields.
- The header and the whole fixed part MUST fit in the buffer; otherwise `OutOfBounds`.
- A reader whose layout is larger than `fixed_len` treats the fields outside it as
  absent (§4.2); a reader whose layout is smaller ignores the bytes it does not know.
- The fixed part holds **bit bytes** (presence bits) and **slots** (inline values,
  pointers, unions), in the order given by the layout algorithm.
- Tables that are elements of a `[Table]` list **do not** have their own `u16
  fixed_len`: they use the list's `elem_fixed_len` (§5.5).

### 3.2 Field kinds in the fixed part

| field type | kind (`FieldKind`) | slot | bits |
|---|---|---|---|
| `u8..u128`, `i8..i128`, `f32`, `f64` | `Scalar { size }` | `size` bytes | 1 (presence) |
| enum | `Scalar { size }`, `size` = size of the base type | 1, 2 or 4 bytes | 1 |
| struct (inline) | `Scalar { size }`, `size` = size of the struct | `size` bytes | 1 |
| `bool` | `Bool` | none | 2 (presence, value) |
| `string`, `bytes`, `[T]`, table | `Ptr` | 4 bytes (`i32` pointer) | 0 |
| union | `Union` | 6 bytes: `[u16 tag][i32 pointer]` | 0 |

A `reserved` field occupies in the layout exactly what it occupied with its original
type (slot and bits).

### 3.3 Layout algorithm

Fields are processed in **ID order** (`@0, @1, …`, with no gaps; reserved IDs are
included with their original type). The layout MUST be computed exactly by this
algorithm (a faithful copy of `graven_layout::compute_layout`):

```
bits(kind):  Scalar → 1;  Bool → 2;  Ptr, Union → 0
size(kind):  Scalar{size} → size;  Bool → 0;  Ptr → 4;  Union → 6

cursor ← 0                      # next free byte of the fixed part
open   ← none                   # open bit byte: (position, next free shift)
for each field f, in ID order:
    need ← bits(f.kind)
    if need > 0:
        if open = (p, s) and s + need ≤ 8:
            f.presence ← (p, s)
            open ← (p, s + need)
        else:
            p ← cursor;  cursor ← cursor + 1        # new bit byte, before the slot
            f.presence ← (p, 0)
            open ← (p, need)
    else:
        f.presence ← none
    if f.kind = Bool:
        f.slot ← none
    else:
        f.slot ← cursor;  cursor ← cursor + size(f.kind)
fixed_len ← cursor
```

Consequences (normative):

- A bit byte is opened **immediately before** the slot of the first field that did not
  fit in the previously open bit byte. A field's bit byte always precedes its slot.
- The 2 bits of a `bool` are never split across two bytes: if only 1 bit is left in the
  open byte, a new byte is opened (the leftover bit stays unused).
- Pointers and unions consume no bits and do not close the open bit byte.
- Appending fields at the end does not move the slot or bit of any earlier field;
  `fixed_len` only grows.
- Bit `(p, s)`: byte at fixed-part offset `p`, bit `s` counted from the least
  significant (`(byte >> s) & 1`). For `bool`, presence is at `s` and the value at
  `s + 1`.

### 3.4 Example: the layout of `conformance.v1.Root`

Fixed-part offsets (absolute position in the message = offset + 2, because the root
starts at 0). Bit `(p, s)`.

| ID | field | type | slot | bit |
|---|---|---|---|---|
| 0 | `a_u8` | `u8` | 1 | (0, 0) |
| 1 | `a_i16` | `i16?` | 2 | (0, 1) |
| 2 | `a_u32` | `u32 = 7` | 4 | (0, 2) |
| 3 | `a_i64` | `i64?` | 8 | (0, 3) |
| 4 | `a_u128` | `u128?` | 16 | (0, 4) |
| 5 | `a_i128` | `i128?` | 32 | (0, 5) |
| 6 | `a_f32` | `f32?` | 48 | (0, 6) |
| 7 | `a_f64` | `f64?` | 52 | (0, 7) |
| 8 | `flag` | `bool?` | — | (60, 0), value (60, 1) |
| 9 | `flag2` | `bool` | — | (60, 2), value (60, 3) |
| 10 | `color` | `Color?` (`u8`) | 61 | (60, 4) |
| 11 | `point` | `Point?` (8 bytes) | 62 | (60, 5) |
| 12 | `name` | `string?` | 70 | — |
| 13 | `blob` | `bytes?` | 74 | — |
| 14 | `nums` | `[u16]?` | 78 | — |
| 15 | `points` | `[Point]?` | 82 | — |
| 16 | `leaves` | `[Leaf]?` | 86 | — |
| 17 | `names` | `[string]?` | 90 | — |
| 18 | `blobs` | `[bytes]?` | 94 | — |
| 19 | `child` | `Leaf?` | 98 | — |
| 20 | `choice` | `Choice?` | 102 (tag 102..104, pointer 104..108) | — |
| 21 | *reserved* (was `old: u32?`) | — | 108 | (60, 6) |

`fixed_len = 112`. `RootV2` appends `extra @22: u64?` with slot 112 and bit (60, 7),
`fixed_len = 120`. `Leaf { n @0: u32; s @1: string?; next @2: Leaf? }`: `n` slot 1 bit
(0, 0), `s` slot 5, `next` slot 9, `fixed_len = 13`.

Complete message [vector: `minimal`] (114 bytes; only `a_u8 = 42` and `flag2 = true`):

```
pos 0..2    70 00          fixed_len = 112
pos 2       01             bit byte 0: a_u8 present (bit 0)
pos 3       2a             a_u8 = 42
pos 4..62   00 …           unwritten slots
pos 62      0c             bit byte 60: bits 2 and 3 = flag2 present, value true
pos 63..114 00 …
```

---

## 4. Presence

### 4.1 Principle

Absence is **on the wire**: a present `0` is different from absent.

- Every inline field (integer, float, enum, struct) has 1 presence bit; `1` = present.
  This holds for required and optional fields alike.
- `bool` has 2 bits: presence and value. When presence is `0`, the value bit MUST be
  ignored.
- Pointer: `0` = absent. Union: `tag = 0` = absent.
- An all-zero fixed part means "nothing present". The reference writer initializes the
  fixed part with zeros; defaults are applied by the reader and never written.

[vector: `zero_is_present` — byte 2 = `ff` (all 8 scalars present, all with value 0),
byte 62 = `35` = `0b0011_0101`: `flag` present with value false, `flag2` present with
value false, `color` and `point` present; `a_u32` reads as 0, not as the default 7]

### 4.2 Field read rule (3 steps)

To read field `f` of a table whose fixed part has size `fixed_len`, the reader MUST
apply, in this order:

1. **Classify the slot** `[o, o + size)` (fields with a slot: inline, `Ptr`, `Union`),
   testing in this order:
   - `o + size <= fixed_len` ⇒ slot **inside**: go to step 3;
   - otherwise, `o >= fixed_len` ⇒ slot **outside**: the field is absent;
   - otherwise (slot partially covered) ⇒ malformed table: `Truncated`.
2. **`bool`** (no slot): if the bit byte is outside (`p >= fixed_len`), the field is
   absent; otherwise go to step 3.
3. **The presence mark decides**: presence bit (inline and `bool`), pointer `≠ 0`
   (`Ptr`), `tag ≠ 0` (`Union`). A bit whose byte is outside `fixed_len` reads as 0.

The "inside" test comes first. Hence a zero-size slot (empty struct) with
`o == fixed_len` is **inside**, and the presence bit decides. Apart from that case, a
field's bit byte precedes its slot, so a slot inside `fixed_len` implies its bit byte is
inside too.

[vector: `truncated_slot` — the root's `fixed_len` = `42 00` (66): the slot of `point`
(offsets 62..70) is cut in half ⇒ `Truncated`]
[vector: `v1_read_by_v2` — message with `fixed_len = 112` read with `RootV2`: the slot
of `extra` (112..120) is entirely outside ⇒ absent]

### 4.3 Required, optional, default

The wire is identical for all three; the difference lives only in the reader schema.

- **Required** (`T`): if the field is absent by the rule in §4.2, the reader MUST fail
  with `Missing`. For a pointer, "present" means slot inside `fixed_len` and value
  `≠ 0` (the target is validated when the field is read). For a union, `tag ≠ 0`; if
  the tag is `≠ 0` but the pointer is `0`, the field is treated as absent, hence
  `Missing`.
- **Optional** (`T?`): absent is a valid result (`None`; in the vectors' value map, the
  key does not appear).
- **Default** (`T = x`): absent ⇒ the reader returns `x`. A default is never written by
  the writer.
- The required-field check of a table MUST also apply step 1 of §4.2 to **every**
  inline (`Scalar`) slot of the reader schema, required or not: an inline slot
  partially covered ⇒ `Truncated`, even if the field is never accessed. Reserved fields
  are exempt (§4.5).
- The writer does not enforce required fields; a message missing a required field is
  well-formed, and it is the reader that rejects it.

[vector: `minimal` — `a_u32` (`u32 = 7`) absent ⇒ reads as 7]
[vector: `missing_required` — byte 2 = `00`: the presence bit of `a_u8` is cleared
(the value 42 is still in the slot) ⇒ `Missing`]

### 4.4 `bool` in a table

The value of a `bool` with bit `(p, s)`: absent if `p >= fixed_len` or if bit `s` is
0; otherwise, bit `s + 1`.

[vector: `minimal` — byte 62 = `0c`: `flag` (bits 0,1) absent, `flag2` (bits 2,3)
present with value true]

### 4.5 Reserved fields

- A `reserved` field keeps the slot and bits of its original type in the layout.
- A writer MUST NOT write a reserved field (slot and bits stay zero).
- A reader MUST NOT read a reserved field: the content of its slot and bits is ignored
  without error (including a reserved slot partially covered by `fixed_len`), and the
  field never appears in the result.

[vector: `reserved_ignored` — written by an old writer that still had `old @21: u32?`:
byte 62 = `4c` (bit 6 = presence of `old`, plus `flag2`), positions 110..114 =
`ef be ad de`; read with `Root` without error and without the field]

---

## 5. Variable-size data

Variable-size data lives outside the fixed part, at any position in the buffer, reached
through a pointer (§2.2). Every length and count is a `u32`. The reader MUST check that
the header and the data fit in `len` (without arithmetic overflow) and reject with
`OutOfBounds` otherwise.

### 5.1 `string` and `bytes`

```
[u32 len][len bytes]
```

- `string`: the bytes MUST be valid UTF-8; otherwise `InvalidUtf8`.
- `bytes`: any content.
- A present `len = 0` is different from a `0` pointer (absent).

[vector: `v1_read_by_v2` — the `name` pointer at position 72 = `2a 00 00 00` ⇒ 114;
at 114: `02 00 00 00 76 31` = `"v1"`]
[vector: `all_fields` — the `blob` pointer at position 76 = `39 00 00 00` ⇒ 133; at
133: `04 00 00 00 00 01 fe ff`]
[vector: `empty_lists` — `name` ⇒ 114: `00 00 00 00` (empty string, present); `blob` ⇒
118: `00 00 00 00`]
[vector: `string_len_oob` — `len` = `05 00 00 00` with only 4 bytes left ⇒
`OutOfBounds`]
[vector: `bad_utf8` — first byte of the string (position 118) = `ff` ⇒ `InvalidUtf8`]

### 5.2 `[scalar]`, `[enum]`

```
[u32 count][count × size bytes]
```

Elements are packed; `size` is the size of the type (for an enum, its base type).
Unknown enum values in a list are accepted as raw values (§6.1).

[vector: `all_fields` — `nums: [u16]` at position 80 = `3d 00 00 00` ⇒ 141; at 141:
`03 00 00 00  00 00  01 00  ff ff` = `[0, 1, 65535]`]

### 5.3 `[bool]`

Same format as §5.2 with `size = 1`: each element is 1 byte. A writer MUST write `00`
(false) or `01` (true); a reader MUST read `00` as false and any other value as true.

[vector: `bool_struct_and_lists`]

### 5.4 `[struct]`

```
[u32 count][count × struct_size bytes]
```

Each element is the inline struct (§6.3), with no presence bits.

[vector: `all_fields` — `points: [Point]` at position 84 = `43 00 00 00` ⇒ 151; at 151:
`02 00 00 00  01 00 00 00 02 00 00 00  fd ff ff ff fc ff ff ff` =
`[(1, 2), (-3, -4)]`]

### 5.5 `[Table]`

```
[u32 count][u16 elem_fixed_len][count × fixed part of elem_fixed_len bytes]
```

- Elements are inline fixed parts, **without** their own `u16 fixed_len`; element `i`
  starts at `start + 6 + i × elem_fixed_len`.
- `elem_fixed_len` is the size of the fixed part according to the writer; each element
  is read with the rules of §4 using `elem_fixed_len` as `fixed_len`. This keeps the
  stride between elements correct across schema versions.
- `count × elem_fixed_len` (without overflow) MUST fit in `len` from `start + 6`;
  otherwise `OutOfBounds`.
- `elem_fixed_len = 0` is valid.
- Pointers inside an element are relative to their own slot inside the element.

[vector: `all_fields` — `leaves` at position 88 = `53 00 00 00` ⇒ 171; at 171:
`03 00 00 00  0d 00`, then 3 × 13 bytes. Element 0 at 177:
`01  01 00 00 00  22 00 00 00  00 00 00 00` (`n = 1`; the slot of `s` at 182 holds 34 ⇒
216, where `01 00 00 00 61` = `"a"` lives; `next` absent)]
[vectors: `table_list_elem_wider`, `table_list_elem_narrower`, `table_list_elem_zero`]

### 5.6 `[string]`, `[bytes]`

```
[u32 count][count × i32 pointer]
```

- Each pointer is relative to **its own slot** in the list and points to a
  `[u32 len][bytes]` (§5.1).
- An element pointer equal to `0` reads as an **empty** string / bytes (not as absent;
  lists have no absent elements). [vector: `strlist_null_elem`]

[vector: `all_fields` — `names` at position 92 = `9a 00 00 00` ⇒ 246; at 246:
`03 00 00 00  0c 00 00 00  0c 00 00 00  0d 00 00 00`; slot 0 (250) ⇒ 262:
`00 00 00 00` = `""`; slot 1 (254) ⇒ 266: `01 00 00 00 78` = `"x"`; slot 2 (258) ⇒
271: `04 00 00 00 6f 6c c3 a1` = `"olá"`]
[vector: `all_fields` — `blobs` at position 96 ⇒ 279, same format: `[ , ab, 010203]`]

### 5.7 Nested table

A table-typed field is a pointer to `[u16 fixed_len][fixed part]` (§3.1). Recursive
tables (a table that contains, directly or indirectly, a field of its own type) are
allowed.

[vector: `all_fields` — `child` at position 100 = `d3 00 00 00` ⇒ 311; at 311:
`0d 00  01  0a 00 00 00  08 00 00 00  0d 00 00 00` (`n = 10`, `s` ⇒ `"filho"`, `next`
⇒ another `Leaf`, recursively, 2 levels)]

### 5.8 Positions and sharing

- Variable-size data MAY come before or after whatever points to it (forward and
  backward pointers).
- Bytes not referenced by any pointer are allowed and ignored.
- More than one pointer MAY point to the same data. The cost of walking shared data is
  bounded by the `verify` budget (§7).
- Pointers MAY form a cycle; cycles are stopped by the `verify` depth limit (§7).

[vector: `streaming_leaves` — `leaves` written in streaming mode: the strings `"um"`
(114), `"dois"` (120) and a nested `Leaf` (128) come **before** the list header (153:
`03 00 00 00 0d 00`). Element 0 at 159; its `s` slot at 164 holds `ce ff ff ff` (−50)
⇒ 114. The `next` of element 2, slot at 194, holds `be ff ff ff` (−66) ⇒ 128]

---

## 6. Enums, unions and structs

### 6.1 Enum

- An enum has a base type `u8`, `u16` or `u32`, fixed forever; the value is stored as
  that integer.
- A value the reader schema does not know MUST NOT be an error: the reader MUST expose
  it as unknown, preserving the raw value (`Unknown(raw)` in the Rust runtime; a raw
  `UInt` in the vectors).

[vector: `unknown_enum` — byte 62 = `1c` (`flag2` + presence of `color`), byte 63 =
`09`: `color = 9`, outside `Color` ⇒ reads as 9]

### 6.2 Union

- 6-byte slot: `[u16 tag][i32 pointer]`. The pointer is relative to **its own first
  byte** (slot offset + 2), not to the start of the tag.
- Variants are tables only, with tags `≥ 1`, unique within the union. `tag = 0` =
  absent.
- `tag ≠ 0` with pointer `0` ⇒ the field is absent (§4.3).
  [vectors: `union_tag_null_ptr_optional`, `union_tag_null_ptr_required`]
- Known tag: the target is a table `[u16 fixed_len][fixed part]` of the variant's type.
- Tag unknown to the reader schema: MUST NOT be an error. The reader MUST expose the tag
  (`Unknown(tag)`; `{tag, value: {}}` in the vectors) and MUST NOT visit the payload's
  fields or charge it to the budget. The pointer, however, is resolved, and the
  target's header and fixed part are checked against `len` as in §3.1 (`BadOffset` /
  `OutOfBounds`). [vector: `union_unknown_tag_bad_ptr`]

[vector: `all_fields` — `choice` at position 104: `01 00` (tag 1), pointer at 106 =
`03 01 00 00` (259) ⇒ 365: `0d 00 01 14 00 00 00 08 00 00 00 00 00 00 00` (`Leaf`
with `n = 20`, `s = "escolha"`)]
[vector: `unknown_union_tag` — position 104: `05 00  08 00 00 00`: tag 5, pointer at
106 ⇒ 114, where a real `Leaf` lives; reads as `{tag: 5, value: {}}`]

### 6.3 Struct (frozen)

- Fields are packed in declaration order, with no padding and **no** presence bits;
  size = sum of the field sizes.
- Allowed types: integers, floats, `bool`, enums and other structs. A struct MUST NOT
  contain a pointer (string, bytes, list, table, union).
- A zero-size struct (no fields) is allowed in v1. As a table field, its empty slot
  follows §4.2: with `o == fixed_len` it is inside and the presence bit decides.
  [vector: `zero_size_slot_at_end`]
- A `bool` inside a struct takes **1 byte**: a writer MUST write `00` or `01`; a reader
  MUST read `00` as false and any other value as true. [vector: `bool_struct_and_lists`]
- A struct never changes after it is published (§9).
- As a table field, the whole struct is one inline slot with 1 presence bit.

[vector: `all_fields` — `point` at position 64: `ff ff ff ff ff ff ff 7f` =
`(x = -1, y = 2147483647)`]

---

## 7. Verification

### 7.1 Lazy reads and `verify`

The format supports lazy reading: a reader MAY validate only the root (header, fixed
part, required fields) and validate each field when it is accessed. In this mode, every
access MUST perform the checks of §2–§6 and return an error, never undefined behavior.

`verify` walks the **whole** message according to the reader schema. Data that comes
from outside (disk, network) SHOULD go through `verify` before it is trusted; reading
through the envelope (`from_bytes`) MUST perform a full `verify`. Conformance (§10) is
defined over a full read plus `verify`.

### 7.2 Walk

For each visited table (the root, nested tables, union variants with a known tag, and
each element of a `[Table]`):

1. required-field check (§4.3), including `Truncated` for inline slots;
2. enter: depth + 1; if the depth was already `max_depth` ⇒ `DepthExceeded`;
3. charge `2 + fixed_len` to the budget, **except** for `[Table]` elements (charged by
   the list);
4. read each **non-reserved** field of the reader schema (§4.2), validate its data
   (§5, §6) and charge the budget per §7.3; recurse into tables, known union variants
   and `[Table]` elements;
5. leave: depth − 1.

Fields the reader schema does not know (beyond its layout, or the payload of a union
with an unknown tag) MUST NOT be visited. The root has depth 1.

### 7.3 Budget

The budget is a number of "visited" bytes. Each piece of data is charged **every time
it is reached** (shared data is charged once per reference), after its bounds are
checked:

| data | charge |
|---|---|
| table (root, nested, union variant), not a list element | `2 + fixed_len` |
| `string` / `bytes` | `4 + len` (strings are UTF-8 validated first) |
| `[scalar]` / `[enum]` / `[bool]` / `[struct]` | `4 + count × max(size, 1)` |
| `[Table]` | `6 + count × max(elem_fixed_len, 1)` (covers the elements' fixed parts) |
| `[string]` / `[bytes]` | `4 + 4 × count`, plus `4 + len` per element |

A message MUST be rejected with `BudgetExceeded` when the **sum** of all charges of the
full walk exceeds the budget. (The sum does not depend on walk order.) The arithmetic
MUST saturate instead of overflowing.

The order between charging a table and entering it (counting depth) is not normative:
it only decides which of `BudgetExceeded` and `DepthExceeded` is reported when both
limits break in the same step, and error precedence is not normative (§7.5).

### 7.4 Default limits

- `max_depth = 64`;
- `budget = max(len × 8, 1 MiB)` (1 MiB = 1 048 576), where `len` is the size of the
  message (the payload, when there is an envelope).

`verify` and `from_bytes` MUST use these defaults. An API MAY offer explicit limits, but
the vectors assume the defaults.

[vector: `self_cycle` — the `next` of `child` (slot at 125) holds `f5 ff ff ff` (−11)
and points back to `child` itself (114): the walk descends until it passes 64 ⇒
`DepthExceeded`]
[vector: `budget_amplification` — 15 172 bytes, budget = 1 MiB (the floor); 1000
`leaves` elements whose `s` points to the same 2048-byte string: charges ≈
1000 × (4 + 2048) ≈ 2 MiB ⇒ `BudgetExceeded`]

### 7.5 What a reader MUST reject

| error | when |
|---|---|
| `OutOfBounds` | table header (`u16`) or fixed part past `len`; variable-size data header (`u32`, or `u32` + `u16` for `[Table]`) past `len`; `len × 1`, `count × size`, `count × elem_fixed_len` or `count × 4` past `len` (or overflow while computing it); empty buffer or buffer shorter than 2 bytes |
| `Truncated` | slot partially covered by `fixed_len` (§4.2) |
| `BadOffset` | pointer with target `< 0` or `>= len` (§2.2) |
| `InvalidUtf8` | `string` (field or `[string]` element) is not valid UTF-8 |
| `Missing` | required field of the reader schema is absent (§4.3) |
| `DepthExceeded` | table depth exceeds `max_depth` (§7.2) |
| `BudgetExceeded` | sum of charges exceeds the budget (§7.3) |

**Error precedence is not normative.** If a message has more than one defect, a reader
MAY return any of the applicable errors. (That is why each invalid vector has exactly
one defect.) The exception is the envelope check order (§8.4), which is normative.

A reader MUST NOT reject: unreferenced bytes; shared data (within the budget); backward
pointers; fields beyond the reader's layout; unknown enum values or union tags; the
content of reserved fields.

### 7.6 Error vocabulary

The vectors compare only the **name** of the `graven::Error` variant; the data some
variants carry (`field`, `needed`, `expected`/`found`, version) is not normative.

| variant | produced by | when |
|---|---|---|
| `OutOfBounds` | reader | §7.5 |
| `Truncated` | reader | §7.5 |
| `BadOffset` | reader | §7.5 |
| `InvalidUtf8` | reader | §7.5 |
| `Missing { field }` | reader | §7.5 |
| `DepthExceeded` | `verify` | §7.5 |
| `BudgetExceeded` | `verify` | §7.5 |
| `TooLarge` | **writer** | the message (or envelope payload) would exceed `i32::MAX` bytes; never produced by a reader |
| `BadMagic` | envelope | §8.4 step 2 |
| `UnsupportedVersion(v)` | envelope | §8.4 step 3 |
| `BadHeader` | envelope | §8.4 step 4 |
| `Incomplete { needed }` | envelope | §8.4 steps 1 and 5 |
| `ChecksumMismatch` | envelope | §8.4 step 6 |
| `SchemaMismatch { expected, found }` | read through envelope | §8.5 |

---

## 8. Envelope

### 8.1 Layout

An optional layer for disk and network. Messages without an envelope remain valid.

```
offset  size  field
0       4     magic = "GRVN" (47 52 56 4e)
4       1     format_version = 1
5       1     flags = 0
6       2     reserved = 0
8       8     schema_id (u64 LE)
16      4     payload_len (u32 LE)
20      N     payload: one graven message (§2.4), N = payload_len
20+N    4     crc32c (u32 LE) of bytes [0, 20+N)
```

Total size `24 + N`. A writer MUST NOT produce `payload_len > i32::MAX` (`TooLarge`).

[vector: `enveloped_all_fields` — 415 bytes; header
`47 52 56 4e  01  00  00 00  29 31 b4 27 2f ab 22 4c  87 01 00 00`
(`schema_id = 0x4c22ab2f27b43129`, `payload_len = 391`), payload = the bytes of
`all_fields`, trailer `76 50 c4 2a` (CRC `0x2ac45076`)]

### 8.2 `schema_id`

- Identifies the **root type** of the message.
- Value: 64-bit FNV-1a over the UTF-8 bytes of the fully qualified name of the root type
  (`namespace.Type`, e.g. `"conformance.v1.Root"`):

  ```
  h ← 0xcbf29ce484222325
  for each byte b:  h ← (h XOR b) × 0x100000001b3   (mod 2^64)
  ```

- The compiler MUST record the id in the lockfile on first compilation and read it from
  there afterwards, so the id never depends on the current text of the schema. Tables are
  matched by name, so a table cannot be renamed: the compatibility check reports
  `table-removed` (§9). An incompatible change is a new type, with a new name and so a new
  id (for example `OrderV2` next to `Order`); old and new messages never share an id.
- Among the schemas compiled together, each namespace is declared by exactly one file, and
  two tables MUST NOT share an id; the compiler MUST reject either situation.

[vector: `enveloped_all_fields` — `"conformance.v1.Root"` ⇒ `0x4c22ab2f27b43129`
(bytes `29 31 b4 27 2f ab 22 4c`)]
[vector: `env_wrong_schema` — `"conformance.v1.Leaf"` ⇒ `0x289ce2faa6b04709` (bytes
`09 47 b0 a6 fa e2 9c 28`)]

### 8.3 CRC32C

CRC-32C (Castagnoli): polynomial `0x1EDC6F41` (reflected form `0x82F63B78`), initial
value `0xFFFFFFFF`, reflected input and output, final XOR `0xFFFFFFFF`. Check values:
`crc32c("123456789") = 0xE3069283`; `crc32c("") = 0` (unit test in
`crates/graven/src/crc.rs`; not a vector). It covers the header **and** the payload.

[vector: `env_bitflip_payload` — trailer `aa aa 6e ad` = CRC `0xad6eaaaa` of the
original `minimal` envelope; the altered payload no longer matches]

### 8.4 Decoding and check order (normative)

A decoder MUST perform the checks **in this order** and stop at the first error:

1. `input_len < 20` ⇒ `Incomplete { needed: 20 }`;
2. bytes `0..4 ≠ "GRVN"` ⇒ `BadMagic`;
3. byte 4 `≠ 1` ⇒ `UnsupportedVersion(byte 4)`;
4. byte 5 `≠ 0`, or bytes 6..8 `≠ 0` ⇒ `BadHeader`;
5. `input_len < 24 + payload_len` (computed without overflow) ⇒
   `Incomplete { needed: 24 + payload_len }`;
6. CRC32C of `[0, 20 + payload_len)` `≠` the `u32` at
   `[20 + payload_len, 24 + payload_len)` ⇒ `ChecksumMismatch`.

Only then come the `schema_id` check (§8.5) and the payload read. A decoder MUST NOT
panic or read outside its input in any case.

**`Incomplete` ≠ `ChecksumMismatch`:**

- `Incomplete` means "bytes are missing": the input ends before the full header or
  before `24 + payload_len`. It is the normal result of an append-only file cut short
  by a crash; whoever reads the file MAY truncate it there.
- `ChecksumMismatch` means the envelope is complete and the CRC does not match: real
  corruption. It MUST NOT be treated as end of file.
- Because magic, version and flags are checked before the length, a header corrupted in
  those fields never turns into `Incomplete`. A corrupted `payload_len` results in
  `Incomplete` (if it grew past the input) or `ChecksumMismatch`, never in an accepted
  payload. [vectors: `env_payload_len_short_inplace`, `env_payload_len_long`]

| vector | input | error |
|---|---|---|
| `env_cut_header` | 10 bytes `47 52 56 4e 01 00 00 00 29 31` | `Incomplete` (20) |
| `env_bad_magic` | byte 0 `58` (`X`), CRC recomputed | `BadMagic` |
| `env_bad_version` | byte 4 `02`, CRC recomputed | `UnsupportedVersion` |
| `env_bad_header` | byte 5 `01`, CRC recomputed | `BadHeader` |
| `env_cut_payload` | 30 bytes (`payload_len = 114`) | `Incomplete` (138) |
| `env_cut_crc` | 136 bytes (half of the CRC) | `Incomplete` (138) |
| `env_bitflip_payload` | byte 23 (`a_u8`) `2a → 2b` | `ChecksumMismatch` |
| `env_bitflip_crc` | byte 137 `ad → 2d` | `ChecksumMismatch` |

### 8.5 Schema id and reading

After §8.4, a reader that expects root type `R` MUST compare `schema_id` with the id of
`R` and fail with `SchemaMismatch` if they differ; otherwise it MUST read the payload as
a message of `R` with full `verify` (§7), with `len` = `payload_len`. A well-formed
envelope whose payload is not a valid message fails with the message error.

[vector: `env_wrong_schema` — well-formed envelope, valid CRC, id of
`conformance.v1.Leaf`, read as `Root` ⇒ `SchemaMismatch`]
[vector: `env_payload_len_short` — valid CRC, payload shorter than the root's fixed
part ⇒ `OutOfBounds` (§2.4)]

### 8.6 Concatenation

- Bytes after the CRC are not an error: `decode` consumes `24 + payload_len` bytes and
  exposes that length so the next envelope can be read right after it.
- Envelopes MAY be concatenated (batches, append-only files). An iterator MUST stop at
  the first error; an `Incomplete` on the last envelope indicates a cut-off tail.
- Empty input = no envelopes.

[vector: `env_concat`]

---

## 9. Schema compatibility

These rules are normative for compilers. They guarantee that a writer and a reader
using different versions of the same schema understand each other (§4.2 provides the
mechanics: fields beyond `fixed_len` are absent; bytes beyond the reader's layout are
ignored).

The compiler MUST keep a lockfile with the computed layout (per table: IDs, type, size,
slot, bit, required/optional/default, reserved; per enum: base type and values; per
struct: fields and size; per root type: `schema_id`) and, on recompilation, compare the
new schema against the lockfile:

**Error** (the compiler MUST refuse):

- changing the type or size of a field; reusing an ID; reordering IDs; removing a field
  without marking it `reserved`;
- changing the base type of an enum, or changing a `struct`;
- adding a **required field without a default** after the first version (every new
  reader would fail on old messages): a new field is `T?` or has `= x`;
- making an optional field required (no default) (same reason);
- changing a field's default value, or giving a default to an optional field: old
  messages without the field would start reading a different value (silent wrong data);
- changing the value of an enum variant or removing it; removing a union variant or
  pointing its tag at another table (silent wrong data);
- changing the kind of a table, enum, struct or union (e.g. enum ↔ struct of the same
  size); removing a table;
- changing the namespace: it changes the schema id of every table (start a new lockfile on
  purpose if that is really intended).

**Warning** (breaks old readers with an explicit error, never with wrong data; the
compiler MUST require explicit confirmation in the schema, otherwise it is an error):

- marking a field that was required as `reserved`: old readers get `Missing`; readers
  that had it as optional get absent;
- making a required field optional or defaulted: new writers may omit it and old
  readers get `Missing`. Requiring confirmation here also stops required → optional →
  `reserved` from bypassing the previous warning.

A lockfile is only created without comparison for a new schema, on explicit request; a
missing lockfile MUST NOT be implicitly treated as "new schema".

**Allowed:**

- a new optional or defaulted ID at the end;
- `reserved` on an optional or defaulted field;
- default → optional (messages without the field start reading absent, never another
  value);
- renaming a field or an enum variant (names are not in the bytes);
- a new enum or union variant (old readers see an unknown value/tag, §6.1, §6.2).

[vector: `v2_read_by_v1` — written with `RootV2` (`fixed_len = 78 00` = 120; byte 62 =
`8c`, bit 7 = `extra`; `extra` at 114..122 = `88 77 66 55 44 33 22 11`) and read with
`Root`: `extra` ignored, `name` (⇒ 122, `"v1"`) read normally]
[vector: `v1_read_by_v2` — written with `Root`, read with `RootV2`: `extra` absent]
[vector: `reserved_ignored` — field removed and marked `reserved`]

---

## 10. Conformance

### 10.1 Reader

A reader implementation **conforms to format v1** if, for every vector
`testvectors/v1/<name>`, it produces exactly the result in the `.json`:

1. if `enveloped`: decode the envelope (§8.4) and check `schema_id` against the 64-bit
   FNV-1a of `schema` (§8.5); the payload is the message;
2. read the whole message with the reader schema `schema` and verify it (§7, default
   limits);
3. compare with `expect`: the value (encoding described in the
   [vectors README](../conformance-vectors.md); floats by bits) or the **name** of the
   error variant.

The vectors are frozen and normative: if this text and a vector conflict, the vector
wins; the error is in the text, which must be corrected.

### 10.2 Writer and canonical layout

A writer conforms if every message it produces is read by a conforming reader with the
values written. A writer does NOT need to reproduce specific bytes: readers MUST accept
any valid layout (data in any order, backward pointers, unreferenced bytes, shared
data).

The **canonical layout** is that of the reference writer, pinned byte for byte by the
valid vectors `minimal`, `all_fields`, `zero_is_present`, `empty_lists`,
`v2_read_by_v1`, `v1_read_by_v2` and `enveloped_all_fields`. A writer that wants output
identical to the reference (byte-for-byte tests, deduplication by hash) MUST follow:

- root at 0; each table's fixed part has the full `fixed_len` of the writer's layout,
  initialized with zeros; only written fields have non-zero bits/slots;
- variable-size data is appended at the end of the buffer, in the order it is written;
  fields are written in ID order; when writing a nested table, its header and fixed
  part are appended and its fields written (recursively) before the parent's next
  field;
- `[Table]` with a known count: header plus all fixed parts reserved at once, then each
  element's variable-size data, in element order;
- `[string]` / `[bytes]`: header and slots, then each element in order.

Streaming lists (`streaming_leaves`), filled reserved fields (`reserved_ignored`) and
unions with an unknown tag (`unknown_union_tag`) are **valid but non-canonical**
layouts: the vectors pin how to read them, not how to write them.

### 10.3 Vectors added after the first release

The following rules had no vector in the first version of `testvectors/v1/`. The vectors
below were added later (existing implementations already accepted them); no earlier
vector changed:

- `bool` inside a struct and `[bool]` elements (1 byte; non-zero = true) (§5.3, §6.3);
  `[enum]` with an unknown value [vector: `bool_struct_and_lists`];
- `[string]` / `[bytes]` element with pointer `0` ⇒ empty (§5.6)
  [vector: `strlist_null_elem`];
- union with `tag ≠ 0` and pointer `0` ⇒ absent / `Missing` (§4.3, §6.2)
  [vectors: `union_tag_null_ptr_optional`, `union_tag_null_ptr_required`];
- union with an unknown tag whose pointer is invalid ⇒ the pointer's error (§6.2): the
  vector pins `BadOffset` for a pointer one past the end
  [vector: `union_unknown_tag_bad_ptr`];
- reserved slot partially covered by `fixed_len` ⇒ no error (§4.5)
  [vector: `reserved_slot_partial`];
- pointer slot partially covered ⇒ `Truncated` [vector: `ptr_slot_partial`];
- `[Table]` with `elem_fixed_len` larger or smaller than the reader's layout;
  `elem_fixed_len = 0` [vectors: `table_list_elem_wider`, `table_list_elem_narrower`,
  `table_list_elem_zero`];
- zero-size slot (empty struct) with `o == fixed_len` ⇒ inside, the bit decides (§4.2)
  [vector: `zero_size_slot_at_end`];
- envelope concatenation (§8.6) [vector: `env_concat`];
- `payload_len` corrupted in place ⇒ `ChecksumMismatch` (smaller) or `Incomplete`
  (past the buffer) [vectors: `env_payload_len_short_inplace`, `env_payload_len_long`];
  a well-formed envelope (valid CRC) whose payload is shorter than the root's fixed
  part ⇒ the message's error (`OutOfBounds`, §8.5 → §2.4)
  [vector: `env_payload_len_short`].

`TooLarge` is a *writer* rule, not a reader rule: it does not fit in a read vector and
is covered by the runtime tests (`crates/graven/tests/writer_limits.rs`).

---

## Appendix A (informative): frame batches

The Rust runtime also offers batches of messages **without** an envelope
(`Builder::seal` / `take_frames`, read by `graven::frames`): each frame is
`[u32 len LE][message of len bytes]`, concatenated; a cut-off tail produces an
`OutOfBounds` and ends the iteration. This format has no magic, version, schema id or
CRC, has no vectors and is **not** part of the normative v1 format; for data that
leaves the process, use the envelope (§8).

## Appendix B (informative): design notes

Decisions that are easy to miss when reading the normative text:

1. **Envelope, not framing in the message.** The raw message has no magic, version or
   checksum; those live in the optional envelope (§8), so in-memory and in-process uses
   pay nothing for them.
2. **`verify` limits are fixed.** `max_depth = 64`, `budget = max(len × 8, 1 MiB)` and
   the charging rules of §7.3 (including `max(size, 1)` per element, so lists of
   zero-size elements also consume budget) are part of the format, so every conforming
   reader accepts and rejects the same inputs.
3. **No `[[T]]` and no `[Union]` in v1.** They can be added in a later version without
   changing any existing encoding.
4. **`bool` in structs and lists is one byte** (`00`/`01`; reading non-zero = true);
   the 2-bit encoding exists only in table fixed parts, where presence is needed.
5. **Unknown union tags are bounds-checked but not walked** (§6.2): the pointer is
   resolved and the target's header and fixed part are checked against `len`, without
   reading fields or charging the budget.
6. **Union with `tag ≠ 0` and pointer `0` is absent**, and a `0` pointer in a
   `[string]`/`[bytes]` element is empty: there are no "null" list elements.
7. **Truncated inline slots are caught at table level** (§4.3): the required-field check
   applies the slot rule to every inline slot of the reader schema, so a partially
   covered slot fails even if the field is never accessed. Reserved slots are exempt.
8. **Streaming lists** are a writer technique, not part of the format: on the wire they
   are ordinary relative (backward) pointers.
9. **Size limit.** A message is at most `i32::MAX` bytes, enforced by the writer
   (`TooLarge`), so every pointer fits in an `i32`.
10. **Writers do not enforce required fields.** A message missing a required field is
    well-formed; the reader rejects it with `Missing`.
