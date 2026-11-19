# The schema language (`.graven`)

Reference for the language accepted by `gravenc` (crate `graven-compiler`). It is the same
for every language: what each [implementation](implementations.md) generates from a schema
is on its own pages. The binary format is specified normatively in the
[format specification](spec/graven-format-v1.md); this document explains how to write a
schema and how to evolve it.

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
item      = enum | struct | table | union | service ;
enum      = "enum" ident ":" type "{" [ evalue { "," evalue } [","] ] "}" ;
evalue    = ident "=" int ;                    (* decimal or 0x…, unsigned *)
struct    = "struct" ident "{" [ sfield { "," sfield } [","] ] "}" ;
sfield    = ident ":" type ;
table     = { attrs } "table" ident "{" { tfield } "}" ;
tfield    = { attrs } ( "@" int ":" "reserved" [ type ] ";"
                      | ident "@" int ":" type [ "?" ] [ "=" literal ] ";" ) ;
attrs     = "#" "[" attr { "," attr } [","] "]" ;
attr      = ident [ "(" [ arg { "," arg } [","] ] ")" ] ;
arg       = ident [ "=" literal ] ;
union     = "union" ident "{" uvar { "," uvar } [","] "}" ;   (* at least one variant *)
uvar      = ident "@" int ":" type ;
service   = { attrs } "service" ident "{" { mdecl } "}" ;
mdecl     = { attrs } ( "@" int ":" "reserved" ";"
                      | ident "@" int ":" "(" type ")" [ "->" reply ] ";" ) ;
reply     = type | "stream" type | "Result" "<" ( type | "stream" type ) "," type ">" ;
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

## 5. Attributes and validation rules

### Evolution confirmation

`#[allow_breaking_readers]` confirms compatibility warnings:

- on a reserved, optional or defaulted field that used to be required (`required-reserved`, `required-relaxed`),
- on any field whose validation rules tightened or gained a check (`rule-tightened`, `check-added`),
- on a table that gained a table-level `check(name)` (`check-added`).

```graven
table T {
  a @0: u32;
  #[allow_breaking_readers] @1: reserved;   // `b` used to be required
  #[allow_breaking_readers] c @2: u32?;     // `c` used to be required
}
```

The attribute may stay in the schema after `update`; it only has an effect in the step
where the breaking change is introduced.

### Validation rules

Tables and table fields can declare value validation rules without changing the wire format
(see [proposal 0002](proposals/0002-validation.md)):

```graven
enum Status : u8 { Active = 0, Draft = 1 }

#[check(valid_period)]
table Promo {
  #[len(min = 1, max = 64), check(slug)]
  name @0: string;

  #[range(min = 1, max = 10_000_000)]
  price @1: u32;

  #[finite, range(min = 0.0, max = 100.0)]
  discount @2: f32?;

  #[known]
  status @3: Status;

  starts @4: u64;
  ends @5: u64;
}
```

| Rule | Applies to | Meaning |
|---|---|---|
| `range(min = a, max = b)` | integer and float fields | `a ≤ value ≤ b` (inclusive; NaN is outside); either bound may be omitted |
| `len(min = a, max = b)` | `string`, `bytes`, `[T]` | byte length (`string`/`bytes`) or element count (`[T]`); either bound may be omitted |
| `finite` | `f32`, `f64` | not NaN and not infinite |
| `known` | enum fields and `[Enum]` | value is a variant declared in the reader's schema |
| `check(name)` | any field or `table` | calls the application's check `name` with the value (or the table view); fails when it returns a reason code |

Validation never runs implicitly when a message is read or written, so old data stays
readable. Each implementation runs it on request; see [Validation](validation.md).

### Aligned data

`#[align(to = N)]` on a `bytes` field or a list of numbers (`[u8]` … `[u128]`, `[i8]` …
`[i128]`, `[f32]`, `[f64]`) makes the writer place the data at a multiple of `N` (a power of
two from 2 to 4096), counted from the start of the envelope, or of the message when there is
none ([proposal 0006](proposals/0006-rebuild-and-alignment.md)):

```graven
table Samples {
  rate @0: u32;
  #[align(to = 16)]
  values @1: [f32];      // readable as a &[f32] / Float32Array, and by SIMD code
  #[align(to = 4096)]
  page @2: bytes?;       // a whole page, for direct I/O
}
```

The writer inserts zero bytes before the data; readers need nothing new, and the message is
format v1. Adding, changing or removing `align` is compatible (`gravenc diff` lists it as
`align-changed`).

Any other attribute is an error.

## 6. Names

Besides being unique in their scope, names must produce valid, collision-free code in the
reference implementation (Rust), whatever language a project uses: a schema that one
implementation cannot generate would split the ecosystem. `check` rejects (with
`file:line:column`):

- a **type name** (enum, struct, table, union) that does not start with an uppercase
  letter `A–Z`: this rules out keywords, Rust primitives (`str`, `usize`) and
  collisions with the layout module and with variables of the generated code;
- a type whose (snake_case) layout module would be `self`, `super` or `crate` (`Crate`,
  `Super`, `SELF`); other modules that are keywords are emitted raw (`Type` ⇒
  `mod r#type`);
- `self`, `Self`, `super`, `crate` and `_` as any name. Other Rust keywords are accepted
  as fields and variants and are emitted as `r#name`;
- the enum variants `Unknown`, `from_raw` and `to_raw`; the union variant `Unknown`;
- any generated identifier repeated in the same scope:
  - **module:** for table `X`: `X`, `XVerified`, `XW`, `XE`, `XListW` (types and
    constructors), `mod x` (snake_case), `X_REQ`, `X_MASK`, `X_SCHEMA_ID` (UPPER_CASE of
    the snake form); for union `U`: `U`, `UVerified`, `UW`; for service `S`: `SCall`,
    `SClient`, `S_SERVICE_ID`, `mod s_timeouts`; and the reserved name `SCHEMA_ID`.
    E.g. `HTTPServer` and `HttpServer` (both become `http_server`), `Foo` and `FooW`,
    `FOO` and `FOO_REQ`;
  - a table's **layout module:** `FIXED_LEN`, `PTR_SLOTS`, `FIELD` (offset of every
    non-`bool` field) and `FIELD_BIT` (scalars and `bool`). E.g. fields `fixed_len`,
    `ptr_slots`; `x` (scalar) with `x_bit`; `fooBar` with `foo_bar`;
  - **reader methods:** `read`, `verified`, `verified_with`, `from_bytes`, `verify`,
    `SCHEMA_ID` and one accessor per field;
  - **view methods:** `reader` and one accessor per field;
  - **writer methods:** `root`, `to_bytes`, `seal` and: one setter per field; for a list
    (except `[string]`/`[bytes]`) also `<field>_streaming`; for a union, one
    `<field>_<variant_snake>` per variant (and no `<field>`). E.g. `xs: [u32]` with
    `xs_streaming`; `u: U` (variant `Big`) with `u_big`.

`Option`, `String`, `Result`, `FnOnce` and the like are accepted: the generated code
only uses absolute paths (`::core::…`, `::graven::…`). So is a field named `schema_id`
(the method `schema_id()` does not collide with the const `SCHEMA_ID`). Style
recommendation: types and variants in `CamelCase`, fields in `snake_case`. The
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

**Warnings** (an error until confirmed with `#[allow_breaking_readers]` on the field or table):
`required-reserved` (required → reserved), `required-relaxed` (required →
optional/default), `rule-tightened` (a `range`/`len` bound tightened or a new built-in rule
added to an existing field), and `check-added` (a new `check(name)` added to an existing
field or table). Since `required-relaxed` also requires confirmation, reserving in two
steps (required → optional → reserved) does not escape the warning.

**Services** have their own rules (`method-removed`, `method-id-reused`, `method-changed`,
`service-removed` are errors; `method-retired` is a warning); see
[Services](services.md#how-services-evolve).

**Allowed**: a new optional or defaulted field (`new-optional`, `new-default`);
reserving an optional or defaulted field; default → optional; renaming a field; a new
enum or union variant; a new table.

## 9. Workflow: `gravenc check`, `update` and `gen`

The lock only changes through an explicit action: `gravenc update`, or one of the build's
opt-ins in `build.rs`, which apply the same rules. `Build::init_missing_locks()` creates the
first lock of a schema that has none, as `gravenc update --init` does, and
`GRAVEN_UPDATE=1 cargo build` rewrites an out-of-date lock, as `gravenc update` does. Without
them, the build never writes the lock.

```text
gravenc check  [--locked] <schema>...      validate schema(s) and compatibility with <schema>.lock
gravenc update <schema>... [--from <lock> | --init]
                                         apply the compatibility check and write <schema>.lock
gravenc gen    <schema> -o <dir> [--lang rust|js] [--side client|server|client,server]
                                         generate <dir>/<file without .graven>.rs, or .js + .d.ts
                                         (creates <dir>)
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
- `gen`: generates the code of one [implementation](implementations.md) into a directory
  (created if missing): `--lang rust` (the default, the same code as Rust's `build.rs`) or
  `--lang js` (JavaScript with TypeScript declarations). `--side client`, `--side server` or
  `--side client,server` adds the code of `service` declarations; without `--side`, services
  produce no code.
- `diff`, `inspect`, `to-json`, `from-json` and `scan` work with schemas and data files; see
  [gravenc](tools.md).
- Diagnostics go to stderr: `file:line:column: message` (compatibility messages name the
  rule); a confirmed warning is printed as `file:line:column: warning: message` and does
  not fail. Exit codes: 0 ok, 1 error, 2 usage error.

In Rust, `build.rs` generates the code during the build and fails on the same conditions as
`gravenc check --locked`; see [Rust SDK](sdk/rust.md).

Typical cycle: create the `.graven` → `gravenc update --init` once; afterwards, edit the
`.graven` → `gravenc update` (review the findings) → commit schema and lock → build.

## 10. Services

A `service` declares operations: methods with an input, an output, typed errors, streamed
replies, one-way calls and timeouts. [Services](services.md) explains them; the bytes are
[RPC frames v1](spec/graven-rpc-v1.md).

```graven
#[timeout(ms = 5000)]
service Inventory {
  #[timeout(ms = 500)]
  get    @1: (GetProduct)   -> Result<Product, LookupError>;
  list   @2: (ListProducts) -> stream Product;
  adjust @3: (AdjustStock)  -> Result<StockChanged, StockError>;
  ping   @4: (Ping);
  @5: reserved;
}
```

- Method IDs (`@1..=@65535`) are unique in a service and never reused, even after a method is
  retired with `@n: reserved;`.
- Inputs, outputs and errors are local or imported **tables** or **unions**. `stream` appears
  only in the output (`-> stream T`, `-> Result<stream T, E>`); `stream Result<T, E>` is
  refused. A method without `->` is one-way.
- `#[timeout(ms = n)]` (`n >= 1`) sets a default timeout for a `service` or overrides it on a
  method; one-way methods do not accept it.
- The compatibility rules for services are in [Services](services.md#how-services-evolve) and
  in §8.
