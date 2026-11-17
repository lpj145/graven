# Changelog

## 0.3.3

- **Rust: `LocalPool`,** a buffer pool with one free list per thread: no lock and nothing to
  set up (`Builder::new(&LocalPool)`). It keeps at most four 4 KiB chunks per thread and
  drops buffers with more than 64 KiB free. Generated RPC code (calls, replies, faults) now
  writes through it instead of starting from an empty buffer every time. Writing a small
  message: 48 → 34 ns on one thread; on 8 threads, 85–120 ns without a pool, 3.7 µs with
  one shared `SimplePool`, 50 ns with `LocalPool`. Needs the `std` feature (on by default);
  without it, generated code falls back to a fresh buffer as before. The examples and docs
  use `LocalPool`; `SimplePool` stays.

## 0.3.2

- **JavaScript: streamed replies are faster, and can be read in batches.** A stream method
  returns an `ItemStream<T>`: still an `AsyncIterable<T>`, plus `batches()`, which hands over
  every item already received in one array. The frames of a network chunk are copied out
  once instead of one by one: with 22-byte items in 64 KiB chunks, `for await` went from
  0.81 to 0.23 µs per item, and `batches()` reads one in 0.13 µs. Items before a fault are
  delivered before the fault is thrown.
- **Rust: reads of a verified view cannot fail, so they no longer return `Result`.** On
  `XVerified` (`from_bytes`, `verified`, and every view a client `Response` or a server call
  hands out), strings, bytes, lists, nested tables and unions return the value itself
  (`&str`, `Option<&str>`, `VListR<…>`…): `req.sku()` instead of `req.sku()?`. Lists of a
  verified view iterate over their elements directly, and `get(i)` returns `Option`.
  `validate_all` returns `Vec<Invalid>`. Checked readers (`read`) keep their `Result`s.
  Breaking for code that wrote `?` after those reads.
- **Rust: `StreamReply::frames(items, |w, item| …)`** encodes a whole streamed reply lazily
  (one `Item` per element, then `End`), ready for an HTTP body stream.
- **Rust: generated code is `#[allow(dead_code)]`,** so a binary that uses part of it builds
  without warnings.
- **Axum example** (`examples/axum`): a service served by Axum, with tests that call it
  through the generated Rust client, and a JavaScript client; documented in the Rust SDK
  ("Serving over HTTP").
- **VS Code extension, on the [Marketplace](https://marketplace.visualstudio.com/items?itemName=graven.graven):** go to definition, find references, hover, outline and workspace
  symbols for `.graven` schemas; highlighting for services, methods and attributes with
  arguments.

## 0.3.1 — one message, two containers

- **RPC frames carry bare messages** ([proposal 0005](docs/proposals/0005-wire-frames.md)).
  One message, two containers: the envelope is for messages at rest, the frame for messages
  in flight, and a frame no longer contains an envelope. Every frame starts with 7 bytes
  (version and kind in one byte, union variant, message length); a `Call` adds method and
  service id. Replies no longer repeat service and method, and frames carry no checksum
  (the transport must deliver bytes intact). A stream item of 15 bytes travels in 22
  instead of 55, and transport compression works again. The RPC spec and vectors were
  rewritten (23 vectors); the generated client and server keep their API. In the runtimes:
  `encode_frame` / `encodeFrame` take no schema id, `FrameHeader::call` / `reply`
  (`callHeader` / `replyHeader` in JavaScript), `encode_end` / `encodeEnd` and the fault frame
  helpers take no service or method, and the `ServiceMismatch`, `MethodMismatch` and
  `SchemaMismatch` protocol errors are gone.
- **JavaScript: a `Sender` over `fetch` type-checks.** The call passed to a `Sender` is typed
  `OwnedBytes` (a `Uint8Array` on its own `ArrayBuffer`), so `fetch(url, { body: call })`
  compiles under TypeScript 5.7 and later.
- **JavaScript: reply size limit.** `CallOptions.maxFrameBytes` (default 16 MiB) bounds each
  reply frame; a frame whose header announces more fails with `RpcProtocolError("TooLarge")`
  before it is buffered, so a broken or hostile server cannot make the client hold unbounded
  memory.
- **JavaScript: streamed scalar lists.** `push()` on a `<field>_streaming()` list now throws
  when another variable-size field was written since the list started, instead of writing a
  corrupt list (Rust rejects this at compile time).
- **JavaScript: faster runtime.** CRC32C four bytes at a time, integers read and written
  without `DataView`, strings verified without decoding them and written without an
  intermediate copy, and calls encoded in one buffer. On a 50-entry `bank.Transaction`
  (Node 24): `fromBytes` 12.2 → 2.9 µs, reading every field 13.3 → 6.0 µs, writing
  13.5 → 3.8 µs (`toBytes` reuses its builder), streams decode each frame once. Same bytes;
  checked by the test suite and the fuzzer against Rust.
- **JavaScript: envelope check order.** An envelope whose `payload_len` goes past the input
  now fails with `Incomplete`, as format v1 §8.4 requires, instead of `TooLarge` for lengths
  over 2³¹ − 1. Found by the new JavaScript fuzzer (`scripts/fuzz-js.sh`), which checks the
  generated code against the Rust reference.
- **Validation failures carry codes for translation.** A `range` or `len` failure carries the
  bounds declared in the schema (`Rule::Range { min, max }`, `Rule::Len { min, max }`; `min`
  and `max` in JavaScript), and a check returns a reason code instead of a boolean (Rust
  `Result<(), &'static str>`, JavaScript `true | string`), reported as `reason`. Messages are
  never written by the schema or the server: the program that shows a failure translates the
  codes. `graven.rpc.BrokenRule` gains the optional fields `min`, `max` and `reason`, and an
  `Invalid` fault no longer carries an English `message`. The RPC vectors pin the new fields.
  Breaking for code that implements `Checks` or matches on `Rule`.

## 0.3.0 — services and JavaScript

- **Services** ([proposal 0003](docs/proposals/0003-services.md)). `service` declarations
  with typed errors (`Result<T, E>`), streamed replies, one-way methods, retired method IDs
  and timeouts. The bytes of a call and of a reply are specified as RPC frame format v1
  ([spec](docs/spec/graven-rpc-v1.md)), with 34 conformance vectors in
  `testvectors/rpc-v1/`. The lockfile checks how services change (`method-removed`,
  `method-id-reused`, `method-changed`, `service-removed`, `method-retired`), and
  `gravenc diff` lists it. `gravenc gen --side client|server` (and `Build::client()` /
  `Build::server()`) generate a client over the application's `Sender`, with streams read
  as their bytes arrive, and the typed edge of a server (`<Service>Call::decode`, typed
  replies). Transports stay with the application. No change to format v1.
- **JavaScript and TypeScript** ([proposal 0004](docs/proposals/0004-typescript.md)).
  `gravenc gen --lang js` writes `.js` files with `.d.ts` declarations: readers, verified
  views, writers, validation and service clients, plus the runtime (`graven.js`,
  `graven.d.ts`), for browsers, Node and Deno, with no package to install and no build
  step. Checked against the same conformance vectors as Rust, byte for byte when writing,
  and against a server generated in Rust.
- **Compiler**: a union's RPC writer name (`UW`) is reserved; compatibility messages for
  services name the old and new signature.

## 0.2.0 — validation rules in schemas

- **Validation rules in schemas** ([proposal 0002](docs/proposals/0002-validation.md),
  implemented). Built-in `range`, `len`, `finite` and `known` rules, named functions
  (`check(name)`) that each application provides for any other rule, validation that runs
  only when asked (`validate`, `validate_all`, `to_bytes_validated`), `graven-dynamic`
  validator construction checks, `gravenc inspect` reporting, and lockfile checks for rules
  that tighten over time (`rule-tightened`, `check-added`). No change to the wire format.

## 0.1.0 — format v1

First public release.

- **Format v1 frozen**: the specification and the 42 conformance vectors in
  `testvectors/v1/` are final. Any
  change to the wire format will be format v2, with a new `format_version` in the
  envelope.
- **`graven` runtime**: builder (including streaming lists), lazy checked reader, full
  `verify` with depth and budget limits, verified views, optional envelope (magic,
  version, schema id, CRC32C), `no_std` support.
- **`gravenc` compiler**: the `.graven` schema language, Rust code generation (CLI and
  `build.rs` API), deterministic lockfile, and the compatibility checker of spec §9.
- **Imports**: `import common;` lets a schema use the structs, enums, tables and unions of
  another namespace (`common.Money`). Each namespace is declared in one file; imported
  layouts are recorded in the importer's lockfile, and breaking an imported type is refused
  (`import-changed`). See [proposal 0001](docs/proposals/0001-imports.md).
- **Documentation comments**: `///` in a schema documents the type, field or variant that
  follows it, and the generated code carries it (reader, verified view, writer, accessors,
  setters, enums, structs, unions).
- **Data tools in `gravenc`**: `diff` (what changed since the lockfile, with a verdict per
  change), `inspect` (read the messages in a file), `to-json` / `from-json`, and `scan`
  (check a file of envelopes, tell a crash-truncated tail from corruption, and remove the
  tail with `--truncate`).
- **`graven-dynamic`**: a schema-driven reader and writer, used by the tools and by the
  conformance suite.
- **Editor support**: a VS Code extension with syntax highlighting for `.graven` schemas and
  lockfiles.
- **Documentation and website**: specification, schema language guide, binary layout by
  example, good practice, benchmarks against FlatBuffers and Cap'n Proto, a guide for AI
  assistants with `llms.txt` / `llms-full.txt`, and the history of the design; landing page
  in English, Portuguese, Spanish and Chinese (`scripts/site.sh`).
- **Assurance**: conformance interpreter, mutation gates over every vector, schema
  evolution matrix, five fuzz targets and Miri runs over the runtime and generated code.
