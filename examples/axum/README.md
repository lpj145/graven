# Graven service on Axum

A small shop service: one schema, a server on [Axum](https://github.com/tokio-rs/axum) and a
JavaScript client. It shows the whole path a product takes, and what Graven leaves to you.

| File | What it is |
|---|---|
| `schemas/shop.graven` | Products, a lookup with a typed error, a streamed list and a validated stock adjustment. |
| `build.rs` | Generates the Rust types, the server edge (`ShopCall`) and the Rust client. |
| `src/main.rs` | The server: one `POST /shop` endpoint, and tests that call it with the Rust client. |
| `web/client.mjs` | The JavaScript client calling the running server. |

## Run it

```sh
cargo test                  # the Rust client against the router, in process
cargo run                   # listens on 127.0.0.1:3001

# in another terminal (Node >= 20)
gravenc gen schemas/shop.graven -o web/gen --lang js --side client
node web/client.mjs
```

This example builds against the crates in this repository. In your own project, depend on
the published crates instead:

```toml
[dependencies]
graven = "0.3"

[build-dependencies]
graven-compiler = "0.3"
```

## How it fits together

- **One endpoint per service.** The call's frame says which method it is for;
  `ShopCall::decode_validated` reads it, verifies the input once, runs the schema's
  validation rules and gives you a typed enum. The `match` is exhaustive: a method added to
  the schema does not compile until it is handled.
- **Typed replies.** Each arm gets a reply builder that only writes what the method
  declares: `reply.ok(...)` the output, `reply.err(...)` one of the declared errors.
- **Verified input, no error handling for reads.** `req.sku()` returns `&str`: the input was
  verified once, so reading it cannot fail.
- **Streams as they are produced.** `reply.frames(rows, |w, row| ...)` encodes one `Item`
  per row and the final `End` lazily; `Body::from_stream` sends each frame as soon as it is
  encoded, and the client reads it as it arrives.
- **Faults are bytes too.** A malformed call, an unknown method or an input that breaks a
  rule gets a `Fault` frame from `Rejected::to_frame()` (nothing for a one-way method). A
  broken rule travels as codes (rule, bounds, reason): the client writes the message in its
  user's language.

## What stays with you

- **HTTP status.** Graven replies, declared errors and faults are all `200 OK` with an
  `application/octet-stream` body; keep 4xx/5xx for what happens before the call is read
  (authentication, limits, routing), so logs and metrics read correctly.
- **TLS.** Frames carry no checksum: the transport keeps the bytes intact.
- **Authentication and authorization.** An Axum middleware before `rpc`.
- **Limits.** `DefaultBodyLimit` bounds a call (here 64 KiB); the JavaScript client bounds a
  reply frame with `maxFrameBytes` (16 MiB by default).
- **Timeouts.** The schema's timeouts are in `shop::shop_timeouts`; enforce them on the server
  with a timeout middleware, and the clients pass them to their send function.
- **Storage.** To keep a message, write it inside an envelope (`XW::to_bytes` with
  `Builder::new_enveloped`); a message received in a frame is stored without re-encoding it.
