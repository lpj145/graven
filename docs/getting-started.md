# Getting started

Graven schemas (`.graven`) are language-neutral. The `gravenc` compiler validates schemas,
tracks compatibility in `<schema>.lock`, inspects binary files, and generates code.

## 1. Install `gravenc`

```sh
cargo install graven-compiler
```

*(Or from a source checkout: `cargo build --release -p graven-compiler --bin gravenc`.)*

## 2. Write a schema and create its lockfile

Create `schemas/bank.graven`:

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

Initialize its lockfile once:

```sh
gravenc update schemas/bank.graven --init
```

The lockfile (`schemas/bank.graven.lock`) records every table's `schema_id`, fixed-part
layout, defaults, and validation rules. Commit it alongside your `.graven` files.

## 3. Evolve, check, and generate

- **Check in CI:** `gravenc check --locked schemas/*.graven`
- **Update after changes:** `gravenc update schemas/bank.graven` (refuses breaking changes)
- **Generate code from the CLI:** `gravenc gen schemas/bank.graven -o generated/` (pass `--lang js` for JavaScript with TypeScript declarations)
- **Inspect / convert binary data:** `gravenc inspect`, `gravenc to-json`, `gravenc from-json`

Because `<schema>.lock` records the resolved layout and `schema_id`s, generators for other
languages can target the lockfile directly without reimplementing the layout rules.

## Option: Rust project (`Cargo.toml` + `build.rs`)

If you are using Rust, you can run code generation automatically during `cargo build`
instead of invoking `gravenc gen` manually:

{{#include ../README.md:usage}}

## Next steps

- [Coming from JSON](coming-from-json.md) — translate existing JSON data into a Graven schema.
- [Schema language](graven-language.md) — grammar, evolution rules, and the generated API.
- [JavaScript and TypeScript](javascript.md) — `.js` with `.d.ts` declarations, validation, and browser/Node RPC clients.
- [Validation](validation.md) — built-in rules (`range`, `len`, `finite`, `known`) and custom `check(name)` hooks.
- [Tools](tools.md) — `diff`, `inspect`, `to-json`, `from-json`, `scan`, and editor support.
