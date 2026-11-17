# Rust SDK

Rust is the reference implementation of Graven. The `graven` crate provides the zero-copy runtime (supporting `no_std` + `alloc` with two dependencies), and `graven-compiler` generates code via `build.rs` or `gravenc gen`.

## Setup

Add the runtime and compiler to `Cargo.toml`:

```toml
[dependencies]
graven = "0.3"

[build-dependencies]
graven-compiler = "0.3"
```

Install the `gravenc` CLI to manage lockfiles and inspect binary data:

```sh
cargo install graven-compiler
```

Generate Rust code automatically in `build.rs`:

```rust
fn main() {
    graven_compiler::build::Build::new()
        .schema("schemas/bank.graven") // reads schemas/bank.graven.lock
        .compile()                     // writes $OUT_DIR/bank.rs
        .unwrap_or_else(|e| panic!("{e}"));
}
```

> [!NOTE]
> `Build::compile()` reads the lockfile (`schemas/bank.graven.lock`) and **never writes it**. It emits `cargo:rerun-if-changed` for both files and fails if the lock is missing, out of date, or has an error/unconfirmed warning. Add `.client()` and `.server()` to emit `service` code. Without `build.rs`, `gravenc gen schemas/bank.graven -o src/gen` writes the same files.

Include each generated file inside its own empty module:

```rust
#![deny(unsafe_code)] // generated files carry #[allow(unsafe_code)] where needed

pub mod bank {
    include!(concat!(env!("OUT_DIR"), "/bank.rs"));
    // keep this module exclusively for the generated file
}
```

> [!IMPORTANT]
> `XVerified<'a>` skips UTF-8 revalidation on strings because it can only be built by `verify()`. In Rust, module privacy allows sibling code in the same module to construct private fields directly — keep the `include!` isolated so sound guarantees hold across the crate.

When schemas import each other, place sibling modules side by side named after the last segment of their namespace (`orders` refers to `super::common::Money`):

```rust
pub mod common { include!(concat!(env!("OUT_DIR"), "/common.rs")); }
pub mod orders { include!(concat!(env!("OUT_DIR"), "/orders.rs")); }
```

## Reading and writing

Using `schemas/bank.graven` from [Getting started](../getting-started.md):

```rust
use graven::{Builder, SimplePool};
use bank::{Currency, Transaction, TransactionW};

fn main() -> Result<(), graven::Error> {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new_enveloped(&pool);

    let bytes = TransactionW::to_bytes(&mut b, |t| {
        t.id(42).ts(1_700_000_000).currency(Currency::USD).from_shard(3).to_shard(7);
        t.note("rent");
        let mut entries = t.entries(2);
        entries.push().account(1001).amount(-2500).memo("transfer");
        entries.push().account(2002).amount(2500);
    })?;

    let tx = Transaction::from_bytes(&bytes)?;
    println!("tx {} in {:?}, note {:?}", tx.id(), tx.currency(), tx.note());
    for e in tx.entries().iter() {
        println!("  account {} amount {} memo {:?}", e.account(), e.amount(), e.memo());
    }
    Ok(())
}
```

- **Untrusted input:** `X::from_bytes(env)` checks envelope magic, `schema_id`, and CRC32C, runs `verify()`, and returns `XVerified<'a>`. `X::verified(buf)` / `X::verified_with(buf, ctx)` do the same for bare payloads.
- **Lazy checked reads:** `X::read(buf)` returns a checked reader without upfront traversal (cheaper when reading 1–2 fields).
- **Return types:** on a verified view (`from_bytes`, `verified`), every accessor returns the value itself: `T` for a required field, `Option<T>` for an optional one, `&str`, `&[u8]`, lists and nested views included. The message was verified once, so reading it cannot fail. On a checked reader (`read`), strings, bytes, lists and nested tables return `Result<_, graven::Error>`, because nothing was verified yet. Enums decode to `E::Variant` or `E::Unknown(raw)`; unions decode to `U::Variant(view)` or `U::Unknown(tag)`. Lists of a verified view iterate over their elements directly, and `get(i)` returns `None` past the end.
- **Writing:** `XW::to_bytes(&mut builder, |w| ...)` emits a single enveloped message; `XW::seal(&mut builder, |w| ...)` appends into a batch collected by `builder.take_frames()`. Use `w.entries(n)` for known-length lists or `w.entries_streaming()` when streaming elements without knowing the count upfront, and `SimplePool` to amortize allocations to zero in steady state.

## Validation

When a schema declares validation rules (`#[range]`, `#[len]`, `#[finite]`, `#[known]`, `#[check(name)]`), the generator emits a `Checks` trait (if only built-in rules exist, `Checks` is implemented for `()`, so `&()` works):

```rust
use shop::{Checks, Promo, PromoVerified, PromoW};

struct AppChecks;

// Ok(()) accepts the value; Err(code) rejects it with a reason code.
impl Checks for AppChecks {
    fn email(&self, v: &str) -> Result<(), &'static str> {
        if v.contains('@') { Ok(()) } else { Err("missing_at") }
    }
    fn slug(&self, v: &str) -> Result<(), &'static str> {
        if v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { Ok(()) } else { Err("bad_char") }
    }
    fn valid_period(&self, p: &PromoVerified<'_>) -> Result<(), &'static str> {
        if p.starts() < p.ends() { Ok(()) } else { Err("ends_before_start") }
    }
}

// Validate at write time:
let bytes = PromoW::to_bytes_validated(&mut builder, &AppChecks, |p| {
    p.name("summer-sale").contact("hi@example.com").starts(10).ends(20);
})?;

// Validate after opening:
let view = Promo::from_bytes(&bytes)?;
view.validate(&AppChecks)?;               // stops at first failure (no allocation)
let all = view.validate_all(&AppChecks);  // every failure: Vec<graven::Invalid>
```

A `graven::Invalid` has `table`, `field`, `index` and `rule`. The `Rule` carries codes for
the application to turn into text in its user's language: `Rule::Range { min, max }` and
`Rule::Len { min, max }` hold the declared bounds as decimal text, and
`Rule::Check { name, reason }` the code the check returned. `Display` writes an English
diagnostic for logs (`Signup.age: breaks rule `range` (min 18, max 120)`).

For runtime schemas loaded without generated code, `graven_dynamic::Validator::new(root, schema, registry)` verifies at construction time that every reachable `check(name)` is registered in `CheckRegistry`; a registered check returns `Ok(())` or `Err(code)`, like the generated trait.

## Services

Enable client and server code generation with `Build::client().server()` or `gravenc gen --side client,server`:

- **Server edge (`no_std` compatible, transport-agnostic):** `<Service>Call::decode(frame)` checks the frame header, service id and method id, and runs `verify()` on the input (`decode_validated(frame, &checks)` also validates input rules, rejecting with an `Invalid` fault frame). Each arm receives the verified input and a typed `Reply<O, E>` (`ok`, `err`, `fault`) or `StreamReply<O, E>` (`item`, `end`, `err`, `fault`, and `frames(items, |w, item| …)`, the whole stream as a lazy iterator of frames). `Rejected::to_frame()` builds the protocol `Fault` frame (`None` for one-way calls).
- **Client:** Implement `graven::rpc::Sender` (`async fn send(&self, call: Vec<u8>, timeout_ms: Option<u32>)`) — and optionally `send_stream` to stream frames incrementally via `ReplyStream`. Wrap it in `<Service>Client::new(sender)`.
- **Errors & timeouts:** `<SERVICE>_SERVICE_ID` and `<service>_timeouts::<METHOD>_MS` expose the service id and the declared timeouts; each method also gets `<method>_with_timeout(f, Some(ms))`. Call errors arrive as `CallError` (`Err`, `Fault(OwnedFault)`, `Protocol(ProtocolError)`, or `Send(T::Error)`).

### Serving over HTTP (Axum)

One endpoint per service; the frame says which method a call is for. A complete, tested
example (schema, server, Rust and JavaScript clients) is in
[`examples/axum`](https://github.com/lpj145/graven/tree/main/examples/axum).

```rust
async fn rpc(State(db): State<Db>, body: Bytes) -> Response {
    let call = match ShopCall::decode_validated(&body, &()) {
        Ok(call) => call,
        Err(rejected) => return frames(rejected.to_frame().unwrap_or_default()),
    };
    match call {
        ShopCall::Get(req, reply) => {
            let sku = req.sku(); // verified input: no `?`
            frames(match db.read().unwrap().get(sku) {
                Some(p) => reply.ok(|w| { w.sku(sku).name(&p.name).stock(p.stock); }),
                None => reply.err(|e| { e.not_found().sku(sku); }),
            })
        }
        ShopCall::List(req, reply) => {
            let rows = /* read what the stream needs; `req` borrows the body */;
            let items = reply.frames(rows, |w, (sku, p)| { w.sku(&sku).name(&p.name).stock(p.stock); });
            frames(Body::from_stream(stream::iter(items.map(|f| Ok::<_, Infallible>(Bytes::from(f))))))
        }
        // …one arm per method: the match does not compile until every method is handled.
    }
}

fn frames(body: impl Into<Body>) -> Response {
    ([(header::CONTENT_TYPE, "application/octet-stream")], body.into()).into_response()
}

// Router::new().route("/shop", post(rpc)).layer(DefaultBodyLimit::max(64 * 1024))
```

- **Always `200 OK`.** A reply, a declared error and a fault all travel in the body. Keep
  HTTP statuses for what happens before the call is read (authentication, limits, routing),
  so logs and metrics read correctly.
- **Limits.** `DefaultBodyLimit` bounds a call; clients bound a reply frame
  (`maxFrameBytes` in JavaScript).
- **TLS** is required: frames carry no checksum. Authentication is a middleware before
  the handler; timeouts a timeout middleware, with the schema's values in
  `<service>_timeouts`.

## API reference

The full rustdoc API documentation is published alongside this site:

| Crate | Purpose |
|---|---|
| [`graven`](../../api/graven/index.html) | Runtime: `Builder`, `TableRef`, `VerifyCtx`, envelope, validation (`Invalid`, `Rule`), and `rpc` (`Reply`, `Sender`, `ReplyStream`, `Fault`). |
| [`graven-compiler`](../../api/graven_compiler/index.html) | `gravenc` CLI and `build::Build` API, parser, checker, lockfile, compatibility rules. |
| [`graven-dynamic`](../../api/graven_dynamic/index.html) | Dynamic runtime reading, writing, and `Validator` from a schema at run time. |
| [`graven-layout`](../../api/graven_layout/index.html) | Wire layout computation of [format v1 §3.3](../spec/graven-format-v1.md). |

### Generated items summary

- **Table `X`:** `X<'a>` (`read`, `verified`, `verified_with`, `from_bytes`, `SCHEMA_ID`, `verify`), `XVerified<'a>` (`reader()`, unchecked-UTF8 accessors, `validate`, `validate_all`), `XW<'b>` (`root`, `to_bytes`, `to_bytes_validated`, `seal`, chainable setters, `<list>_streaming()`, `<union>_<variant>()`), layout module `mod x`, `XE`, `XListW`, `X_REQ`, `X_MASK`, `X_SCHEMA_ID`.
- **Enum `E`:** `enum E { ..., Unknown(base) }` (`from_raw`, `to_raw`, `Default`, `graven::Le`).
- **Struct `S`:** `struct S { pub ... }` (`Copy`, `Default`, `PartialEq`, `Eq` if no floats, `graven::Le`).
- **Union `U`:** `enum U<'a>` and `enum UVerified<'a>` (`Variant(view)` / `Unknown(u16)`), plus `UW` writer when services are enabled.
- **Validation & services:** `pub trait Checks`, `<Service>Call<'a>`, `<Service>Client<T>`, `<SERVICE>_SERVICE_ID`, `<service>_timeouts`.

Collision and naming rules accepted by `gravenc check` are documented in [Schema language §6](../graven-language.md#6-names).
