# Conformance vectors

Two sets of vectors define conformance: one for the format, frozen like format v1, and one
for the frames of services, which may still change while services are in alpha. Each is a
directory of `.bin` files, each with a `.json` describing the expected result.

| Set | Pins | Download | In the repository |
|---|---|---|---|
| Format v1 | [format v1](spec/graven-format-v1.md): reading, verification, envelope | [`.tar.gz`](../graven-testvectors-v1.tar.gz) | `testvectors/v1/` |
| RPC frames v1 | [RPC frames v1](spec/graven-rpc-v1.md): frame decoding and faults | [`.tar.gz`](../graven-testvectors-rpc-v1.tar.gz) | `testvectors/rpc-v1/` |

## Format v1

{{#include ../testvectors/v1/README.md:body:1}}

## RPC frames v1

{{#include ../testvectors/rpc-v1/README.md:body:1}}
