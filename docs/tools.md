# Tools

`gravenc` is the schema compiler and also the tool for working with Graven data by hand:
looking inside files, converting to and from JSON, checking logs after a crash and
reviewing schema changes. Every data tool reads the `.graven` schema and its lockfile, so
it decodes exactly what generated code would.

| Command | What it is for |
|---|---|
| [`gravenc check`](#check) | Validate schemas and their compatibility with the lockfile (CI). |
| [`gravenc update`](#update) | Apply the compatibility rules and write the lockfile. |
| [`gravenc gen`](#gen) | Generate Rust, or JavaScript with TypeScript declarations. |
| [`gravenc diff`](#diff) | List what changed since the lockfile, with a verdict per change. |
| [`gravenc inspect`](#inspect) | Read the messages in a file. |
| [`gravenc to-json`](#to-json-and-from-json) | Decode messages as JSON. |
| [`gravenc from-json`](#to-json-and-from-json) | Encode JSON as messages. |
| [`gravenc scan`](#scan) | Check a file of envelopes: intact records, a cut-off tail, corruption. |

Install it with `cargo install graven-compiler`, or from a clone with
`cargo install --path crates/graven-compiler`.

Exit codes are the same for every command: `0` success, `1` an error or a problem found
in the data, `2` incorrect usage.

## check

```sh
gravenc check [--locked] <schema>...
```

Parses and checks each schema and, when a lockfile exists, the compatibility of the schema
with it. Schema ids are compared across all the schemas given: two differently named tables
with the same id would make their envelopes indistinguishable, so that is an error. Nothing
is written. Schemas that the given ones import are found under their directories and
checked with them. With `--locked`, a missing or out-of-date lockfile is an
error: this is the command for continuous integration.

```sh
gravenc check --locked schemas/*.graven
```

## update

```sh
gravenc update <schema>... [--init | --from <lock>]
```

The only way a lockfile changes. `update` compares the schema with the lockfile using the
[compatibility rules](spec/graven-format-v1.md#9-schema-compatibility) and rewrites the
lockfile only when there is no error and no unconfirmed warning.

- `--init` creates the first lockfile of a new schema.
- `--from <lock>` compares against another lockfile, for a schema that was moved or
  renamed. Without either flag, a schema with no lockfile is refused, so deleting a
  lockfile can never skip the compatibility check.

## gen

```sh
gravenc gen <schema> -o <dir> [--lang rust|js] [--side client|server|client,server]
```

Writes `<dir>/<schema name>.rs`, the same code the `build.rs` API produces, plus the code of
the schemas it imports. Most Rust projects use `build.rs` instead; see
[getting started](getting-started.md). With `--lang js` it writes `<schema name>.js` and
`<schema name>.d.ts`, plus the runtime `graven.js` and `graven.d.ts`; see
[JavaScript and TypeScript](javascript.md). `--side` adds the code of `service`
declarations: a client, the server edge, or both (JavaScript has the client only).

## diff

```sh
gravenc diff <schema> [--against <lock>]
```

A readable report of every change between the lockfile (or another lockfile given with
`--against`) and the schema, each with the verdict `update` would give. Use it to review a
schema change in a pull request.

```text
$ gravenc diff bank.graven
bank.graven compared with bank.graven.lock

table Entry
  ~ amount @1: type i64 → i32      error: type-changed

table Transaction
  ~ note @6: optional → reserved   allowed: reserved-optional
  + fee @7: i64 = 0                allowed: new-default
  + channel @8: string?            allowed: new-optional

enum Currency
  + ARS = 3                        allowed: new-enum-variant

5 change(s): 4 allowed, 1 blocking
```

Fields that only moved because an earlier field changed size are not listed separately:
the change that caused it is.

## inspect

```sh
gravenc inspect <schema> <file> [--root <Table>] [--raw]
```

Decodes every message in a file and prints it as indented JSON, with the offset and the
table of each one. A file that starts with an envelope is read as a sequence of envelopes,
and each message's table is found from its schema id. A bare message has no schema id, so
it needs `--root`. If the schema declares validation rules, `inspect` checks the built-in
rules (`range`, `len`, `finite`, `known`) and reports each `check(name)` as `not checked`.

```text
$ gravenc inspect bank.graven log.bin
#0  offset 0  bank.Transaction  129 bytes
{
  "id": 42,
  "ts": 1700000000,
  "currency": "USD",
  "from_shard": 3,
  "to_shard": 7,
  "entries": [
    {
      "account": 1001,
      "amount": -2500,
      "memo": "transfer"
    },
    {
      "account": 2002,
      "amount": 2500
    }
  ],
  "note": "rent"
}
#1  offset 129  bank.Transaction  67 bytes
…
```

Every message is fully verified before it is shown: a damaged message is reported with
its error instead of being half-printed.

## to-json and from-json

```sh
gravenc to-json   <schema> <file> [--root <Table>] [--raw] [--pretty]
gravenc from-json <schema> --root <Table> <input.json | -> -o <output | -> [--raw]
```

`to-json` prints one JSON document per message (JSON Lines), ready for `jq`, scripts or
other systems. `from-json` reads one or more JSON documents and writes them as a file of
envelopes, or as a single bare message with `--raw`. Decoding the output of `from-json`
with `to-json` gives the same documents back.

```sh
gravenc to-json bank.graven log.bin | jq 'select(.currency == "USD") | .id'
gravenc from-json bank.graven --root Transaction fixtures.jsonl -o fixtures.bin
```

The JSON follows the schema:

| Schema type | JSON |
|---|---|
| integers | numbers; `u128`/`i128` values outside the 64-bit range are decimal strings |
| `f32`, `f64` | numbers; `"NaN"`, `"inf"` and `"-inf"` for non-finite values |
| `bool` | `true` / `false` |
| enum | the variant name; a number for a value the schema does not know |
| `string` | string |
| `bytes` | lowercase hex string |
| struct, table | object, fields in declaration order |
| list | array |
| union | `{"Variant": { … }}`; `{"$tag": 5}` for a tag the schema does not know |

Absent optional fields are left out and absent fields with a default show the default.
On input, `null` is the same as leaving a field out.
`from-json` rejects documents that do not match the schema and names the place:
`document 0: Order.total: struct Money needs every field; currency is missing`.

## scan

```sh
gravenc scan <file> [--schema <schema>] [--truncate]
```

Checks a file of envelopes, such as an append-only log, from start to end. It reports how
many records are intact and what stopped the scan, if anything. With `--schema`, every
payload is also fully verified and records are counted per table.

A crash during a write leaves an incomplete record at the end of the file:

```text
$ gravenc scan orders.bin --schema bank.graven
orders.bin: 1 record(s) intact, 129 bytes
  bank.Transaction                 1
incomplete tail at offset 129: 57 bytes of a record that was cut short, which is what a crash during a write leaves behind.
Every record before it is intact. Run with --truncate to cut the file at offset 129.
```

`--truncate` removes that tail, so the file ends after the last intact record and new
records can be appended again. It never removes anything else.

Damaged data inside the file is a different situation, and `scan` says so:

```text
$ gravenc scan damaged.bin
damaged.bin: 0 record(s) intact, 0 bytes
corrupt data at offset 0: ChecksumMismatch: the record is complete but its CRC does not match (corruption).
The 0 record(s) before it are intact.
The next intact record starts at offset 129; 1 intact record(s) follow from there (129 bytes in between cannot be read).
--truncate only removes an incomplete tail, never corrupt data.
```

The distinction comes from the envelope's normative check order
([spec §8.4](spec/graven-format-v1.md#84-decoding-and-check-order-normative)): an
incomplete record and a corrupt one can never be confused.

## Editor support

The repository includes a Visual Studio Code extension in `editors/vscode` with syntax
highlighting for `.graven` schemas and `.graven.lock` files, comment toggling and bracket
matching. To install it:

```sh
cd editors/vscode
npx @vscode/vsce package
code --install-extension graven-0.1.0.vsix
```
