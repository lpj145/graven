# Changelog

## Unreleased

- **Services** ([proposal 0003](docs/proposals/0003-services.md), draft). `service`
  declarations with typed errors (`Result<T, E>`), streamed replies, one-way methods and
  timeouts; the bytes of a call and of a reply; lockfile rules for services; a generated
  client and the typed edge of a server, chosen with `--side`. Transports stay with the
  application. No change to format v1.

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
