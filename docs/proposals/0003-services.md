# Proposal 0003: services

Status: **Stages 1–4 implemented** (Rust RPC runtime, conformance vectors, compiler support, codegen, CLI `--side` / `Build`); Stage 5 (TypeScript runtime / `--lang ts`) pending.

## Motivation

Graven messages already travel between processes: a browser asks a server for data, nodes
of a cluster exchange Raft entries or SWIM membership updates, an embedded database ships
its log to a replica. Today each application invents how a request names its operation
and how the answer and its errors come back. Each invention differs, and none is checked
by the compiler.

A schema can declare the operations a program offers, with their inputs, outputs, errors
and time limits, the same way it declares tables. The compiler can then check how they
change over time and generate the code at both ends.

## Goals

- Declare **services** in the schema: methods with an input, an output, typed errors,
  streamed replies, one-way calls and timeouts.
- Specify the **bytes of a call and of a reply**, so that any two implementations agree on
  them.
- Make the **lockfile** check service changes, as it already does for tables.
- Generate a **client** that turns method calls into bytes and bytes into typed results,
  and the **typed edge of a server** that turns bytes into a typed call and a typed reply
  into bytes.
- Keep the message format unchanged: format v1, its envelope and its conformance vectors
  stay as they are. Every payload is an ordinary envelope.

## Non-goals

- **No transport.** How the bytes travel (HTTP, TCP, UDP, a queue, a file), how replies
  are matched to calls on a shared connection, cancellation, retries and authentication
  belong to the application. Graven specifies the bytes, not how they move.
- **No server runtime or framework integration.** The generated server code is plain
  functions over bytes, with no async runtime and no dependency on any library.
- **No route per method.** The method is in the call's bytes; an application serves a
  whole service from one place and dispatches with a `match`.
- **No client streaming** (a method whose input is a stream) in this proposal.
- **No remote objects or promise pipelining** (as in Cap'n Proto RPC).

## Syntax

```graven
namespace inventory;

table Product      { sku @0: string; name @1: string; price_cents @2: i64; stock @3: u32; }
table GetProduct   { sku @0: string; }
table ListProducts { prefix @0: string?; limit @1: u32 = 100; }
table AdjustStock  { sku @0: string; delta @1: i32; reason @2: string?; }
table StockChanged { sku @0: string; stock @1: u32; at @2: u64; }
table Ping         { node @0: u64; }

table NotFound     { sku @0: string; }
table OutOfStock   { sku @0: string; available @1: u32; }

union LookupError  { NotFound @1: NotFound }
union StockError   { NotFound @1: NotFound, OutOfStock @2: OutOfStock }

/// Products and stock.
#[timeout(ms = 5000)]
service Inventory {
  /// One product.
  #[timeout(ms = 500)]
  get    @1: (GetProduct)   -> Result<Product, LookupError>;

  /// Products whose SKU starts with `prefix`, one at a time.
  #[timeout(ms = 2000)]
  list   @2: (ListProducts) -> stream Product;

  /// Changes the stock of one product.
  adjust @3: (AdjustStock)  -> Result<StockChanged, StockError>;

  /// A notice; there is no reply.
  ping   @4: (Ping);

  @5: reserved;             // a retired method: its ID is never reused
}
```

Grammar additions (section 1 of the language guide):

```ebnf
item      = enum | struct | table | union | service ;
service   = { attrs } "service" ident "{" { method } "}" ;
method    = { attrs } ( "@" int ":" "reserved" ";"
                      | ident "@" int ":" "(" type ")" [ "->" reply ] ";" ) ;
reply     = "Result" "<" output "," type ">" | output ;
output    = [ "stream" ] type ;
```

`service`, `stream` and `Result` are keywords in these positions only; they stay valid as
field names.

### Rules the checker enforces

- Method IDs are unique within a service and in `1..=65535`. A reserved ID can never name
  a method again.
- Method names are unique within a service.
- The input, the output and the error type are each a **table or a union** (local or
  imported). Scalars, structs, enums and lists are refused: a payload is a message, and a
  message needs a root that can evolve.
- `stream` appears only in the output: `-> stream T` or `-> Result<stream T, E>`.
  `stream Result<T, E>` is refused; errors per item are written as a union in `T`.
- A method without `->` is **one-way**: there is never a reply, not even a fault.
- `#[timeout(ms = n)]`, with `1 ≤ n ≤ 4_294_967_295`, is allowed on a service (the default
  for its methods) and on a method (overrides the default). It is refused on a one-way
  method. Without either, a method has no timeout.
- `#[allow_breaking_readers]` is allowed on a reserved method ID (see
  [Evolution](#evolution-and-the-lockfile)). Any other attribute on a service or method is
  an error.
- `///` documentation comments are allowed on services and methods and are carried to the
  generated code.
- Each service has a `service_id`: 64-bit FNV-1a over `namespace.Service`, computed as the
  `schema_id` of a table (spec §8.2) and recorded in the lockfile on first compilation.
  Service IDs and table schema IDs share one space: a collision between any two is an
  error.

### What a timeout means

A timeout is part of the method's contract: how long a caller should wait for it. Graven
does not enforce it on the wire. The generated client uses it as the default limit of each
call (overridable per call) and hands it to the application's send function; the generated
server exposes it as a constant, for a server that wants to stop work nobody waits for.

## Calls and replies

Every method has one of five **shapes**, determined by its declaration:

| Declaration | Shape | Reply, in order |
|---|---|---|
| `(I) -> T` | unary | `Ok` or `Fault` |
| `(I) -> Result<T, E>` | unary | `Ok`, `Err` or `Fault` |
| `(I) -> stream T` | stream | any number of `Item`, then `End` or `Fault` |
| `(I) -> Result<stream T, E>` | stream | `Err` before any `Item`; or `Item`s, then `End` or `Fault` |
| `(I)` | one-way | none |

`Err` carries the method's own error type `E`: an outcome the method declares, such as
"not found". `Fault` carries a **protocol fault**, the same for every method: the call
could not be decoded, broke a validation rule, named an unknown method, or the server
failed. A client always handles both.

## The frame

A call and each part of a reply is a **frame**: a fixed 16-byte header followed by an
envelope (spec §8), or by nothing for `End`. All integers are little-endian.

```
offset  size  field
0       1     frame_version = 1
1       1     kind
2       2     method       the method's @n
4       2     variant      the union tag of the body when its type is a union; 0 otherwise
6       2     reserved = 0
8       8     service_id   the service's id (u64)
16      …     body         one envelope (spec §8), absent for End
```

| kind | Name | Sent by | Body |
|---|---|---|---|
| 1 | `Call` | caller | the input `I` |
| 2 | `Ok` | server | the output `T` |
| 3 | `Err` | server | the error `E` |
| 4 | `Item` | server | one `T` of a stream |
| 5 | `End` | server | none |
| 6 | `Fault` | server | a `graven.rpc.Fault` |

- **Bodies are envelopes.** The envelope's `schema_id` names the body's table and its
  CRC32C covers it, so a body is verified exactly like any other enveloped message. When
  the declared type is a union, `variant` holds the tag of the chosen variant and the
  envelope holds that variant's table. A tag the reader does not know reads as
  `Unknown(tag)`, as for union fields.
- **Frames delimit themselves.** The envelope carries its own length, so a frame's size is
  `16`, or `16 + 24 + payload_len`. A stream reply is its frames one after another; an
  application can write them to any byte channel and read them back without extra
  framing.
- **Reply frames repeat `service_id` and `method`** of the call they answer, so a reply can
  be checked against the method that was called.

### Decoding and check order (normative)

A decoder performs these checks in order and stops at the first error:

1. `input_len < 16` ⇒ `Incomplete { needed: 16 }`;
2. byte 0 `≠ 1` ⇒ `UnsupportedFrameVersion(byte 0)`;
3. byte 1 not in `1..=6` ⇒ `BadKind(byte 1)`;
4. `reserved ≠ 0`, or `variant ≠ 0` in `End` ⇒ `BadFrameHeader`;
5. for every kind but `End`, the body is decoded as an envelope (spec §8.4), with the
   envelope's own errors.

Bytes after the frame belong to the next one. Checking that `service_id`, `method`,
`variant` and the envelope's `schema_id` match the schema is the job of the generated
code, not of the frame decoder.

### Protocol faults

Faults use a fixed schema published with the specification:

```graven
namespace graven.rpc;

enum FaultCode : u16 {
  Unsupported      = 1,   // unknown service_id, method or variant
  Malformed        = 2,   // the frame or its body failed to decode or verify
  Invalid          = 3,   // the input broke a validation rule (proposal 0002)
  DeadlineExceeded = 4,   // the server stopped because the method's timeout passed
  Internal         = 5,   // the server failed; details stay on the server
  Unavailable      = 6,   // the server cannot take calls now; the caller may retry
}

table BrokenRule {
  table_name @0: string;
  field      @1: string?;
  index      @2: u32?;
  rule       @3: string;   // "range", "len", "finite", "known" or "check(name)"
}

table Fault {
  code    @0: FaultCode;
  message @1: string?;
  broken  @2: BrokenRule?;  // present when code = Invalid
}
```

New codes may be added; a client treats a code it does not know as `Internal`.

## Generated code

The compiler generates the types (tables, unions, enums), as today, plus one or both
sides of each service. The side is chosen at generation time:

```
gravenc gen inventory.graven -o src/gen                         # types only (as today)
gravenc gen inventory.graven -o src/gen --side server           # types + server edge
gravenc gen inventory.graven -o src/gen --side client           # types + client
gravenc gen inventory.graven -o src/gen --side client,server
```

and in `build.rs` with `Build::new().schema(…).client().server()`. Without a side,
services generate nothing, so schemas used only for storage compile as before.

### Server edge (Rust)

For `service Inventory`:

```rust
pub enum InventoryCall<'a> {
    Get(GetProductVerified<'a>, Reply<ProductW, LookupErrorW>),
    List(ListProductsVerified<'a>, StreamReply<ProductW, ()>),
    Adjust(AdjustStockVerified<'a>, Reply<StockChangedW, StockErrorW>),
    Ping(PingVerified<'a>),
}

impl<'a> InventoryCall<'a> {
    /// Decodes and verifies one `Call` frame of this service.
    pub fn decode(frame: &'a [u8]) -> Result<Self, Rejected>;
    /// The same, then runs the input's validation rules.
    pub fn decode_validated(frame: &'a [u8], checks: &impl Checks) -> Result<Self, Rejected>;
}

pub const INVENTORY_SERVICE_ID: u64 = /* from the lockfile */;
pub mod inventory_timeouts { pub const GET_MS: u32 = 500; /* … */ }
```

- `Reply` builds the reply frame: `ok` accepts only the method's output and `err` only its
  error type, so a reply of the wrong type does not compile. Both return the frame bytes.
  `StreamReply` builds `Item` frames and the final `End` (or `Err` before the first item
  when the method returns `Result<stream T, E>`).
- `Rejected` is a decoding failure; `Rejected::to_frame()` gives the `Fault` frame to send
  back (`Malformed`, `Unsupported` or `Invalid`), or `None` for a one-way method.
- As in proposal 0002, validation never runs by itself: `decode` verifies,
  `decode_validated` also validates.
- The code depends only on the `graven` runtime, through a new `graven::rpc` module (frame
  header, decoder, encoder, `Rejected`, the reply builders). It has no async code and works
  under `no_std` like the rest of the runtime.

The application owns everything around it, for example in an axum handler:

```rust
async fn inventory(State(db): State<Db>, body: Bytes) -> Vec<u8> {
    let call = match InventoryCall::decode_validated(&body, &CHECKS) {
        Ok(call) => call,
        Err(rejected) => return rejected.to_frame().unwrap_or_default(),
    };
    match call {
        InventoryCall::Get(req, reply) => match db.product(req.sku()) {
            Some(p) => reply.ok(|w| w.sku(p.sku).name(p.name).price_cents(p.price).stock(p.stock)),
            None => reply.err(|e| e.not_found().sku(req.sku())),
        },
        InventoryCall::Adjust(req, reply) => { /* … */ }
        InventoryCall::List(req, stream) => { /* … */ }
        InventoryCall::Ping(_) => Vec::new(),
    }
}
```

(The writer closures follow the existing `XW` API; the example shows the shape.)

### Client

The client has one method per service method. It builds the `Call` frame, hands it to a
**send function the application provides**, and decodes what comes back into typed
results. Graven ships no send function: the application writes it over whatever it uses,
and decides there about connections, authentication and retries.

**Rust.**

```rust
pub trait Sender {
    type Error;
    /// Sends a `Call` frame and returns the reply frames (empty for a one-way method).
    async fn send(&self, call: Vec<u8>, timeout_ms: Option<u32>) -> Result<Vec<u8>, Self::Error>;
    /// For streaming methods: the reply as it arrives. The default waits for `send`.
    async fn send_stream(&self, call: Vec<u8>, timeout_ms: Option<u32>)
        -> Result<impl ReplyStream<Error = Self::Error>, Self::Error>;
}

pub trait ReplyStream {
    type Error;
    /// The next chunk of reply bytes, `None` once the reply is complete.
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, Self::Error>;
}

let inv = InventoryClient::new(my_sender);
let p = inv.get(|w| { w.sku("cafe-500g"); }).await?;   // Result<Response<Product>, CallError<Response<LookupError>, _>>
let mut items = inv.list(|w| { w.prefix("cafe"); }).await?;
while let Some(p) = items.next().await { /* … */ }
```

`Response` owns the reply bytes and gives a verified view. `CallError` is the method's
`Err`, a `Fault`, a reply that does not match the method, or the sender's error. Streaming
methods return a `StreamResponse` whose `next()` yields each `Item` as soon as its frame has
arrived, so a stream that stays open (`watch`) works when the application implements
`send_stream`; with only `send`, the whole reply is read first. A reply that ends before its
`End` or `Fault` frame is reported as `Truncated`. Each call passes the declared timeout to
the sender by default and can override it.

**TypeScript** (after the TypeScript runtime exists):

```ts
const inv = new InventoryClient(async (call, timeoutMs) => {
  const r = await fetch("/inventory", { method: "POST", body: call,
                                        signal: AbortSignal.timeout(timeoutMs ?? 30_000) });
  return new Uint8Array(await r.arrayBuffer());
});
const r = await inv.get({ sku: "cafe-500g" });           // { ok: Product } | { err: LookupError }
```

## Evolution and the lockfile

The lockfile records each service and method:

```toml
[[services]]
name = "Inventory"
service_id = "0x…"
timeout_ms = 5000

[[services.methods]]
id = 1
name = "get"
input = "GetProduct"
output = "Product"
error = "LookupError"
shape = "unary"          # "unary", "stream" or "one-way"
timeout_ms = 500

[[services.methods]]
id = 5
reserved = true
```

| Change | Verdict |
|---|---|
| a new service, or a new method | allowed |
| a method renamed (same ID) | allowed: names are not in the bytes |
| a timeout added, changed or removed | allowed; listed by `gravenc diff` |
| a new variant in a union used as output or error | allowed (union rules); old clients read `Unknown(tag)` |
| a method replaced by `@n: reserved;` | warning `method-retired`: old clients will get `Unsupported`; confirm with `#[allow_breaking_readers]` |
| a method removed without reserving its ID | error `method-removed` |
| a reserved ID used by a method again | error `method-id-reused` |
| input, output or error type changed, or the shape changed (unary ↔ stream ↔ one-way, a `Result` added or removed) | error `method-changed` |
| a service removed or renamed | error `service-removed` (services are matched by name, like tables) |

The types a method uses keep evolving under their own rules: adding an optional field to
`GetProduct` is allowed, exactly as for any table. A method that may need errors later
should return `Result<T, E>` from the start, since adding a `Result` changes its shape.

## Other languages

Rust is the only generator today. The design keeps room for others without a rewrite:

- `--lang` joins `--side` as a generation option (`--lang rust` is the default; `ts` is
  the first addition). Every language generates the same two sides.
- Format and compatibility rules stay in the front-end (`parse`, `check`, `ir`, `lock`,
  `compat`). Generators only read the IR, so every language accepts and refuses exactly
  the same schemas and schema changes.
- Two parts of the compiler are tied to Rust today and move when a second language
  arrives: `names.rs`, which refuses names that collide in Rust (keywords, generated
  suffixes), becomes the Rust generator's own check, and each language gets its own; and
  `codegen/` becomes `codegen/rust/` next to `codegen/ts/`.
- Runtimes in other languages are correct when they pass the conformance vectors of the
  format and of the frame.

## What does not change

- Format v1, the envelope, `verify`, and the 42 conformance vectors.
- Schemas without services, and `gravenc gen` without `--side`: the same generated code
  as today.

## Alternatives considered

- **Requests and replies as plain unions, with no `service`.** It needs nothing new, but
  nothing would tie an operation to its reply and errors, nor say which methods stream or
  are one-way, and the client would have to build unions by hand. `service` keeps the same
  idea (a number selects the operation) and adds those guarantees.
- **`throws` lists** (as in Thrift). Rejected for `Result<T, E>`: the error becomes a named
  type that several methods can share, it evolves under the existing union rules, and it
  maps directly to `Result` in Rust and to discriminated unions in TypeScript.
- **Transport bindings, call IDs and deadlines in the frame** (as in gRPC). Rejected: they
  describe how bytes move, which differs per application and per transport. Applications
  that multiplex calls or propagate deadlines add that in their own channel.
- **Frames as Graven tables.** Rejected: a fixed header can be read before deciding where
  a frame goes, without the schema.

## Implementation order

Each step can be released on its own.

1. **Frame specification and vectors.** A normative `docs/spec/graven-rpc-v1.md` (frame
   layout, check order, kinds, reply sequences, the `graven.rpc` schema), the
   `graven::rpc` module (header, decoder, encoder), and conformance vectors in
   `testvectors/rpc-v1/` produced by `gen-vectors` and checked with `--check`.
2. **Services in the compiler.** Parser, checker, IR, lockfile, compatibility rules,
   `gravenc diff`, the language guide and the compatibility corpus.
3. **Rust server edge** (`--side server`): `decode`, `decode_validated`, `Reply`,
   `StreamReply`, `Rejected`.
4. **Rust client** (`--side client`): the `Sender` and `ReplyStream` traits and the generated client.
5. **TypeScript runtime** for format v1 and frames, tested against the vectors; then
   `--lang ts` for types and clients.

## Test plan

- Frame decoder: every step of the check order, a vector for each kind, several frames in
  one buffer, a union body with an unknown tag.
- Parser and checker: each rule in [Rules the checker enforces](#rules-the-checker-enforces),
  including a `stream Result`, a scalar input, a timeout on a one-way method, duplicate IDs
  and names, a reused reserved ID, and a service ID that collides with a table's.
- Compatibility corpus: one case for each row of the evolution table.
- Server edge: decoding each method; a frame for another service (`Unsupported`); an
  unknown method; a validation failure turned into a `Fault` `Invalid`; a reply of the
  wrong type failing to compile (a `trybuild` test).
- Client: a round trip for each shape through an in-memory send function connected to the
  server edge; an `Err`, a `Fault` and a reply for another method; the declared timeout
  passed to the send function and overridden per call.
- The format's conformance vectors stay unchanged.
