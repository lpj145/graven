# Implementations

Every implementation of Graven builds on two language-neutral pillars: the
[format specification](spec/graven-format-v1.md) (plus [RPC frames v1](spec/graven-rpc-v1.md)),
which defines every byte, and the [conformance vectors](conformance-vectors.md) that pin exact
decoding behavior. Because all implementations pass the same vectors and canonical layout
rules, messages written in one language are read identically by all others.

## Supported SDKs

| Language | Distribution | Generate | Capabilities | Guide |
|---|---|---|---|---|
| **Rust** *(reference)* | `graven` crate (`no_std` + `alloc`) | `build.rs` or `gravenc gen` | Format v1, canonical write, validation, RPC client & server edge | [Rust SDK](sdk/rust.md) |
| **JavaScript / TypeScript** | Embedded `graven.js` + `graven.d.ts` (zero deps) | `gravenc gen --lang js` | Format v1, canonical write, validation, RPC client | [JS / TS SDK](sdk/js.md) |

## Adding a new language

`gravenc` already handles parsing, schema validation, layout computation, and lockfile
compatibility rules for every language in common:

1. **Read & verify:** implement format v1 + envelope (`GRVN`, CRC-32C) and pass all 42 vectors
   in `testvectors/v1/`.
2. **Canonical writer:** follow spec §10.2 so encoded payloads match the reference writer byte
   for byte.
3. **RPC frames (for services):** implement frame encoding/decoding and `graven.rpc.Fault`
   and pass all 18 vector pairs in `testvectors/rpc-v1/`.
4. **Add a code generator:** place `codegen/<lang>/` next to `codegen/rust` and `codegen/js`
   in `crates/graven-compiler`, reading only the checked IR + Lock.
5. **Document once:** add `docs/sdk/<lang>.md`, register it in `site/docs.py`, and add a tab
   to the inline examples in the guides.
