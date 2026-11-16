# Testing

## On every change

```sh
cargo test --workspace                       # ~1 min
cargo test -p graven --no-default-features   # no_std runtime
cargo clippy --workspace --all-targets
target/debug/gravenc check --locked $(find crates -name '*.graven' -not -path '*/tests/compat/*' -not -path '*/evo/*')
for f in crates/graven-schemas/schemas/evo/*.graven; do target/debug/gravenc check --locked "$f"; done
```

`cargo test` covers:
- the frozen vectors (`testvectors/v1/`, 42 vectors) against the conformance interpreter
  and against the generated code;
- the mutation gates (every single-byte mutation of every vector is read identically by
  the interpreter and the generated code);
- the `bank` v1/v2/v3 evolution matrix (`crates/graven-schemas/tests/evolution_matrix.rs`);
- the frozen RPC frame vectors (`testvectors/rpc-v1/`, 18 vectors), the fault schema's
  layout, and the generated service client and server edge talking to each other;
- the compiler's compatibility corpus, including version chains.

The JavaScript implementation is tested by `./scripts/ts.sh` (Node >= 20; the first run
installs the TypeScript compiler in `ts/`): the same vectors, byte-for-byte writing against Rust, validation, RPC frames,
`.d.ts` declarations against the `.js` exports, and a client calling the generated Rust server
([details](sdk/js.md#verification-suite-scriptstssh)).

## Fuzzing (nightly + `cargo-fuzz`)

```sh
cargo install cargo-fuzz      # once
scripts/fuzz.sh               # 600 s per target, all 5 in parallel
scripts/fuzz.sh 60 read       # one target, 60 s
```

Targets run with `-O -a` (optimized, with debug assertions and overflow checks).

| target | what it guarantees |
|---|---|
| `read` | `read` of every root plus every accessor, over arbitrary bytes, never panics |
| `verify` | `verified` plus a walk of the verified view never panics |
| `envelope` | `decode`/`envelopes` never panic; the iterator stops at the first error; round trips are stable |
| `differential` | if `verified` accepts, the verified view reads exactly the same as the checked reader and as `reader()`, with no errors |
| `schema` | `parse` → `check` → `generate` never panic (arbitrary text and mutations of valid schemas) |

Roots covered:
- `conformance::{Root, Extra, Holder}`;
- `edge::Edge`;
- `bank::Transaction`, `game::Frame`, `iot::Telemetry`, `stream::Chunk`.

The seed corpus is the vectors and the repository's `.graven` files. A crash lands in
`fuzz/artifacts/<target>/`. For each finding:
1. minimize it with `cargo +nightly fuzz tmin <target> <file>`;
2. store the input in `fuzz/regressions/`;
3. write a regression test in `cargo test`;
4. fix the cause.

In the walkers (`fuzz/src/lib.rs`), each list is walked up to `MAX_ELEMS` elements.
Without `verify`, a `[Table]` with `elem_fixed_len = 0` can announce ~2³² elements in a
few bytes. That is legal for lazy reads (spec §7.1); `verify` stops it through the
budget.

Recommendation: before each release, run ≥ 1 h per target (`scripts/fuzz.sh 3600`).

## Miri (nightly)

```sh
scripts/miri.sh               # ~25 min on a desktop machine
```

Packages and configuration:
- Miri runs over `graven`, `graven-layout`, `graven-schemas`, `graven-codegen-tests` and
  `graven-conformance`.
- In `graven-compiler`, it runs over `--lib`, `parse`, `check`, `compat`, `lock` and
  `names`.
- Flags: `-Zmiri-strict-provenance -Zmiri-disable-isolation`. proptest writes
  regression files, so isolation is disabled.

Under `cfg(miri)` the mutation gates and proptest use smaller samples: the code exercised
is the same, with fewer cases. No test of `Verified`, `VListR`, `VStrList`,
`str_unchecked` or the envelope is skipped.

Two compiler test files stay out of Miri (the compiler has no `unsafe`):
- `cli.rs`, because it spawns processes;
- `codegen.rs`, because `prettyplease` over whole schemas takes hours under Miri.
