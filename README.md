# Graven

**Graven** is an open specification for a zero-copy binary serialization format and its
schema language. Messages are read in place, straight from the buffer: no parsing step,
no allocation, no copies. The wire format is **frozen** (v1) and fully specified, with
conformance vectors that any implementation, in any language, can test against.

This repository holds the specification, the conformance vectors and the **reference
implementation in Rust**: a runtime, the `gravenc` compiler and a conformance interpreter.

It is designed for data that lives for years — on disk, in logs, across services that
upgrade at different times — so schema evolution is checked by the compiler, not left
to discipline.

<!-- ANCHOR: highlights -->
## Highlights

- **Zero-copy reads.** Fields are read directly from the bytes; strings and byte slices
  borrow from the buffer.
- **Presence on the wire.** A present `0` is different from an absent field. Fields are
  required, optional (`T?`) or defaulted (`T = x`).
- **Verified once, then trusted.** `verify` walks the whole message with depth and
  budget limits, so hostile input cannot cause unbounded work. After that, a verified
  view reads without re-validating.
- **Schema evolution enforced by the compiler.** A lockfile records every layout; `gravenc`
  refuses changes that would make old and new code disagree (changed types, reused IDs,
  changed defaults, …) and requires explicit confirmation for changes that break old
  readers.
- **Optional envelope for disk and network:** magic, format version, schema id and
  CRC32C. Envelopes can be concatenated into append-only files; a crash-truncated tail
  is distinguished from real corruption.
- **Language-neutral by design:** no alignment, no allocator needed, little-endian
  throughout. The Rust runtime is `no_std` with two dependencies (`bytes`, `crc32c`).
- **Tested hard:** 42 frozen conformance vectors, an independent conformance
  interpreter, mutation gates over every vector, five fuzz targets and Miri runs over
  the runtime and generated code.

<!-- ANCHOR_END: highlights -->

<!-- ANCHOR: quicklook -->
## Quick look

A schema:

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

Writing and reading it from Rust, with the code `gravenc` generates
(`crates/graven-schemas/examples/quickstart.rs`):

```rust
use graven::{Builder, SimplePool};
use graven_schemas::bank::{Currency, Transaction, TransactionW};

fn main() -> Result<(), graven::Error> {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new_enveloped(&pool);

    // Write: one enveloped message (header + schema id + CRC32C).
    let bytes = TransactionW::to_bytes(&mut b, |t| {
        t.id(42).ts(1_700_000_000).currency(Currency::USD).from_shard(3).to_shard(7);
        t.note("rent");
        let mut entries = t.entries(2);
        entries.push().account(1001).amount(-2500).memo("transfer");
        entries.push().account(2002).amount(2500);
    })?;

    // Read: check the envelope and schema id, verify the whole message once,
    // then read fields straight from the buffer, without copying or decoding.
    let tx = Transaction::from_bytes(&bytes)?;
    println!("tx {} in {:?}, note {:?}", tx.id(), tx.currency(), tx.note()?);
    for e in tx.entries()?.iter() {
        let e = e?;
        println!("  account {} amount {} memo {:?}", e.account(), e.amount(), e.memo()?);
    }
    Ok(())
}
```

```sh
cargo run -p graven-schemas --example quickstart
```
<!-- ANCHOR_END: quicklook -->

## Implementations

| Language | Status |
|---|---|
| Rust | Reference implementation (this repository). Passes all v1 vectors. |

A new implementation conforms when it reads every vector in `testvectors/v1/` with exactly
the expected result (spec §10). The lockfile written by `gravenc` records every layout, so a
code generator for another language can start from it.

## Using it in a Rust project

<!-- ANCHOR: usage -->
1. Install the compiler: `cargo install graven-compiler` (provides `gravenc`).
   Add the runtime and the code generator to your crate's `Cargo.toml`:

   ```toml
   [dependencies]
   graven = "0.1"

   [build-dependencies]
   graven-compiler = "0.1"
   ```

2. Write `schemas/bank.graven` and create its lockfile once: `gravenc update schemas/bank.graven --init`.
3. Generate code from `build.rs` (it reads the lock and never writes it):

   ```rust
   fn main() {
       graven_compiler::build::Build::new()
           .schema("schemas/bank.graven")
           .compile()
           .unwrap_or_else(|e| panic!("{e}"));
   }
   ```

4. Include the generated file **alone** in its own module:

   ```rust
   pub mod bank {
       include!(concat!(env!("OUT_DIR"), "/bank.rs"));
   }
   ```

5. When the schema changes, run `gravenc update schemas/bank.graven`: it checks compatibility
   against the lock before rewriting it. In CI, `gravenc check --locked schemas/*.graven`.

<!-- ANCHOR_END: usage -->

The [language guide](docs/graven-language.md) covers the full workflow.

## Documentation

| Document | Content |
|---|---|
| [Format specification v1](docs/spec/graven-format-v1.md) | The normative wire format: layout, presence, variable-size data, verification, envelope, compatibility rules, conformance. |
| [Binary layout](docs/binary-layout.md) | Every kind of value on the wire, byte by byte. |
| [Good practice](docs/good-practice.md) | Advice for designing schemas and storing collections, with the reason behind each tip. |
| [Schema language](docs/graven-language.md) | `.graven` grammar and types, evolution rules, `gravenc` commands, generated API. |
| [Coming from JSON](docs/coming-from-json.md) | Translating JSON data into a schema, piece by piece, with a worked example. |
| [Tools](docs/tools.md) | `gravenc diff`, `inspect`, `to-json`, `from-json`, `scan`, and the VS Code extension. |
| [Conformance vectors](testvectors/v1/README.md) | The 42 frozen vectors and how to check an implementation against them. |
| [The future](docs/future.md) | Where Graven can be applied, and what each area would still need. |
| [Benchmarks](docs/benchmarks.md) | Against FlatBuffers and Cap'n Proto: size, allocations and time. |
| [For AI assistants](docs/for-ai.md) | A compact guide for language models (also published as `llms.txt`). |
| [History](docs/history/index.md) | How the format was designed and benchmarked (not normative). |
| [Testing](docs/testing.md) | Running the test suite, fuzzing and Miri. |
| [Changelog](CHANGELOG.md) | Release history. |

The documentation is also published as a static website, with the landing page in
English, Portuguese, Spanish and Chinese, the documentation pages, the rustdoc API reference
and a download of the vectors:

```sh
pip install -r site/requirements.txt                           # once
scripts/site.sh                                                # writes target/site/
npx wrangler pages deploy target/site --project-name graven   # Cloudflare Pages
```

`target/site/` can also be uploaded by hand in the Cloudflare dashboard
(Workers & Pages → Create → Pages → Upload assets).

## Repository layout

| Path | Content |
|---|---|
| `crates/graven` | Runtime: builder, readers, `verify`, envelope. |
| `crates/graven-layout` | The layout algorithm (spec §3.3). |
| `crates/graven-compiler` | `gravenc`: parser, checker, lockfile, compatibility, Rust code generator, data tools. |
| `crates/graven-dynamic` | Schema-driven reader and writer (schemas as data, no generated code). |
| `crates/graven-conformance` | Conformance schemas, frozen vectors and their generator. |
| `crates/graven-schemas` | Example schemas (`bank`, `game`, `iot`, `stream`) and evolution fixtures. |
| `crates/graven-codegen-tests` | Edge-case schemas for the code generator. |
| `crates/graven-bench` | Benchmarks. |
| `testvectors/v1` | Frozen conformance vectors. |
| `fuzz` | `cargo-fuzz` targets. |
| `editors/vscode` | VS Code extension: syntax highlighting for `.graven` and lockfiles. |
| `site` | Website sources: landing page, translations, documentation renderer. |
| `docs/history` | How we got here: the original designs, plans and benchmark reports (not normative). |

## Status

Format v1 is frozen: the specification and vectors will not change. Any change to the
wire format will be format v2, announced by a new `format_version` in the envelope.
The Rust API is at 0.1 and may still change.

## License

[MIT](LICENSE)
