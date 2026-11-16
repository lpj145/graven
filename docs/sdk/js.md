# JavaScript / TypeScript SDK

`gravenc gen --lang js` generates **plain ES2022 modules (`.js`) alongside full TypeScript declarations (`.d.ts`)** with **zero runtime dependencies**. It runs directly in browsers, Node (>= 20), and Deno — no bundler or build step required for plain JS, and full strict type checking in TypeScript.

## Setup

Install `gravenc` (requires Rust toolchain only on the machine generating bindings):

```sh
cargo install graven-compiler
```

Generate bindings from a checked schema and lockfile:

```sh
gravenc gen schemas/bank.graven -o src/gen --lang js
```

Pass `--side client` to also emit typed RPC clients for `service` declarations (`--lang js --side server` is refused: *"the TypeScript server edge is not available yet"* / JS server edge is not available yet).

`gravenc` writes `graven.js` and `graven.d.ts` (the matching runtime) into the same output directory next to `<schema>.js` / `<schema>.d.ts` and any imported schemas:

```js
import { Currency, Transaction, TransactionW } from "./gen/bank.js";
```

> [!WARNING]
> Generate all schemas of a project into **one directory** so every module shares a single `graven.js` runtime (otherwise `instanceof GravenError` checks fail across separate runtime copies). A schema file cannot be named `graven.graven` because it would collide with `graven.js`.

## Types and name escaping

| Graven type | Runtime value / `.d.ts` type |
|---|---|
| `u8`, `u16`, `u32`, `i8`, `i16`, `i32`, `f32`, `f64` | `number` |
| `u64`, `i64`, `u128`, `i128` | `bigint` |
| `bool` | `boolean` |
| `string` | `string` (decoded with fatal UTF-8 `TextDecoder`) |
| `bytes` | `Uint8Array` (zero-copy view into buffer) |
| `enum` | `number` + frozen constant object + `E.name(v)` |
| `struct` | Frozen plain object |
| `union` | `{ tag: <n>, variant: "<Name>", value: T }` or `{ tag: number, variant: "Unknown" }` |
| `[T]` | Indexed & iterable view (`length`, `get(i)`, `[Symbol.iterator]()`) |

Field and method names stay **as written in the schema** (`price_cents()`, never camelCase), matching `gravenc to-json` and conformance vectors. To avoid breaking JS/TS without ever rejecting valid `.graven` schemas:
- **Reserved members:** `constructor`, `toString`, `valueOf`, `__proto__`, `verify`, `reader`, `validate`, `validateAll`, etc. gain a trailing `_` (e.g. `constructor_()`).
- **`then`:** always becomes `then_` so objects never look like Promise "thenables" to `await`.
- **Reserved global/TS type names:** `Record`, `Object`, `Promise`, `Uint8Array`, `Error`, `Checks`, etc. gain a trailing `_` (`Record_`).

## Reading and writing

```js
import { Currency, Transaction, TransactionW } from "./gen/bank.js";

const bytes = TransactionW.toBytes((t) => {
  t.id(42n).ts(1_700_000_000n).currency(Currency.USD).from_shard(3).to_shard(7);
  t.note("rent");
  const entries = t.entries(2);
  entries.push().account(1001n).amount(-2500n).memo("transfer");
  entries.push().account(2002n).amount(2500n);
  entries.close();
});

const tx = Transaction.fromBytes(bytes);
console.log(`tx ${tx.id()} in ${Currency.name(tx.currency())}, note ${tx.note()}`);
for (const e of tx.entries()) {
  console.log(`  account ${e.account()} amount ${e.amount()} memo ${e.memo()}`);
}
```

- **Untrusted input:** `X.fromBytes(envelope)` checks envelope header, `schemaId`, and CRC32C, verifies the message, and returns `XVerified`. `X.verified(buf)` / `X.verifiedWith(buf, ctx)` do the same for bare messages.
- **Lazy reads:** `X.read(buf)` returns a checked reader without full traversal; optional fields return `undefined` when absent.
- **Errors:** malformed messages throw `GravenError` whose `.kind` matches [format v1 §7.6](../spec/graven-format-v1.md#76-error-vocabulary) (`"OutOfBounds"`, `"InvalidUtf8"`, `"Missing"`, `"ChecksumMismatch"`, etc.).
- **Writing:** `XW.toBytes(fn)` returns an enveloped `Uint8Array`. For batches, `XW.seal(builder, fn)` appends enveloped frames and `builder.takeFrames()` returns them. Nested tables return their writer (`w.child().x(1)`); unions expose `<field>_<Variant>()` (`w.choice_Leaf()`); lists accept a known count (`w.items(n)`, calling `.close()` when done) or stream via `w.items_streaming()`. Output matches the Rust canonical writer byte for byte.

## Validation

Schemas with rules emit a `Checks` interface in `<schema>.d.ts`:

```ts
import { z } from "zod";
import type { Checks } from "./gen/shop.js";
import { Promo, PromoW } from "./gen/shop.js";

// `true` accepts the value; a string rejects it with that reason code.
const checks: Checks = {
  email: (v) => z.string().email().safeParse(v).success || "not_an_email",
  slug: (v) => /^[a-z0-9-]+$/.test(v) || "bad_char",
  valid_period: (p) => p.starts() < p.ends() || "ends_before_start",
};

const bytes = PromoW.toBytesValidated(
  (p) => p.name("summer-sale").contact("hi@example.com").starts(10n).ends(20n),
  checks,
);

const promo = Promo.fromBytes(bytes);
promo.validate(checks);                // throws ValidationError on first failure
const all = promo.validateAll(checks); // returns ValidationFailure[]
```

Each `ValidationFailure` has `{ table, field?, index?, rule, min?, max?, reason? }` where `rule` is `"range"`, `"len"`, `"finite"`, `"known"`, or `"check(name)"`; `min`/`max` are the declared bounds of `range` and `len` (decimal text), and `reason` is the code a check returned. They are codes for the application to turn into a message in its user's language ([messages](../validation.md#messages-in-the-readers-language)); `ValidationError.message` is an English diagnostic for logs. If a schema uses only built-in rules, `checks` may be omitted (`checks = {}`).

## Services

Generate with `--side client`:

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

const inv = new InventoryClient(send);

// Result<Product, LookupError> -> { ok: true, value } | { ok: false, error }
const r = await inv.get((w) => w.sku("cafe-500g"));
if (r.ok) {
  console.log(`${r.value.stock()} in stock`);
} else if (r.error.variant === "NotFound") {
  console.log(`no product ${r.error.value.sku()}`);
}

// Streaming replies decode incrementally as network chunks arrive:
for await (const p of await inv.list((w) => w.prefix("cafe"))) {
  console.log(p.name());
}

// Per-call timeout override (otherwise uses the schema's declared timeoutMs):
await inv.get((w) => w.sku("cafe-500g"), { timeoutMs: 100 });
```

- **Protocol faults:** throws `RpcFault` with `code` (`FaultCode`), `faultMessage` (optional diagnostic string; absent on `Invalid`), `message` (`Error.message`), and `broken` (`BrokenRule` on `Invalid` faults: `tableName`, `field?`, `index?`, `rule`, `min?`, `max?`, `reason?`, the same codes as a `ValidationFailure`).
- **Protocol violations:** throws `RpcProtocolError` (`EmptyReply`, `Truncated`, `ServiceMismatch`, `MethodMismatch`, `SchemaMismatch`, `TrailingBytes`, etc.). Transport errors thrown by `Sender` propagate untouched.
- **Metadata:** `<SERVICE>_SERVICE_ID` (`bigint`) and frozen `<service>_timeouts.<METHOD>_MS`.

## API reference and verification

### Generated per schema (`<schema>.js` / `<schema>.d.ts`)

- **Table `X`:** `X.read(buf)`, `X.verified(buf)`, `X.verifiedWith(buf, ctx)`, `X.fromBytes(env)`, `X.SCHEMA_ID`, `verify(ctx)`; `XVerified` (`reader()`, accessors, `validate`, `validateAll`); `XW` (`root`, `toBytes`, `toBytesWith`, `seal`, `toBytesValidated`, setters); `X_SCHEMA_ID`.
- **Enum `E`:** `type E = number` + frozen object with `E.name(v)`. **Struct `S`:** readonly interface. **Union `U`:** `U`, `UVerified`, and `UW` (with `--side client`).
- **Validation & RPC:** `Checks` interface, `<Service>Client`, `<SERVICE>_SERVICE_ID`, `<service>_timeouts`.

### Embedded runtime (`graven.js` / `graven.d.ts`)

| Export | Purpose |
|---|---|
| `GravenError` | Structural read/verify/write errors with `.kind` and data fields. |
| `Builder`, `TableW` | Message writer (`finish`, `finishEnvelope`, `seal`, `sealEnvelope`, `takeFrames`). |
| `TableRef`, `VerifyCtx` | Low-level unchecked/verified buffer navigation. |
| `encodeEnvelope`, `decodeEnvelope`, `openEnvelope`, `envelopes` | Envelope v1 encoding, validation, and iteration. |
| `crc32c`, `schemaId` | Table-driven CRC-32C and 64-bit FNV-1a `schemaId` (`bigint`). |
| `ValidationError`, `ValidationFailure` | Rule failure details. |
| `encodeFrame`, `decodeFrame`, `frames`, `FrameKind`, `FaultCode` | RPC v1 frame & fault encoding/decoding. |
| `RpcFault`, `RpcProtocolError`, `Sender`, `CallOptions`, `RpcResult` | Client transport contracts and error types. |

### Verification suite (`scripts/ts.sh`)

`cargo test` has no Node dependency. Run all JS/TS checks with:

```sh
./scripts/ts.sh
```

This recompiles `crates/graven-compiler/runtime/graven.ts` (and fails if committed `graven.js`/`graven.d.ts` differ), generates bindings for all test schemas, compiles TypeScript suites under `tsc --strict`, checks with the TS AST parser that every `.d.ts` matches its `.js` exports/members, tests plain `.mjs` usage with no build step, and runs all 42 v1 vectors, canonical write parity, 18 RPC v1 vector pairs, validation, and live Rust ↔ JS stdio RPC interoperability.
