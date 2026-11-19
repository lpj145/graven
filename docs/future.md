# The future

This page maps out where the format fits well, what works today, and what is still missing
for each area. Nothing here is a commitment or a schedule.

## Records that outlive their software

Accounting entries, invoices, fiscal documents, contracts, medical and audit records are
written once and may need to be read decades later by software that does not exist yet.

Format v1 is frozen and specified down to the byte with public conformance vectors, so a
record can be decoded without the original program. Each envelope carries its `schema_id`
and a CRC32C checksum, append-only files survive crashes, and
[`gravenc scan`](tools.md#scan) separates a cut-off write from real corruption.

What is still open here is archival tooling on top of the format: manifests that bundle
definitions alongside stored files, periodic integrity checks, and signed envelopes for
tamper-evident records.

## Libraries and SDKs shipped to other people

When code runs on a customer's machine, you do not control upgrade timing. Ten client
versions may talk to the same server at once.

The `.lock` file and compatibility rules ensure old and new code decode each other's
messages without guessing. Breaking edits fail at build time, and
[`gravenc diff`](tools.md#diff) reports the verdict of every edit before a release.

To cover more ecosystems, the project needs runtimes in languages commonly used for client
SDKs (Python, Go, Java, Kotlin, Swift, C#) alongside Rust and
[JavaScript / TypeScript](implementations.md). Every runtime is checked against the same
42 format vectors.

## Event streams and messaging

Payloads in Kafka, NATS, queues, and webhooks are produced by one service and consumed by
many, often long after they were written.

Because every envelope carries its `schema_id`, consumers know what type a payload is
without an out-of-band header. A router or filter can verify a message once and read only
the fields it cares about without decoding the rest.

Missing pieces: a registry that serves `.lock` files by `schema_id`, and ready-made
connectors for common brokers.

## Storage engines, caches, and indexes

Values in key-value stores, rows in embedded databases, and memory-mapped files benefit
from zero-copy reads: a fetched value or mapped page is read field by field without
allocating, and reading one field of a large record skips everything else.

Useful additions on top of the format would be exporters from envelope files to columnar
formats like Apache Arrow or Parquet for analytics, and secondary-index helpers that
extract keys directly from raw buffers.

## Devices at the edge

Point-of-sale terminals, scales, sensors, and vehicles often collect data offline and flush
it in batches when connectivity returns.

The format needs no allocator and no alignment, the Rust runtime is `no_std` with two
dependencies, and envelopes concatenate naturally into batches that the receiver checks
record by record.

A small C runtime for microcontrollers and a store-and-forward queue for flash storage
would make this space much easier to target.

## Web applications

Browsers and servers default to JSON, which parses every response in full, loses integer
precision past 2^53, and gives no compile-time check when a server changes a response shape
while older tabs are still open.

The [JavaScript runtime](sdk/js.md) runs in browsers, Node, and Deno with zero
dependencies: `gravenc gen --lang js` produces typed readers, writers, and RPC clients that
read fields directly from a `fetch` response and pass the same 42 vectors as Rust.
Lockfile checks keep old tabs working as the server evolves, `verify` guards against
malformed responses, and [RPC frames](services.md#the-bytes) add 7 bytes per reply.

Trade-offs to keep in mind: `JSON.parse` is heavily optimized native code, so the speed
gain matters most for large payloads or partial reads. Over gzip or brotli, the size gap to
JSON narrows, and 64-bit integers map to `BigInt` in JavaScript, which is exact but more
verbose than `number`.

A JavaScript server edge for Node/Deno and a browser DevTools panel for inspecting binary
responses are still unbuilt.

## Plugins and sandboxes

Third-party extensions (often compiled to WebAssembly) exchange data across a trust
boundary with no shared object model. Buffers pass as plain bytes, and `verify` bounds
nesting depth and total traversal work so a hostile plugin output can only return an error.

A small guest-side helper crate for WebAssembly modules and a documented calling convention
would round this out.

## Games and real-time state

Snapshots, replays, save files, and network state need fast reads and old saves that keep
loading across patches. In-place reads fit frame-by-frame state, and lock-checked evolution
keeps save files readable (the repository's `game` schema is included in the benchmarks).

Delta encoding between snapshots could be built as a library on top of the format without
changing v1.

## Interchange between languages and teams

`.graven` definitions are language-neutral, `.lock` files hold every layout a code
generator needs, and [`gravenc to-json` / `from-json`](tools.md#to-json-and-from-json)
bridge to systems that only speak JSON (see [Coming from JSON](coming-from-json.md)).

Beyond additional language code generators, importers from JSON Schema, Protocol Buffers,
or OpenAPI would make migrating existing projects easier.

## What v1 already allows

The specification accepts more layouts than the reference writer produces: data in any
order, backward pointers and unreferenced bytes are all valid (§7.5, §10.2).
[Proposal 0006](proposals/0006-rebuild-and-alignment.md) uses that room, without a new
format version, for aligned numeric lists (`#[align(to = N)]`), envelopes padded to a cache
line or a page, and rebuilding a message with a few fields changed.

One idea in the same room is still open: **patching a stored message in place**. A string
of the same or a smaller size would be rewritten where it is; a larger one appended at the
end and its slot re-pointed; then the envelope's length and CRC recomputed. It saves a
rebuild on large records that change a little, but leaves unreferenced bytes that a later
`compact()` would have to remove, and a field with no slot (added by a newer schema) still
needs a rebuild.

## Reading without generated code, from the lockfile

A message does not describe itself: the envelope carries the root type's `schema_id`, not
the schema. Reading one without generated code works today in Rust, through
`graven_compiler::dynamic::Schema::load` and `graven_dynamic::read`, which is what
`gravenc inspect`, `to-json` and `scan` use; but it needs the whole compiler (parser and
checker) and the `.graven` source.

The lockfile already records every layout: types, slots, presence bits, defaults and schema
ids. A small dynamic reader that builds its model from the lock alone, in Rust and in
JavaScript, would let a generic viewer, an admin tool or a JSON bridge read any message
given only its lock, the role FlatBuffers' binary schemas (`.bfbs`) play there.

## How the format itself can evolve

Format v1 is frozen and will stay readable for good. Ideas that do not fit in v1 are
candidates for a future version, announced by a new `format_version` in the envelope, and
v1 messages remain valid alongside it:

- lists of lists and lists of unions;
- a native map type;
- optional compression of the envelope payload.

Each would come with its own frozen vectors, under the same rules as v1.

## Taking part

The most valuable contribution is an implementation in another language. The
[specification](spec/graven-format-v1.md) is the reference, the
[conformance vectors](conformance-vectors.md) are the test, and an implementation that
reads all 42 vectors correctly is a conforming one.
