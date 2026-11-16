# Services

A schema can declare the operations a program offers, the same way it declares tables. A
`service` lists methods, each with an input, an output, the errors it can return and how
long a caller should wait. The compiler checks how services change over time and generates
both ends: a client that turns calls into bytes and replies into typed results, and the edge
of a server that turns bytes into a typed call and a typed reply into bytes.

What sits between them is up to the application. Graven specifies the **bytes** of a call and
of a reply ([RPC frames v1](spec/graven-rpc-v1.md)), not how they travel: HTTP, TCP, UDP, a
queue or a file all work, and connections, retries and authentication stay with the
application. The design is [proposal 0003](proposals/0003-services.md).

## Declaring a service

```graven
namespace inventory;

table GetProduct   { sku @0: string; }
table ListProducts { prefix @0: string?; limit @1: u32 = 100; }
table AdjustStock  { sku @0: string; delta @1: i32; }
table Product      { sku @0: string; name @1: string; stock @2: u32; }
table StockChanged { sku @0: string; stock @1: u32; }
table Ping         { node @0: u64; }

table NotFound     { sku @0: string; }
table OutOfStock   { sku @0: string; available @1: u32; }

union LookupError  { NotFound @1: NotFound }
union StockError   { NotFound @1: NotFound, OutOfStock @2: OutOfStock }

/// Products and stock.
#[timeout(ms = 5000)]
service Inventory {
  #[timeout(ms = 500)]
  get    @1: (GetProduct)   -> Result<Product, LookupError>;
  list   @2: (ListProducts) -> stream Product;
  adjust @3: (AdjustStock)  -> Result<StockChanged, StockError>;
  ping   @4: (Ping);
  @5: reserved;
}
```

- **Method IDs** (`@1` to `@65535`) identify methods in the bytes, as field IDs do; names do
  not travel, so a method can be renamed. A retired method becomes `@n: reserved;` and its
  ID is never used again.
- **Inputs, outputs and errors** are tables or unions, local or imported. A union lets one
  method take or return one of several messages.
- **Timeouts**: `#[timeout(ms = n)]` on the service sets a default; on a method it
  overrides it. A timeout is part of the method's contract, not of the bytes: the generated
  client hands it to the application's send function, and the server edge exposes it as a
  constant.
- `///` comments on services and methods are carried into the generated code.

## Method shapes

| Declaration | Shape | The caller receives |
|---|---|---|
| `(I) -> T` | unary | `T`, or a fault |
| `(I) -> Result<T, E>` | unary | `T`, the method's error `E`, or a fault |
| `(I) -> stream T` | stream | any number of `T`, then the end or a fault |
| `(I) -> Result<stream T, E>` | stream | `E` before any item; or items, then the end or a fault |
| `(I)` | one-way | nothing, not even a fault |

## Errors and faults

There are two kinds of failure, and a client handles both:

- **The method's error (`E`)**: an outcome the method declares, such as "not found". It is a
  normal message, typed like the output, and evolves under the rules of unions: a new
  error variant is allowed, and an old client reads it as unknown.
- **A protocol fault**: the same for every method. The call could not be decoded
  (`Malformed`), named an unknown service or method (`Unsupported`), broke a
  [validation rule](validation.md) (`Invalid`, with the table, field, rule, bounds and check
  reason, as codes the client turns into a message in its user's
  [language](validation.md#messages-in-the-readers-language)), ran out of
  time (`DeadlineExceeded`), or the server failed (`Internal`, `Unavailable`). Faults use
  the fixed `graven.rpc` schema of [RPC frames v1 §5](spec/graven-rpc-v1.md#5-protocol-faults-gravenrpc).

## The bytes

A message is the same bytes everywhere; what changes is the container around it. On disk it
goes in an [envelope](spec/graven-format-v1.md#8-envelope), which says what it is and carries
a checksum, because stored bytes have no context. In a call it goes in a **frame**, which
carries only what the call does not already know:

- every frame starts with 7 bytes: frame version and kind in one byte, the union variant,
  and the message's length;
- a call adds the method and the service id (17 bytes in all); a reply does not repeat them,
  since it belongs to the call it answers;
- the message follows, with no envelope. Its type comes from the method.

Frames are self-delimiting, so a streamed reply is its frames one after another on any byte
channel; a stream item of 15 bytes travels in 22. Frames carry no checksum: they require a
transport that delivers bytes intact (TLS, a local socket, a queue that checks its records).
Moving a message between a frame and an envelope never re-encodes it. The full rules, with
their check order, are in [RPC frames v1](spec/graven-rpc-v1.md).

## How services evolve

The lockfile records every service and method, and `gravenc update` checks each change:

| Change | Verdict |
|---|---|
| a new service, or a new method | allowed |
| a method renamed (same ID) | allowed |
| a timeout added, changed or removed | allowed; listed by `gravenc diff` |
| a new variant in a union used as output or error | allowed |
| a method replaced by `@n: reserved;` | warning `method-retired`, confirmed with `#[allow_breaking_readers]` |
| a method removed without reserving its ID | error `method-removed` |
| a reserved ID used by a method again | error `method-id-reused` |
| input, output, error type or shape changed | error `method-changed` |
| a service removed or renamed | error `service-removed` |

The messages a method uses keep evolving under their own rules: adding an optional field to
its input is allowed, as for any table. A method that may need errors later should return
`Result<T, E>` from the start, since adding a `Result` changes its shape.

## Generated code

`gravenc gen --side client`, `--side server`, or `--side client,server` generates typed RPC
bindings alongside table types:

<!-- tabs -->
#### Rust

```rust
use graven::rpc::{CallError, Sender};
use inventory::{InventoryClient, LookupErrorVerified};

let inv = InventoryClient::new(http_sender);
match inv.get(|w| { w.sku("cafe-500g"); }).await {
    Ok(p) => println!("{} in stock", p.view().stock()),
    Err(CallError::Err(e)) => match e.view() {
        LookupErrorVerified::NotFound(nf) => println!("no product {}", nf.sku()?),
        LookupErrorVerified::Unknown(_) => println!("lookup failed"),
    },
    Err(other) => return Err(other.into()),
}
```

Supports both typed **client** and `no_std` **server edge** (`InventoryCall::decode` / `decode_validated`); see [Rust SDK services](sdk/rust.md#services).

#### JavaScript / TypeScript

```ts
import { InventoryClient } from "./gen/inventory.js";

const inv = new InventoryClient(async (call, timeoutMs) => {
  const res = await fetch("/inventory", {
    method: "POST",
    body: call,
    signal: timeoutMs !== undefined ? AbortSignal.timeout(timeoutMs) : undefined,
  });
  return res.body!;
});

const r = await inv.get((w) => w.sku("cafe-500g"));
if (r.ok) {
  console.log(`${r.value.stock()} in stock`);
} else if (r.error.variant === "NotFound") {
  console.log(`no product ${r.error.value.sku()}`);
}
```

Supports `--side client` (browsers, Node, Deno; streaming items decoded incrementally); see [JS / TS SDK services](sdk/js.md#services).
<!-- /tabs -->
