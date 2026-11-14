# For AI assistants

This page is written for language models and the people who work with them. It is a
compact, self-contained guide to writing Graven schemas and using the tools correctly.
Linked pages have the details.

Two plain-text files are published with the site for tools that load documentation
directly:

- [`/llms.txt`](../llms.txt) is a short index in the [llms.txt](https://llmstxt.org)
  format: a summary of Graven, then links to every page as Markdown, starting with this guide.
- [`/llms-full.txt`](../llms-full.txt) has the whole documentation, specification included,
  in one file.

Every documentation page is also published as Markdown next to its HTML: replace `.html`
with `.md` in the address.

## What Graven is

Graven is a zero-copy binary serialization format with a schema language. A `.graven`
schema describes messages. The compiler `gravenc` checks it, records its layout in a
lockfile (`<schema>.graven.lock`) and generates code. Messages are read in place from the
buffer, with no parse step. The format (v1) is frozen and specified, with 42 conformance
vectors. The reference implementation is in Rust. Source: <https://github.com/lpj145/graven>.

Use it for data that is stored or exchanged for a long time and must stay readable as the
schema evolves: records, logs, messages between services. Prefer JSON for small,
human-edited configuration.

## Schema cheat sheet

```graven
namespace shop;                         // one per file; part of every schema id

import common;                          // another file's namespace; use as common.Money

/// Doc comments (///) go into the generated code.
enum Status : u8 { Draft = 0, Active = 1, Archived = 2 }   // base: u8, u16 or u32

struct Point { x: i32, y: i32 }         // inline and frozen: can never change

table Product {
  sku    @0: string;                    // required
  name   @1: string;
  price  @2: common.Money;              // imported struct
  status @3: Status = Active;           // default: absent reads Active
  tags   @4: [string]?;                 // optional list
  stock  @5: u32?;                      // optional scalar: absent ≠ 0
  @6: reserved u64;                     // a removed field: ID and slot kept forever
}

table Catalog {
  products @0: [Product];               // a list of tables
}

table Card { last4 @0: u16; }
table Pix  { key @0: string; }
union Method { Card @1: Card, Pix @2: Pix }   // variants are tables, tags > 0
```

Types: `u8`–`u128`, `i8`–`i128`, `f32`, `f64`, `bool`, `string` (UTF-8), `bytes`, `[T]`,
enums, structs, tables and unions. Field forms: `x @N: T;` (required), `x @N: T?;`
(optional), `x @N: T = literal;` (default, only for scalars, `bool` and enums),
`@N: reserved [T];`.

## Rules to follow

Write schemas that `gravenc` accepts the first time:

1. Field IDs in a table are contiguous from `@0`, with no gaps or repeats. Layout follows ID
   order, not text order.
2. Never renumber, reuse or delete an ID. A field that is no longer used becomes
   `@N: reserved;`.
3. Never change a field's type, an enum's base, a struct, or a default value. These
   are refused.
4. A new field in an existing table must be optional or have a default. A new required
   field is refused (`new-required`).
5. Making a required field optional or reserved breaks deployed readers. The compiler asks
   for `#[allow_breaking_readers]` on that field. Add the attribute only when the user
   explicitly accepts the break.
6. An incompatible change is a new type (`OrderV2`), not a new namespace or version
   suffix on the namespace.
7. `?` and `=` cannot be combined. Defaults are only for scalars, `bool` and enums
   (enums by variant name).
8. No lists of lists (`[[T]]`), no lists of unions, no struct that contains itself.
   Recursion is allowed only through tables.
9. Union variants are tables, with unique tags greater than 0. Tag 0 means "no value".
10. One namespace per file, and two files cannot share one. Imported types are always
    qualified by the last segment of their namespace (`common.Uuid`), or by an alias
    (`import acme.common as ac;`).

Renaming a field, adding an enum variant, adding a union variant and adding a table are
all allowed.

Full grammar and rules: [schema language](graven-language.md). Normative compatibility
rules: [specification §9](spec/graven-format-v1.md#9-schema-compatibility).

## Commands

```sh
gravenc update schema.graven --init        # first lockfile of a new schema
gravenc update schema.graven               # after a change: check compatibility, rewrite the lock
gravenc check --locked schemas/*.graven    # CI: fail if a lock is missing or out of date
gravenc from-json schema.graven --root Product in.json -o out.bin
gravenc to-json schema.graven out.bin      # one JSON document per message
gravenc inspect schema.graven out.bin      # decode and verify, with offsets
gravenc diff schema.graven                 # every change against the lock, with its verdict
gravenc scan log.bin --schema schema.graven   # find damaged or truncated records
```

Only `gravenc update` writes the lockfile. `build.rs` reads it and fails if it is missing
or out of date. Always commit the lockfile with the schema. Details: [tools](tools.md).

## Using it from Rust

Install the compiler with `cargo install graven-compiler`, and add to
`Cargo.toml`:

```toml
[dependencies]
graven = "0.1"

[build-dependencies]
graven-compiler = "0.1"
```

`build.rs`:

```rust
fn main() {
    graven_compiler::build::Build::new()
        .schema("schemas/common.graven")   // every file an import needs
        .schema("schemas/shop.graven")
        .compile()
        .unwrap_or_else(|e| panic!("{e}"));
}
```

Include each generated file alone in its own module:

```rust
pub mod common {
    include!(concat!(env!("OUT_DIR"), "/common.rs"));
}
pub mod shop {
    include!(concat!(env!("OUT_DIR"), "/shop.rs"));
}
```

Write and read an enveloped message:

```rust
use graven::{Builder, SimplePool};
use shop::{Catalog, CatalogW};

fn main() -> Result<(), graven::Error> {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new_enveloped(&pool);
    let bytes = CatalogW::to_bytes(&mut b, |c| {
        let mut products = c.products(1);       // a list with a known length
        products
            .push()
            .sku("CF-01")
            .name("Coffee")
            .price(common::Money { cents: 8990, currency: 986 })
            .stock(120);
    })?;

    let catalog = Catalog::from_bytes(&bytes)?; // checks the envelope and verifies
    for p in catalog.products()?.iter() {
        let p = p?;
        println!("{} {:?}", p.sku()?, p.stock());
    }
    Ok(())
}
```

The generated API for a table `X`:
- `X::read(buf)` is a checked reader.
- `X::verified(buf)` verifies a bare message and returns a verified view.
- `X::from_bytes(buf)` does the same for an enveloped message, after checking its schema
  id and CRC.
- `XW` is the writer, with chainable setters, `XW::to_bytes` and `XW::seal`.
- Required scalars return the value, optional fields return an `Option`, and strings,
  lists and nested tables return a `Result`.

Details: [generated API](graven-language.md#11-generated-api-summary).

## JSON mapping

The tools and JSON conversion use this mapping:
- Integers are numbers. `u128`/`i128` values outside 64 bits are decimal strings.
- Floats are numbers, or `"NaN"`, `"inf"` and `"-inf"`.
- An enum is its variant name, or a number for an unknown value.
- `bytes` are a lowercase hex string.
- A union is `{"Variant": {…}}`.
- An absent optional field is left out. `null` on input means absent.

Translating JSON data into a schema: [coming from JSON](coming-from-json.md).

## Mistakes to avoid

- **Deleting a field or renumbering IDs to "clean up".** Reserve the field instead.
- **Treating `0` or `""` as absent.** Graven stores presence explicitly. Use `T?` when
  "not sent" is a distinct state.
- **Using `f64` for money.** Use an integer of minor units and a currency code.
- **Using a struct for something that may gain fields.** Structs are frozen; use a table.
- **Editing the lockfile by hand,** or regenerating it with `--init` to get past an error.
  That throws away the history that protects old data. Fix the schema instead.
- **Decoding untrusted bytes without verification.** Use `from_bytes` or `verified`.
- **Wrapping every record of a log in one big list.** Write one enveloped message per
  record. Wrap a list in a table only when it is read and written as a unit.

More: [good practice](good-practice.md).
