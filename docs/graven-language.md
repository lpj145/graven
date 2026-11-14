# The schema language (`.graven`)

Reference for the language accepted by `gravenc` (crate `graven-compiler`) and for the Rust
code it generates. The binary format is specified normatively in the
[format specification](spec/graven-format-v1.md); this document explains how to write a
schema, how to evolve it, and what the generator produces.

```graven
namespace bank;

enum Currency : u16 { BRL = 0, USD = 1, EUR = 2 }

table Entry {
  account @0: u64;
  amount  @1: i64;
  memo    @2: string?;
}

table Transaction {
  id         @0: u64;
  ts         @1: u64;
  currency   @2: Currency;
  from_shard @3: u32;
  to_shard   @4: u32;
  entries    @5: [Entry];
  note       @6: string?;
}
```

## 1. Grammar

```text
schema    = "namespace" path ";" { import } { item } ;
import    = "import" path [ "as" ident ] ";" ;
path      = ident { "." ident } ;
item      = enum | struct | table | union ;
enum      = "enum" ident ":" type "{" [ evalue { "," evalue } [","] ] "}" ;
evalue    = ident "=" int ;                    (* decimal or 0x…, unsigned *)
struct    = "struct" ident "{" [ sfield { "," sfield } [","] ] "}" ;
sfield    = ident ":" type ;
table     = "table" ident "{" { tfield } "}" ;
tfield    = { attr } ( "@" int ":" "reserved" [ type ] ";"
                     | ident "@" int ":" type [ "?" ] [ "=" literal ] ";" ) ;
attr      = "#" "[" ident "]" ;
union     = "union" ident "{" uvar { "," uvar } [","] "}" ;   (* at least one variant *)
uvar      = ident "@" int ":" type ;
type      = ident [ "." ident ] | "[" type "]" ;   (* `name.Type`: an imported type *)
literal   = [ "-" ] int | [ "-" ] float | "true" | "false" | ident ;
ident     = ( letter | "_" ) { letter | digit | "_" } ;   (* ASCII *)
int       = decimal digits | "0x" hex digits ;           (* "_" separates digits *)
float     = digits ( "." digits [ exponent ] | exponent ) ;   (* 1.5, 2e10, 1.0e-3 *)
exponent  = ( "e" | "E" ) [ "+" | "-" ] digits ;
digits    = digit { digit | "_" } ;
```

Comments: `// …` to end of line and `/* … */`. Whitespace is free. Documentation
comments, `/// …`, document the declaration that follows them (see
[Documentation comments](#documentation-comments)). The `namespace` is
part of each table's `schema_id` (`"<namespace>.<Table>"`, spec §8.2).

### Documentation comments

A `///` comment documents the type, field, enum variant, union variant or struct field
right after it. The generated code carries it, so it shows up in the editor and in API
documentation next to the generated reader, verified view, writer, accessors and setters:

```graven
/// A double-entry transaction between two shards.
///
/// The entries of a transaction always sum to zero.
table Transaction {
  /// Unix time, in seconds.
  ts @1: u64;
}
```

- Consecutive `///` lines form one comment; blank `///` lines separate paragraphs, and
  Markdown is passed through.
- A `///` comment with no declaration after it is an error. `////` (four or more slashes)
  is an ordinary comment.
- Documentation is not part of the layout: it never appears in the lockfile, and adding,
  changing or removing it is always compatible. A reserved field generates no code, so its
  documentation is not kept.

## 2. Types

| Type | Where | Notes |
|---|---|---|
| `u8 u16 u32 u64 u128 i8 i16 i32 i64 i128` | field, struct, list | little-endian |
| `f32 f64` | field, struct, list | IEEE-754 |
| `bool` | field, struct, list | in a table it takes only bits (spec §4.4); in a struct or list, 1 byte |
| `string` | field, list | UTF-8, validated by `verify` |
| `bytes` | field, list | |
| `[T]` | field | `T` = scalar, `bool`, enum, struct, `string`, `bytes` or table; no `[[T]]` and no `[Union]` |
| `enum E : u8/u16/u32` | field, struct, list | an unknown value reads as `Unknown(raw)` |
| `struct S` | field, struct, list | **frozen**: packed fields, no padding, never evolves; only scalars, `bool`, enums and structs; no recursion |
| `table T` | field, list, union variant | evolves through IDs; recursion allowed |
| `union U` | field | variants are tables, unique tags `> 0`; an unknown tag reads as `Unknown(tag)` |

The built-in names (`u8`…`f64`, `bool`, `string`, `bytes`) cannot be redeclared.

## 3. Table fields: IDs, presence and defaults

Every field has an ID `@N`. IDs are **contiguous from `@0`**, with no repeats; ID order
defines the layout (spec §3.3), not the order in the text.

| Form | Presence | Reading a message without the field |
|---|---|---|
| `x @0: u32;` | required | `Missing` error (on `read` for scalars; on access/`verify` for pointers) |
| `x @0: u32?;` | optional | `None` |
| `x @0: u32 = 7;` | default | the default |

- Defaults are only allowed on scalars, `bool` or enums (by variant name:
  `color @3: Color = Red;`). `?` and `=` cannot be combined.
- An integer default may be any value of its type, from `i128::MIN` to `u128::MAX`, in
  decimal or `0x` hexadecimal, with `_` between digits.
- A float default is rounded to the nearest value of its type, as in any language
  (`f32 = 0.1` stores the `f32` closest to 0.1). A default that would become infinite, or
  a non-zero literal that would become 0 (`f32 = 1e-50`), is refused instead of being
  changed silently.
- An absent `bool` with a default reads the default; an absent required `bool` is
  `Missing`, like any other scalar.

## 4. Reserved fields

A field that leaves the schema **is not deleted**: it becomes reserved, keeping its ID
and slot.

```graven
table Root {
  a @0: u32;
  @1: reserved u32;   // was `old @1: u32?`
}
```

- The type may be omitted (`@1: reserved;`) when the lock already knows the field: the
  type comes from the lock. If both are present, they must match.
- A reserved field cannot be `?` or have a default; its ID is never reused
  (`id-reused`).
- Reserving a **required** field breaks old readers (they get `Missing`): that is the
  `required-reserved` warning, which requires confirmation with the attribute below.

## 5. Attributes

There is exactly one: `#[allow_breaking_readers]`, on a reserved, optional or defaulted
field (never on a required one). It confirms the `required-reserved` and
`required-relaxed` warnings: the field used to be required and new writers may omit it,
so old readers get `Missing`:

```graven
table T {
  a @0: u32;
  #[allow_breaking_readers] @1: reserved;   // `b` used to be required
  #[allow_breaking_readers] c @2: u32?;     // `c` used to be required
}
```

The attribute may stay in the schema after `update`; it only has an effect in the step
where the field stops being required.

Any other attribute is an error.

## 6. Names

Besides being unique in their scope, names must produce valid, collision-free Rust.
`check` rejects (with `file:line:column`):

- a **type name** (enum, struct, table, union) that does not start with an uppercase
  letter `A–Z` — this rules out keywords, Rust primitives (`str`, `usize`) and
  collisions with the layout module and with variables of the generated code;
- a type whose (snake_case) layout module would be `self`, `super` or `crate` (`Crate`,
  `Super`, `SELF`); other modules that are keywords are emitted raw (`Type` ⇒
  `mod r#type`);
- `self`, `Self`, `super`, `crate` and `_` as any name. Other Rust keywords are accepted
  as fields and variants and are emitted as `r#name`;
- the enum variants `Unknown`, `from_raw` and `to_raw`; the union variant `Unknown`;
- any generated identifier repeated in the same scope:
  - **module** — for table `X`: `X`, `XVerified`, `XW`, `XE`, `XListW` (types and
    constructors), `mod x` (snake_case), `X_REQ`, `X_MASK`, `X_SCHEMA_ID` (UPPER_CASE of
    the snake form); for union `U`: `U`, `UVerified`; and the reserved name `SCHEMA_ID`.
    E.g. `HTTPServer` and `HttpServer` (both become `http_server`), `Foo` and `FooW`,
    `FOO` and `FOO_REQ`;
  - a table's **layout module** — `FIXED_LEN`, `PTR_SLOTS`, `FIELD` (offset of every
    non-`bool` field) and `FIELD_BIT` (scalars and `bool`). E.g. fields `fixed_len`,
    `ptr_slots`; `x` (scalar) with `x_bit`; `fooBar` with `foo_bar`;
  - **reader methods** — `read`, `verified`, `verified_with`, `from_bytes`, `verify`,
    `SCHEMA_ID` and one accessor per field;
  - **view methods** — `reader` and one accessor per field;
  - **writer methods** — `root`, `to_bytes`, `seal` and: one setter per field; for a list
    (except `[string]`/`[bytes]`) also `<field>_streaming`; for a union, one
    `<field>_<variant_snake>` per variant (and no `<field>`). E.g. `xs: [u32]` with
    `xs_streaming`; `u: U` (variant `Big`) with `u_big`.

`Option`, `String`, `Result`, `FnOnce` and the like are accepted: the generated code
only uses absolute paths (`::core::…`, `::graven::…`). So is a field named `schema_id`
(the method `schema_id()` does not collide with the const `SCHEMA_ID`). Style
recommendation: types and variants in `CamelCase`, fields in `snake_case` — the
generator does not rename, and other styles produce lint warnings in the crate that
includes the code.

## 7. Imports

A schema can use the types declared in other schemas by importing their namespace:

```graven
// common.graven
namespace common;

struct Money { cents: i64, currency: u16 }
table Address { street @0: string; city @1: string; }
```

```graven
// orders.graven
namespace orders;

import common;

table Order {
  total   @0: common.Money;
  ship_to @1: common.Address?;
}
```

- An import names a **namespace**, not a file. The compiler finds the file that declares it
  among the schemas compiled together: the ones passed to `build.rs` or `gravenc check`, and
  for single-schema commands, the `.graven` files under the schema's directory. Files can be
  moved or renamed freely.
- **One namespace, one file.** Two files declaring the same namespace in one compilation is
  an error.
- Imported types are always written qualified, by the last segment of their namespace:
  `import acme.common;` → `common.Money`. Unqualified names refer only to the file's own
  types, so adding a type to `common` never changes the meaning of a name elsewhere. When two
  imports would have the same name, give one an alias: `import acme.common as ac;`.
- Any type can be imported: structs, enums, tables and unions. Import cycles are errors.
- An imported table keeps the schema id of the namespace that declares it.
- The importing file's lockfile also records the layout of every imported type it uses, so a
  change to `common` that would break `orders` is refused by `gravenc update orders.graven`
  (rule `import-changed`), and a pull request that changes a shared type shows, in the lock
  diffs, every schema it affects. A compatible change to an imported type (a new optional
  field, a new enum variant) is accepted, and the importer's lock is refreshed with
  `gravenc update`.
- Namespaces carry no version: compatible changes evolve a type in place, and an
  incompatible change is a new type (`OrderV2` next to `Order`), with its own schema id.

## 8. Compatibility (lockfile)

The lock (`<schema>.lock`, deterministic TOML, committed next to the schema) stores the
state that must survive across versions: each table's `schema_id`, the layout (ID, type,
size, slot, bit, presence) and the types of reserved fields, enums, structs and unions.
`gravenc update` compares the new schema against the lock (normative rules in
[spec §9](spec/graven-format-v1.md#9-schema-compatibility)) before rewriting it.

**Errors** (refused):

| Rule | When |
|---|---|
| `type-changed` | a field's type or size changed (or a type changed kind: enum ↔ struct ↔ table ↔ union) |
| `id-reused` | a reserved ID became a field again |
| `ids-reordered` | fields swapped IDs |
| `field-removed` | a field disappeared without becoming `reserved` |
| `enum-base-changed` | an enum's base type changed |
| `struct-changed` | a struct's fields or size changed |
| `new-required` | a new required field without a default |
| `optional-to-required` | an optional/defaulted field became required |
| `table-removed` | a table in the lock disappeared |
| `default-changed` | a field's default value changed (old messages without the field would change meaning) |
| `default-added` | an optional field gained a default (old messages without the field would start reading the default; this also closes `= 5` → `?` → `= 6`) |
| `enum-variant-changed` | an enum variant changed value or was removed (renaming while keeping the value is allowed) |
| `union-variant-changed` | a union variant was removed, or its tag now points to another table |
| `namespace-changed` | the namespace changed, which changes every schema id; if that is intended, start a new lockfile (delete the lock and run `gravenc update --init`) |
| `layout-changed` | defensive: a different slot/bit with the same types (the sequential layout never produces this) |

**Warnings** (an error until confirmed with `#[allow_breaking_readers]` on the field):
`required-reserved` (required → reserved) and `required-relaxed` (required →
optional/default). Since the second one also requires confirmation, reserving in two
steps (required → optional → reserved) does not escape the warning.

**Allowed**: a new optional or defaulted field (`new-optional`, `new-default`);
reserving an optional or defaulted field; default → optional; renaming a field; a new
enum or union variant; a new table.

## 9. Workflow: `gravenc update` vs `build.rs`

The lock only changes through an explicit action; the build never writes it.

```text
gravenc check  [--locked] <schema>...      validate schema(s) and compatibility with <schema>.lock
gravenc update <schema>... [--from <lock> | --init]
                                         apply the compatibility check and write <schema>.lock
gravenc gen    <schema> -o <dir>           generate <dir>/<file without .graven>.rs (creates <dir>)
```

- `check`: parse, check and (if there is a lock) compatibility; writes nothing. Warns
  (without failing) if the lock does not exist or would be updated. With `--locked`,
  both cases are errors (exit 1), as in `build.rs`: this is the CI command
  (`gravenc check --locked schemas/*.graven`), with no compilation.
- `update`: with a lock, only rewrites it if there is no error and no unconfirmed
  warning. Without a lock, it refuses unless given `--init` (a new schema: first lock,
  no history) or `--from` (a moved or renamed schema: compare against the old lock).
  This way a deleted lock or a renamed file does not reset the compatibility check.
  `--from <lock>` compares against another lock (e.g. `v2.graven` against
  `v1.graven.lock`), keeping its `schema_id`s; the lock written is always
  `<schema>.lock` (if the current `<schema>.lock` is unreadable, it is treated as
  missing and overwritten).
- `gen`: the same as `build.rs`, into a directory (created if missing).
- `diff`, `inspect`, `to-json`, `from-json` and `scan` work with schemas and data files; see
  [Tools](tools.md).
- Diagnostics go to stderr: `file:line:column: message` (compatibility messages name the
  rule); a confirmed warning is printed as `file:line:column: warning: message` and does
  not fail. Exit codes: 0 ok, 1 error, 2 usage error.

In the crate that uses the schema:

```rust
// build.rs
fn main() {
    graven_compiler::build::Build::new()
        .schema("schemas/bank.graven")     // reads schemas/bank.graven.lock
        .compile()                       // writes $OUT_DIR/bank.rs
        .unwrap_or_else(|e| panic!("{e}"));
}
```

`compile()` emits `cargo:rerun-if-changed` for the schema and the lock, and fails if:
the lock does not exist (the message points to `gravenc update --init` or `--from`);
regenerating the lock would change something (the schema was edited without
`gravenc update`); compatibility has an error or an unconfirmed warning; the schema does
not pass parse/check. Confirmed warnings become `cargo:warning`.

Typical cycle: create the `.graven` → `gravenc update --init` once; afterwards, edit the
`.graven` → `gravenc update` (review the findings) → commit schema and lock →
`cargo build`.

## 10. Safety: generated code lives alone in its module

**Include each generated file alone in its own module:**

```rust
#![deny(unsafe_code)]   // in the crate (generated code carries #[allow(unsafe_code)] where needed)

pub mod bank {
    include!(concat!(env!("OUT_DIR"), "/bank.rs"));
    // nothing else here
}
```

The view `XVerified<'a>` reads strings without revalidating UTF-8 (`str_unchecked`).
That is sound because it is only constructed after `verify()` has passed. Its tuple
field is private — but privacy in Rust is **per module**: *safe* code written in the
same module as the `include!` could write `XVerified(X::read(buf)?)` without verifying
and, given an invalid string, cause undefined behavior. Outside the module, that
construction does not compile. The header of every generated file repeats this warning.

When schemas import each other, include each generated file in a module named after the
last segment of its namespace, side by side: the generated `orders` code refers to
`super::common::Money`.

```rust
pub mod common { include!(concat!(env!("OUT_DIR"), "/common.rs")); }
pub mod orders { include!(concat!(env!("OUT_DIR"), "/orders.rs")); }
```

## 11. Generated API (summary)

For each **table** `X` (with a field `field`):

- `mod x { FIXED_LEN, FIELD, FIELD_BIT, PTR_SLOTS }` — the layout (spec §3.3);
- `X<'a>` — checked reader: `X::read(buf)`, accessors that validate on demand
  (required scalar ⇒ value; optional ⇒ `Option`; pointer ⇒ `Result<…>`),
  `verify(&mut VerifyCtx)`, `X::verified(buf)` / `X::verified_with(buf, ctx)` (read +
  verify ⇒ view), `X::from_bytes(envelope)` (checks the `schema_id` and verifies),
  `X::SCHEMA_ID`;
- `XVerified<'a>` — verified view: the same accessors, without revalidating UTF-8;
  `reader()` goes back to the reader;
- `XW<'b>` — writer: `XW::root(&mut builder)`, `XW::to_bytes(&mut builder, |w| …)`
  (an enveloped message), `XW::seal(…)` (envelope in a batch), chainable setters
  (`w.id(1).ts(2)`), a table list with a count (`w.entries(n)`) or streaming
  (`w.entries_streaming()`), a nested table (`w.child()`), a union by variant
  (`w.choice_leaf()`);
- `XE` (list element descriptor) and, for a table of scalars only, `XListW`;
- at module level: `X_REQ` and `X_SCHEMA_ID`. There is no module-level `SCHEMA_ID`: the
  "root" depends on who references whom and would silently change when a new table
  referenced the old one; use `X::SCHEMA_ID`.

**Enum** `E`: `enum E { …, Unknown(base) }` with `from_raw`/`to_raw`, `Default` (raw
value 0) and `graven::Le`. **Struct** `S`: `struct S { pub fields }` (`Copy`, `Default`,
`PartialEq`, and `Eq` when there are no floats) with `graven::Le`. **Union** `U`:
`enum U<'a> { Variant(T<'a>), …, Unknown(u16) }` for the reader and
`enum UVerified<'a>` for the view.
