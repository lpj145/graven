# Proposal 0001: imports between schema files

Status: **accepted and implemented** (see [Imports](../graven-language.md#7-imports)).

## Motivation

Every `.graven` file is self-contained today. Types that many schemas need — an identifier,
a money amount, a timestamp, an address — have to be copied into each file, and nothing
checks that the copies stay identical.

## Decisions

- **Imports name a namespace, not a file.** The compiler is given every schema file of the
  project (as `build.rs` and `gravenc check` already are), finds the file that declares
  the namespace, and resolves it. Files can be moved or renamed freely.
- **One namespace, one file.** Two files declaring the same namespace in one compilation
  is an error.
- **No versions in namespaces.** Compatible changes need no version: fields, variants and
  reserved IDs evolve a type in place, guarded by the lockfile. An incompatible change is
  a new type (`OrderV2`), which gets its own schema id; old and new can coexist.
- **Imported types are always qualified** by the last segment of their namespace:
  `import acme.common;` → `common.Uuid`. When two imports end in the same segment, an
  alias is required: `import acme.common as ac;`.
- **All type kinds can be imported**: structs, enums, tables and unions.
- **Generated Rust refers to `super::<namespace segment>`** by default
  (`super::common::Uuid`); `build.rs` can override the path.

## Syntax

```graven
// common.graven
namespace common;

/// RFC 4122 identifier, stored inline as 16 bytes.
struct Uuid { hi: u64, lo: u64 }

/// An amount in minor units of an ISO 4217 currency.
struct Money { cents: i64, currency: u16 }
```

```graven
// orders.graven
namespace orders;

import common;

table Order {
  id    @0: common.Uuid;
  total @1: common.Money;
}
```

`import` lines come after `namespace` and before any declaration. Unqualified names refer
only to the file's own declarations, so adding a type to `common` can never change the
meaning of a name in another file.

## Resolution and checks

- An import whose namespace is not among the files compiled is an error that names it.
- A file sees the types declared in the namespaces it imports, not their own imports.
- Import cycles are an error, reported with the chain.
- An alias that collides with a declared type name, or two imports with the same name,
  is an error.

## Namespaces and schema ids

A table keeps the namespace of the file that declares it: `common.Address` sent on its own
has the schema id of `"common.Address"`, wherever it is used. The existing schema id
collision check covers imports unchanged.

## Lockfiles and compatibility

Each file keeps its own lockfile. The importing file's lock also records, under an
`imports` section, the layout of every imported type it uses (directly or through other
imported types). `common`'s own lock protects `common` against itself; the copy in
`orders.graven.lock` makes `gravenc update orders.graven` refuse a change to `common` that
would break `orders`, even when `common` was updated in a separate change. In a pull
request that changes `Money`, the diff of every dependent lock shows who is affected.

Compatibility rules apply to imported types exactly as to local ones. Two new rules:

| Rule | When |
|---|---|
| `import-removed` | a type the file uses no longer exists in the imported namespace |
| `import-changed` | an imported type changed in a way the existing rules refuse; the message names the rule and the namespace that owns the type |

## Generated code (Rust)

Each file generates its own module, as today. The importing module refers to the imported
one as `super::<segment>`:

```rust
pub mod common { include!(concat!(env!("OUT_DIR"), "/common.rs")); }
pub mod orders { include!(concat!(env!("OUT_DIR"), "/orders.rs")); }
```

`orders::Order::total()` returns `common::Money`: the same Rust type everywhere it is used.

## Tools

Commands that take a single schema (`inspect`, `to-json`, `from-json`, `diff`, `update`)
find imported namespaces among the `.graven` files in the schema's directory and its
subdirectories, or in directories given with `-I <dir>`.

## Alternatives considered

- **Importing files by path**: ties schema text to file names; moving a file breaks every
  importer. Rejected.
- **One namespace spread over several files** (Java packages): flexible, but makes
  ownership of a type and its lockfile ambiguous. Rejected.
- **One lockfile for the whole repository**: every schema change would touch one shared
  file, and per-file review would be lost. Rejected.

## Test plan

- Parser: `import` syntax, placement, aliases.
- Checker: qualified resolution, unknown namespace or type, cycles, alias collisions,
  duplicate namespaces.
- Compatibility corpus: `common` changes `Money` and the dependent file's `update` is
  refused with `import-changed`; an imported type removed → `import-removed`.
- Codegen: a compiled crate with two files sharing a struct, an enum and a table, where the
  importing file's accessors return the imported Rust types.
- Tools: `inspect` and `to-json` on a message whose fields use imported types.
- Conformance vectors are unaffected (no format change).
