# The future

Graven is a small thing on purpose: a frozen wire format, a schema language with
enforced evolution, and a way to prove an implementation correct. Small foundations travel
far. This page describes where it can be applied, what already works for each area today,
and what would still need to be built. Nothing here is a promise or a schedule; it is a
map of where the format fits.

## Records that have to outlive their software

Accounting entries, invoices, fiscal documents, contracts, medical and audit records:
data that is written once, must never silently change meaning, and may have to be read
decades later, by software that does not exist yet.

**What already fits.** The format is frozen and specified down to the byte, with public
conformance vectors, so a record can be decoded without the program that wrote it. Each
envelope carries its schema id and a checksum, append-only files survive crashes, and
[`gravenc scan`](tools.md#scan) tells a cut-off write from real corruption.

**What would make it go further.** Archival conventions on top of the format: a manifest
that stores the schemas next to the data, periodic integrity reports, and signed
envelopes for records that must be tamper-evident.

## Libraries and SDKs shipped to other people

When code runs at a customer's site, you no longer choose when it upgrades. Ten versions
of a client may be talking to the same server at once.

**What already fits.** The lockfile and the compatibility rules guarantee that old and new
code read each other's messages without guessing; changes that would break that are
refused at build time, and [`gravenc diff`](tools.md#diff) shows the verdict of every
change before it ships.

**What would make it go further.** Implementations in more of the languages SDKs are
written in, such as Python, Go, Java, Kotlin, Swift and C#, next to the Rust and
[JavaScript / TypeScript](implementations.md) ones. Each one is measured against the same 42
vectors, so "compatible" means the same thing everywhere.

## Event streams and messaging

Payloads for Kafka, NATS, queues and webhooks are produced by one team and consumed by
many, often long after they were written.

**What already fits.** The schema id in every envelope identifies the payload without a
side channel. A consumer verifies a message once and reads fields in place, so routing
or filtering by one field does not decode the rest.

**What would make it go further.** A schema registry that serves lockfiles by schema id,
and connectors for common brokers.

## Storage engines, caches and indexes

Values in key-value stores, rows in embedded databases, memory-mapped files.

**What already fits.** Reads are zero-copy: a value fetched from storage, or a page of a
memory-mapped file, can be read field by field without allocation. Reading one field of
a large record costs only that field.

**What would make it go further.** Export of envelope files to columnar formats such as
Apache Arrow and Parquet for analytics, and secondary-index helpers that extract fields
without full decoding.

## Devices at the edge

Point-of-sale terminals, scales, sensors and vehicles that collect data while offline and
send it in batches when they can.

**What already fits.** The format needs no allocator and no alignment, the Rust runtime is
`no_std` with two dependencies, and envelopes can be concatenated into a batch that the
receiving side checks record by record.

**What would make it go further.** A C implementation for microcontrollers, and a
store-and-forward library that persists a queue of envelopes on flash.

## Web applications

Browsers and web servers exchange JSON because it is everywhere, not because it fits:
every response is parsed in full, numbers lose precision past 2^53, and nothing stops a
newer server from sending a shape that an older tab, still open since yesterday, cannot
read.

**What already fits.** The [JavaScript implementation](sdk/js.md) runs in browsers, Node
and Deno with no dependency: `gravenc gen --lang js` writes typed readers and writers, and
reads fields in place from a `fetch` response. It passes the same 42 vectors as Rust. The
compatibility rules cover the case JSON leaves to discipline: old tabs keep working while
the server moves ahead. `verify` protects the client from a malformed response, and the
schema id in the envelope says what arrived. A schema's [services](services.md) generate a
typed client, so a page calls the server through one endpoint with typed inputs, outputs
and errors.

**What would make it go further.** The server edge in JavaScript, for servers written in
Node or Deno, and a browser DevTools panel that shows binary responses as readable data.

**What to expect.** `JSON.parse` is highly optimized native code, so the gain is largest
for large messages, or when only a few fields of a response are read. With gzip or brotli
the size gap to JSON narrows. 64-bit integers become `BigInt` in JavaScript, which is
exact but less convenient than `number`.

## Plugins and sandboxes

Extensions written by third parties, often compiled to WebAssembly, that exchange data
with the host application.

**What already fits.** Buffers cross the boundary as bytes, with no shared object model,
and `verify` lets the host accept whatever a plugin returns without trusting it: depth
and total work are bounded, so a hostile buffer can only produce an error.

**What would make it go further.** A small guest-side library for WebAssembly modules and
a documented calling convention.

## Games and real-time state

Snapshots, replays, save files and network state, where reading must be fast and old
saves must still load after an update.

**What already fits.** Reading in place suits frame-by-frame state, and schema evolution
keeps old save files readable. The repository's `game` schema is used in the benchmarks.

**What would make it go further.** Delta encoding between snapshots, which would be a
library on top of the format rather than a change to it.

## Interchange between languages and teams

Data shared across services written in different languages, where JSON is used today
because it is everywhere.

**What already fits.** The schema language is language-neutral, the lockfile records every
layout a code generator needs, and [`gravenc to-json` and `from-json`](tools.md#to-json-and-from-json)
bridge to systems that only speak JSON. [Coming from JSON](coming-from-json.md) shows the
translation.

**What would make it go further.** Code generators for more languages, and importers from
JSON Schema, Protocol Buffers or OpenAPI definitions that produce a first `.graven` file.

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
