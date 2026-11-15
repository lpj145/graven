# JavaScript and TypeScript

`gravenc gen --lang js` writes **JavaScript files with TypeScript declarations**: for each
schema a `<schema>.js` and a `<schema>.d.ts`, and next to them the runtime, `graven.js` and
`graven.d.ts`.

- The `.js` files contain no type syntax. They run as they are in browsers, Node (>= 20),
  Deno and any bundler (ES2022 modules), so a JavaScript project uses them with no build
  step and no dependency.
- The `.d.ts` files give TypeScript projects, and editors in JavaScript projects, the full
  types: checking and completion work as with TypeScript source.
- Nothing is installed from a package registry: the runtime comes from the same `gravenc`
  that generated the code, so the two always match. Each file starts with a header naming
  the `gravenc` version that wrote it.

## Generate code

```sh
gravenc gen schemas/inventory.graven -o src/gen --lang js
```

To also generate typed RPC clients for `service` declarations:

```sh
gravenc gen schemas/inventory.graven -o src/gen --lang js --side client
```

```js
import { Product, ProductW } from "./gen/shop.js";
```

Notes:
- `--lang rust` is the default.
- Generate every schema of a project into one directory. Two directories hold two copies of
  the runtime, and an error thrown by one copy fails `instanceof` against the other.
- A schema file cannot be named `graven.graven` (it would collide with the runtime
  `graven.js`).
- `--lang js --side server` is refused for now: "the JavaScript server edge is not
  available yet". The JavaScript code is for clients, browsers and tools that read Graven
  messages or call Rust services.

## Types and values

| Graven type | JavaScript value (type in `.d.ts`) |
| --- | --- |
| `u8`, `u16`, `u32`, `i8`, `i16`, `i32`, `f32`, `f64` | `number` |
| `u64`, `i64`, `u128`, `i128` | `bigint` |
| `bool` | `boolean` |
| `string` | `string` (UTF-8 fatal decode) |
| `bytes` | `Uint8Array` (zero-copy view) |
| `enum` | `number` + constant object + `.name(v)` |
| `struct` | Frozen plain object |
| `union` | Discriminated object (`tag`, `variant`, `value`) or `{ tag, variant: "Unknown" }` |
| `[T]` | Indexed + iterable list view (`length`, `get(i)`, `[Symbol.iterator]()`) |

Names stay as written in the schema (`price_cents()`, not `priceCents()`), matching
`gravenc to-json` and the conformance vectors. The generator adds a trailing `_`
only where a name would break the generated code; this never makes `gravenc check` refuse a
schema:

- members: a field, enum value, union variant or method named like a member the generator
  or JavaScript already uses (`constructor`, `toString`, `valueOf`, `__proto__`, `verify`,
  `reader`, `validate`, `validateAll`, …) becomes `constructor_()`, `verify_()`, and so on;
- `then` always becomes `then_`: an object with a callable `then` is treated as a promise
  by `await`, which would never settle;
- types: a type named like a TypeScript keyword or a global the generated code relies on
  (`Record`, `Object`, `Promise`, `Uint8Array`, `Error`, `Checks`, …) becomes `Record_`,
  `Object_`, and so on.

## Tables, reading, and writing

```ts
import { Product, ProductW } from "./gen/shop.js";

// Encode an enveloped message (GRVN header + CRC32C)
const bytes = ProductW.toBytes((w) => {
  w.sku("cafe-500g").price_cents(1890n).tags(["cafe", "graos"]);
  w.supplier().name("Fazenda");          // nested table: returns its writer
  const variants = w.variants(2);        // list of tables with a known count
  variants.push().size(250);
  variants.push().size(500);
  variants.close();
  w.choice_Leaf().x(1);                  // union: one method per variant
});

// Verify envelope + schema id + CRC32C + message layout
const p = Product.fromBytes(bytes);
p.sku();
p.price_cents();
p.discount(); // number | undefined when optional
```

- `T.read(buf)` — unchecked-envelope reader from a raw message payload (`Verify` not run yet).
- `T.verified(buf)` / `T.verifiedWith(buf, ctx)` — verifies bare payload against budget & depth limits and returns `TVerified`.
- `T.fromBytes(env)` — validates the `GRVN` v1 envelope, schema id, and CRC32C, then verifies and returns `TVerified`.
- `T.SCHEMA_ID` — 64-bit FNV-1a schema id (`bigint`) from `<schema>.lock`.
- `TW.toBytes(fn)` / `TW.seal(builder, fn)` — canonical zero-copy writer API (`ListW`, `StreamW` for unknown-count lists).

## Validation

Schemas with validation rules (`#[range]`, `#[len]`, `#[finite]`, `#[known]`, `#[check(...)]`) generate an exported `Checks` interface and methods on `TVerified` / `TW`:

- `view.validate(checks)` — throws `ValidationError` on the first violated rule (`.failure` is `{ table, field?, index?, rule }`, matching Rust `Invalid`).
- `view.validateAll(checks)` — returns all `ValidationFailure[]` instead of stopping on the first.
- `TW.toBytesValidated(fn, checks)` — builds an enveloped message and runs `validate(checks)` before returning bytes.

## RPC client (`--side client`)

RPC clients require only one send function (`Sender`) matching browsers' `fetch`, WebSockets, or Node streams:

```ts
import { RpcFault, RpcProtocolError, type Sender } from "./gen/graven.js";
import { InventoryClient } from "./gen/inventory.js";

const send: Sender = async (call, timeoutMs) => {
  const res = await fetch("/inventory", {
    method: "POST",
    body: call,
    signal: timeoutMs !== undefined ? AbortSignal.timeout(timeoutMs) : undefined,
  });
  return res.body!; // Uint8Array | ReadableStream<Uint8Array> | AsyncIterable<Uint8Array>
};

const client = new InventoryClient(send);

// Unary -> ItemVerified
const item = await client.get_item((w) => w.sku("espresso"));

// Unary -> Result<ReceiptVerified, ReserveErrVerified>
const res = await client.reserve((w) => w.sku("espresso").qty(2));
if (res.ok) {
  console.log(res.value.id());
} else {
  console.log(res.error);
}

// Server stream -> AsyncIterable<EventVerified> (decoded incrementally as chunks arrive)
for await (const ev of await client.watch((w) => w.sku("espresso"))) {
  console.log(ev.seq(), ev.stock());
}

// Per-call timeout override (otherwise the schema's declared timeoutMs is passed)
await client.get_item((w) => w.sku("espresso"), { timeoutMs: 1000 });
```

Errors:
- Server `Fault` frames throw `RpcFault` (`code`, `message`, `broken`).
- Protocol mismatches (empty reply, truncated stream, wrong service/method/schema id, trailing bytes) throw `RpcProtocolError` with `.kind` matching Rust's `ProtocolError`.
- Transport errors thrown by `Sender` propagate unchanged.

## Running the JavaScript suite

`cargo test` itself has no Node dependency. `scripts/ts.sh` runs the JavaScript side:

- it compiles the runtime (`crates/graven-compiler/runtime/graven.ts`, the source of
  `graven.js` and `graven.d.ts`) and fails if the committed compiled files are out of date
  (`scripts/ts.sh --update-runtime` rewrites them);
- it generates code for every test schema, type-checks the TypeScript tests under
  `tsc --strict` against the `.d.ts` files, and runs them against the `.js` files:
  conformance vectors, byte-for-byte writing, RPC frames, validation, declarations matching
  the code, and a client calling the generated Rust server;
- it runs plain JavaScript tests with no compilation step.

```sh
./scripts/ts.sh
```
