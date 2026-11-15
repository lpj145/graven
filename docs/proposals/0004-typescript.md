# Proposal 0004: JavaScript runtime and generator, with TypeScript declarations

Status: **accepted and implemented** in 0.3.0 (see [JavaScript and TypeScript](../javascript.md)). Stage 5 of [proposal 0003](0003-services.md). Stages 1–6 were first built with `.ts` output; stage 7 switched it to `.js` with `.d.ts`.

## Motivation

Browsers are where most service calls start, and today Graven has nothing that runs
there. A browser that talks to a Graven service has to fall back to JSON, losing the
envelope checks, the schema ids and the shared types.

Format v1 is frozen and pinned by conformance vectors, so a second implementation can be
checked byte for byte against the first. This proposal adds that implementation for
JavaScript, with TypeScript declarations, and teaches the compiler to generate it.

The output is **JavaScript with `.d.ts` declarations**, not TypeScript source: `.js` runs
in every JavaScript project, with or without TypeScript and with no build step, and the
`.d.ts` files give TypeScript projects the same types a `.ts` file would. A first version
generated `.ts`, which a project without TypeScript cannot use; see
[Output](#output-javascript-and-declarations).

## Goals

- A **JavaScript runtime** for format v1: reader, `verify`, writer, envelope, and the RPC
  frame of [graven-rpc-v1](../spec/graven-rpc-v1.md). It passes the 42 format vectors and
  the 34 frame vectors, and `gravenc` writes it next to the generated code.
- **`gravenc gen --lang js`**: readers, verified views and writers for tables, unions,
  enums and structs, as `.js` with `.d.ts`; and, with `--side client`, one client per
  service.
- The **same behaviour as Rust**: the same messages accepted and refused, the same errors
  by name, the same default limits, the same bytes written in canonical layout.
- Runs **unchanged in browsers, Node and Deno**: ES modules, no dependencies, no
  platform-specific APIs.

## Non-goals

- **No JavaScript server edge** (`--lang js --side server`) in this proposal. The compiler
  refuses it with a clear message. A Node server can come later on the same runtime.
- **No transport.** As in Rust, the client calls a send function the application writes.
- **No new format rules.** Anything the runtime needs that the specification does not say
  is a gap in the specification, fixed there first.

## Where it lives

- The runtime's source is `crates/graven-compiler/runtime/graven.ts`, inside the compiler
  crate so that the published `graven-compiler` package contains it. Its tests live in
  `ts/`, next to the vectors, and one CI run tests both.
- **It is not published as a package.** `gravenc` embeds it and `gen --lang js` writes it
  as `graven.js` and `graven.d.ts` into the output directory, next to the generated
  files, which import it as `./graven.js`. The runtime always comes from the same `gravenc` that generated the
  code, so their versions cannot drift, and projects need no dependency, registry or
  package name.
- Each file starts with a header naming the `gravenc` version that wrote it. Regenerating
  replaces it.
- Generate every schema of a project into one directory: two directories hold two copies
  of the runtime, and an error thrown by one copy fails `instanceof` against the other.
- Targets: ES2022 (for `BigInt`, `DataView.getBigUint64`, `TextDecoder`), as ES modules,
  with no bundler and no platform-specific APIs. The only development dependency is
  `typescript`, to compile and type-check the runtime and the generated code in tests.
  Tests use `node --test`.

## The runtime

### Values

| Graven | TypeScript |
|---|---|
| `u8 u16 u32 i8 i16 i32 f32 f64` | `number` |
| `u64 i64 u128 i128` | `bigint` |
| `bool` | `boolean` |
| `string` | `string` (decoded with `TextDecoder("utf-8", { fatal: true })`) |
| `bytes` | `Uint8Array`, a view into the message (no copy) |
| enum | `number`, plus a generated constant object of its variants |
| struct | a plain frozen object, read in full |
| `[T]` | a list view with `length`, `get(i)` and iteration |

Strings are the only values copied on read, because JavaScript strings cannot point into a
byte array. `schema_id`s are `bigint`.

### Errors

One class, `GravenError`, whose `kind` is the variant name of
[spec §7.6](../spec/graven-format-v1.md#76-error-vocabulary) (`"OutOfBounds"`,
`"InvalidUtf8"`, `"Missing"`, `"DepthExceeded"`, `"BudgetExceeded"`, `"ChecksumMismatch"`,
…) and of the frame decoder (`"UnsupportedFrameVersion"`, `"BadKind"`,
`"BadFrameHeader"`). Fields that carry data in Rust (`field`, `needed`, `expected`,
`found`) are properties. Readers throw it; the conformance tests compare `kind` with the
vectors' error names.

### Reading and verification

- A low-level `TableRef` over a `Uint8Array` and an offset, with the same operations as
  the Rust `TableRef`: scalar with presence bit, string, bytes, nested table, list
  headers, union pointer. Generated code is built on it; applications rarely use it.
- Accessors are **checked and lazy**, as in Rust: reading a field resolves only that
  field and throws on a bad pointer.
- `verify` walks the message exactly as spec §7 says, with the same charges and the
  default limits of §7.4 (`max_depth = 64`, `budget = max(len × 8, 1 MiB)`). A verified
  view skips the checks `verify` already did.

### Writing

- A `Builder` over a growing buffer that writes the **canonical layout** of spec §10.2,
  so the TypeScript writer produces the same bytes as the Rust writer, and the canonical
  vectors can be compared byte for byte.
- `TooLarge` when a message would pass `i32::MAX` bytes.

### Envelope and frames

- Envelope encode and decode in the order of spec §8.4, with its own table-driven CRC32C
  and the 64-bit FNV-1a `schema_id` (as `bigint`). An iterator over concatenated
  envelopes.
- RPC frames: encode and decode in the order of graven-rpc-v1 §4, `Fault` encode and
  decode, and reading frames from a byte stream that arrives in chunks.

## Output: JavaScript and declarations

`gravenc gen schema.graven -o dir --lang js` writes:

| File | Content |
|---|---|
| `dir/graven.js`, `dir/graven.d.ts` | the runtime and its declarations |
| `dir/<stem>.js`, `dir/<stem>.d.ts` | the schema's code and its declarations |

Imported schemas are generated next to them and imported as `./<stem>.js`. A schema whose
file is named `graven.graven` would collide with the runtime and is refused.

- **The `.js` files contain no type syntax.** They run as they are in browsers, Node and
  Deno, and any bundler accepts them. A JavaScript project uses them with no build step
  and no `typescript` dependency.
- **The `.d.ts` files carry every type** described in this proposal: a TypeScript project
  (or an editor on a JavaScript project) gets the same checking and completion it would
  get from `.ts` source.
- **The runtime is written in TypeScript** in the repository
  (`crates/graven-compiler/runtime/graven.ts`), compiled by `tsc` into `graven.js` and
  `graven.d.ts`, and both compiled files are committed next to it. `gravenc` embeds the
  compiled files, so it never needs Node or `tsc`. `scripts/ts.sh` recompiles them and
  fails if the committed files differ, so they cannot fall out of date.
- **The generator writes both files of a schema itself.** Each generated member (accessor,
  setter, static method, client method) is described once, with its parameters, types and
  body, and rendered twice: the body into `.js`, the signature into `.d.ts`. One
  description per member is what keeps the two files from disagreeing.

## Generated code

### Names

Names stay as written in the schema, with no conversion to camelCase: a field
`price_cents` is `price_cents()` on readers and `price_cents(v)` on writers, matching
`gravenc to-json` and the vectors. Types keep their schema names.

TypeScript allows reserved words as property names, so `delete` or `class` need no
change. A field whose name collides with a member the generator adds (`verify`,
`validate`, `reader`, `constructor`, and the others listed in the language guide) gets a
trailing `_` (`verify_()`). This is a rule of the TypeScript generator only: it never
makes `gravenc check` refuse a schema, so adding TypeScript to a project cannot break a
schema that works in Rust.

### Tables

For `table Product`:

```ts
const p = Product.verified(bytes);          // read + verify; throws GravenError
p.sku();                                    // string
p.price_cents();                            // bigint (i64)
p.discount();                               // number | undefined (optional)

const env = Product.toBytes(w => {          // an enveloped message
  w.sku("cafe-500g").price_cents(1890n).tags(["cafe", "graos"]);
  w.supplier().name("Fazenda");             // nested table: returns its writer
  const v = w.variants(2);                  // list of tables, known count
  v.push().size(250); v.push().size(500); v.close();
});
const q = Product.fromBytes(env);           // checks schema_id and verifies
Product.SCHEMA_ID;                          // bigint, from the lockfile
```

- `Product.read(bytes)` gives the checked reader, `Product.verified(bytes)` the verified
  view, `Product.fromBytes(envelope)` the verified view of an enveloped message.
- Optional fields return `undefined` when absent; fields with a default return the
  default.

### Unions and enums

```ts
const c = order.choice();   // { tag: "Leaf", value: LeafVerified } | { tag: "Unknown", raw: number }
w.choice_Leaf().x(1);

Color.Red;                  // 1
Color.name(order.color());  // "Red", or undefined for a value the schema does not declare
```

### Validation

Schemas with rules (proposal 0002) also get an interface `Checks` with one method per
named function, and `validate(checks)` / `validateAll(checks)` on verified views, which
return the same failures as Rust (`{ table, field, index, rule }`).

### Client

With `--side client`:

```ts
type Sender = (call: Uint8Array, timeoutMs: number | undefined)
  => Promise<Uint8Array | ReadableStream<Uint8Array> | AsyncIterable<Uint8Array>>;

const inv = new InventoryClient(async (call, timeoutMs) => {
  const r = await fetch("/inventory", { method: "POST", body: call,
    signal: timeoutMs ? AbortSignal.timeout(timeoutMs) : undefined });
  return r.body!;                           // a stream: items are read as they arrive
});

const r = await inv.get(w => w.sku("cafe-500g"));
if (r.ok) r.value.name(); else r.error;     // r.error: the method's error type, as a union

for await (const p of await inv.list(w => w.prefix("cafe"))) { /* … */ }

await inv.adjust(w => w.sku("x").delta(-1), { timeoutMs: 1000 });   // override the timeout
```

- One function, not an interface: returning bytes, a `ReadableStream` (a `fetch` body) or
  any async iterable covers every case, and a stream is read item by item.
- Unary methods with `Result<T, E>` return `{ ok: true, value } | { ok: false, error }`.
  Methods without `Result` return the value. Streaming methods return an async iterable;
  with `Result<stream T, E>`, a result whose `value` is that iterable.
- A `Fault` throws `RpcFault` (with `code`, `message`, `broken`). A reply that breaks the
  protocol throws `RpcProtocolError`, with the same kinds as Rust's `ProtocolError`. An
  error from the send function propagates unchanged.
- The declared timeout is passed to the send function by default.

## Compiler changes

- `gravenc gen` gains `--lang rust|js` (default `rust`); `Build` keeps generating Rust.
  The compiled runtime (`graven.js`, `graven.d.ts`) is embedded from
  `crates/graven-compiler/runtime/` with `include_str!`, so the `gravenc` binary and the
  published crate are self-contained. `--lang js --side server` is refused: "the
  JavaScript server edge is not available yet". `--lang ts` is refused with a message
  pointing to `--lang js`.
- `codegen/` becomes `codegen/rust/`, next to a new `codegen/js/`. Both read only the IR;
  no format or compatibility rule moves into a generator.
- The JavaScript generator has its own name handling (the trailing `_` above).
  `names.rs` keeps checking Rust names in `gravenc check`, since Rust remains the
  reference implementation and `build.rs` depends on it.

## Tests

- **Conformance**: the JavaScript code generated for `conformance.graven` reads each of
  the 42 vectors and gets exactly the value or error name of its `.json`, with the same
  value encoding (integers as decimal strings, floats by bits).
- **Canonical writing**: writing the values of the canonical vectors (`minimal`,
  `all_fields`, `zero_is_present`, `empty_lists`, `v2_read_by_v1`, `v1_read_by_v2`,
  `enveloped_all_fields`) produces their `.bin` byte for byte.
- **Frames**: the 34 frame vectors, and `Fault` encode/decode.
- **Interop with Rust**: a small Rust binary in `graven-codegen-tests` serves the
  `inventory` service over stdin/stdout with the generated Rust server edge; the generated
  JavaScript client calls every method shape through it, including a stream read item by
  item and a validation `Fault`.
- **Generator**: name escaping for colliding fields.
- **Types**: the tests are TypeScript and compile with `tsc --strict` against the
  generated `.d.ts` files, then run against the generated `.js` files.
- **Plain JavaScript**: one test is a `.mjs` file run by Node with no compilation, using
  the generated code for `conformance.graven` and the `inventory` client; it fails if any
  type syntax reaches a `.js` file.
- **Declarations match the code**: for the runtime and every generated module, a test
  compares the exports, classes and class members found at run time with those declared
  in the matching `.d.ts` (read with the `typescript` package's parser). A member present
  in one file and missing from the other fails.
- **Compiled runtime is current**: `scripts/ts.sh` recompiles `graven.ts` and fails if the
  result differs from the committed `graven.js` and `graven.d.ts`.
- `scripts/ts.sh` runs everything. `cargo test` does not need Node.

## Implementation order

1. **Runtime reader**: `TableRef`, `verify`, envelope, `GravenError`; the conformance
   test reading the 42 vectors through a hand-written reader for `conformance.graven`.
2. **Runtime writer**: `Builder` in canonical layout; the byte-for-byte writing test.
3. **Generated types**: the compiler reorganisation, then generated readers, views,
   writers, unions, enums and structs; the conformance test switches to generated code.
4. **Frames and client**: frame and `Fault` in the runtime, `--side client`, the frame
   vectors and the interop test with Rust.
5. **Validation**: `Checks`, `validate`, `validateAll`.
6. **Documentation**: a JavaScript and TypeScript page in the docs, and the guide for AI
   assistants.
7. **JavaScript output**: stages 1–6 were first built with `.ts` output. This stage
   switches to `--lang js` and the `.js` + `.d.ts` files described in
   [Output](#output-javascript-and-declarations): the compiled, committed runtime, the
   generator rendering each member into both files, the new tests (plain JavaScript,
   declarations matching code, compiled runtime current), and the docs.

## Open questions

- The minimum Node version. Node 20 has everything ES2022 needs and is the oldest
  maintained line today.
- An npm package can be added later for projects that prefer one, without changing the
  generated code beyond its import path.
