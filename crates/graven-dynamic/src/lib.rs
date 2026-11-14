//! Schema-driven reader and writer for the Graven wire format.
//!
//! Where generated code knows a schema at compile time, this crate takes the schema as
//! data ([`model`]): [`read`] decodes a whole message into a [`Value`] and [`write`](fn@write)
//! encodes one. It is the second, independent implementation used by the conformance
//! tests, and the engine behind `gravenc inspect`, `scan`, `to-json` and `from-json`.
//! Reading uses only the public `graven::TableRef`/list API and writing only
//! `graven::TableW`.
//!
//! # Reader semantics
//!
//! [`read`] behaves like a generated `Root::read(buf)` followed by
//! `verify(&mut VerifyCtx::new(buf.len()))` (it accepts and rejects the same messages; with
//! more than one defect the error returned may differ, since the generated `verify` checks
//! leaf data before descending and precedence is not normative, spec §7.5). For each
//! table, in this order:
//!
//! 1. required-field check over the fields in ID order (`docs/spec/graven-format-v1.md` §4.3): a required field
//!    that is absent ⇒ `Missing { field }`; any scalar slot (required or not) only
//!    partially covered by `fixed_len` ⇒ `Truncated`; a required pointer is present when
//!    its slot is inside `fixed_len` and non-zero; a required union when its tag is
//!    non-zero;
//! 2. `ctx.enter()?` (depth limit ⇒ `DepthExceeded`);
//! 3. `ctx.charge(2 + fixed_len)` — except list elements, which their list charges;
//! 4. every non-reserved field in ID order (reserved fields are never read).
//!
//! Charges (budget exhausted ⇒ `BudgetExceeded`), each after the data was bounds-checked:
//!
//! | data | charge |
//! |---|---|
//! | table (not a list element) | `2 + fixed_len` |
//! | `string` / `bytes` | `4 + len` (strings are validated as UTF-8 first) |
//! | `[scalar]` / `[enum]` / `[struct]` | `4 + count × max(size, 1)` |
//! | `[table]` | `6 + count × max(elem_fixed_len, 1)` (like `ListR::verify`) |
//! | `[string]` / `[bytes]` | `4 + 4 × count`, then `4 + len` per element |
//!
//! Resulting map: an absent optional field is **not** in the map; an absent field with a
//! default (`= x`) is in the map with the default; enums are `UInt(raw)` (unknown values
//! included); unsigned integers are `UInt`, signed ones `Int`; structs are `Table`s keyed
//! by field name; a known union tag is `Union { tag, value: Table(..) }`, an unknown one
//! `Union { tag, value: Table({}) }` (the payload is not visited).
//!
//! # Writer
//!
//! [`write`](fn@write) writes the fields present in the map in ID order (fields absent from the map
//! are not written). Struct inline bools are 1 byte (0/1). See [`write`](fn@write) for the panics on
//! values that do not match the schema. `[string]`/`[bytes]` use the forward layout of
//! `TableW::set_strs`/`set_bytes_list` (header, then elements in order).
//!
//! # JSON representation of [`Value`]
//!
//! serde's default externally tagged enum: every value is a one-key object naming its
//! variant. Exact representations are used where JSON numbers are not exact:
//!
//! | `Value` | JSON |
//! |---|---|
//! | `Bool(true)` | `{"Bool": true}` |
//! | `UInt(7)` | `{"UInt": "7"}` (decimal string, any `u128`) |
//! | `Int(-7)` | `{"Int": "-7"}` (decimal string, any `i128`) |
//! | `F32(1.5)` | `{"F32": "f32:0x3fc00000"}` (IEEE-754 bits, 8 lowercase hex digits) |
//! | `F64(-0.0)` | `{"F64": "f64:0x8000000000000000"}` (bits, 16 lowercase hex digits) |
//! | `Str("oi")` | `{"Str": "oi"}` |
//! | `Bytes([0, 171])` | `{"Bytes": "00ab"}` (lowercase hex, 2 digits per byte) |
//! | `List([..])` | `{"List": [ .. ]}` |
//! | `Table({..})` | `{"Table": {"field": .., ..}}` (keys sorted) |
//! | `Union { tag: 1, value }` | `{"Union": {"tag": 1, "value": {"Table": {..}}}}` |
//!
//! `Value` equality is exact: floats compare by bits (`NaN` payloads matter, `0.0 != -0.0`).
pub mod interp;
pub mod model;

pub use interp::{Value, field_kind, layout, read, write};
pub use model::*;
