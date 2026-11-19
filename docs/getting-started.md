# Getting started

A `.graven` file defines your data and services once. The `gravenc` compiler checks it,
records layouts in a `.graven.lock` file, inspects binary files, and generates code for each
supported language.

## 1. Install `gravenc`

```sh
cargo install graven-compiler
```

*(Or from a source checkout: `cargo build --release -p graven-compiler --bin gravenc`.)*

## 2. Define types and initialize the `.lock` file

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

> [!NOTE]
> The lockfile (`schemas/bank.graven.lock`) records every table's `schema_id`, fixed-part
> layout, defaults, and validation rules. Commit it alongside your `.graven` files so
> compatibility checks protect old readers.

## 3. Generate and use in your language

<!-- tabs -->
#### Rust

Add `graven` and `graven-compiler` to `Cargo.toml`, generate in `build.rs` (or run
`gravenc gen schemas/bank.graven -o src/gen`), and read/write zero-copy:

```rust
// build.rs
fn main() {
    graven_compiler::build::Build::new()
        .schema("schemas/bank.graven")
        .compile()
        .unwrap_or_else(|e| panic!("{e}"));
}
```

```rust
pub mod bank {
    include!(concat!(env!("OUT_DIR"), "/bank.rs"));
}

use graven::{Builder, LocalPool};
use bank::{Currency, Transaction, TransactionW};

let mut b = Builder::new_enveloped(&LocalPool);
let bytes = TransactionW::to_bytes(&mut b, |t| {
    t.id(42).ts(1_700_000_000).currency(Currency::USD).from_shard(3).to_shard(7);
    let mut entries = t.entries(1);
    entries.push().account(1001).amount(-2500).memo("transfer");
})?;

let tx = Transaction::from_bytes(&bytes)?;
println!("tx {} ({:?})", tx.id(), tx.currency());
```

See the full [Rust SDK](sdk/rust.md).

#### JavaScript / TypeScript

Generate ES2022 `.js` + `.d.ts` files (zero dependencies, runs in browsers, Node, and Deno):

```sh
gravenc gen schemas/bank.graven -o src/gen --lang js
```

```js
import { Currency, Transaction, TransactionW } from "./gen/bank.js";

const bytes = TransactionW.toBytes((t) => {
  t.id(42n).ts(1_700_000_000n).currency(Currency.USD).from_shard(3).to_shard(7);
  const entries = t.entries(1);
  entries.push().account(1001n).amount(-2500n).memo("transfer");
  entries.close();
});

const tx = Transaction.fromBytes(bytes);
console.log(`tx ${tx.id()} (${Currency.name(tx.currency())})`);
```

See the full [JavaScript / TypeScript SDK](sdk/js.md).
<!-- /tabs -->

All implementations read and write the exact same bytes, so a message written in Rust can be
read directly in JavaScript/TypeScript and vice versa.

## 4. Evolve and check

- **After an edit:** `gravenc update schemas/bank.graven` checks compatibility against the
  `.lock` file and rejects breaking changes ([compatibility rules](graven-language.md#8-compatibility-lockfile)).
- **In CI:** `gravenc check --locked schemas/*.graven` fails if a `.lock` file is missing or stale.
- **Review a change:** `gravenc diff schemas/bank.graven` lists every change with its verdict.
- **Inspect or convert data:** `gravenc inspect`, `gravenc to-json`, and `gravenc from-json`
  work directly from the `.graven` file ([gravenc](tools.md)).

## Next steps

- [Coming from JSON](coming-from-json.md): translate JSON payloads into `.graven` types.
- [Schema language](graven-language.md): types, IDs, presence, imports, and evolution rules.
- [Validation](validation.md): value rules and custom `check(name)` hooks.
- [Services](services.md): RPC services, streaming, and typed errors.
- [Implementations](implementations.md): supported runtimes and adding new languages.
