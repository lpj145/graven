# zidl v1: tests and robustness (Plan 3 of 3)

> **Historical document.** Written when the project was called zidl (later Graven). It is the
> plan that added the remaining conformance vectors, fuzzing, the generated evolution matrix
> and Miri. Names changed afterwards; the current description of the test suite is in
> [Testing](../../testing.md), and the normative text is the
> [format specification](../../spec/graven-format-v1.md).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (recommended: the user asked for lean execution) or superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close part E of the hardening design with four deliverables:
- frozen vectors for the rules of spec §10.3 that do not have a vector yet;
- fuzzing with `cargo-fuzz`, including a differential `Verified` × checked target;
- the `bank` v1/v2/v3 evolution matrix generated from the `.zidl` files;
- Miri over the runtime and the generated code.

The small pending items that are worth it also go in.

**Architecture:** Everything is tests or tooling around what already exists. The wire
format, the public API and the generated code do not change, with two exceptions:
- **Task 1:** adds tables to the conformance schema. It is a compatible change; the lock is
  updated with `zidlc update`.
- **Task 5:** fixes specific pending items.

Fuzz and Miri live in scripts (`scripts/fuzz.sh`, `scripts/miri.sh`) and are not part of
`cargo test --workspace`.

**Tech Stack:**
- Rust 2024 (stable 1.94 in the workspace) and nightly only for fuzz and Miri; nightly and
  Miri are already installed.
- `cargo-fuzz` + `libfuzzer-sys` + `arbitrary`. `cargo-fuzz` is not installed yet:
  `cargo install cargo-fuzz` uses the network.

**Spec:** `design/2026-11-12-zidl-v1-hardening-design.md` (part E) and
`docs/spec/zidl-format-v1.md` (§10.3 lists the rules without a vector). Records:
- the Plan 1 ledger: Task 8, which allows **adding** vectors and forbids changing existing
  ones;
- the Plan 2 ledger: pending items marked "Plan 3".

## Global Constraints

- **No git.** Where the template asks for a commit, run `cargo test --workspace` and move on.
- **Existing vectors are untouchable.** No existing file in `testvectors/v1/` changes
  (neither bytes nor `.json`). A new vector goes in only if the reference interpreter **and**
  the generated code already accept it with the expected result. If either disagrees, it is
  a bug to investigate: the vector does not adapt to the code.
- **The wire format does not change.** The `zidl` runtime keeps building with
  `--no-default-features`, and a new runtime dependency only if it is `no_std`.
- **No timing benchmarks** in this plan. No task measures performance.
- **Time limit per task:** about 45 min of work, plus the machine time stated in the step
  itself (fuzz 10 min per target in parallel; Miri). If it runs over: stop, record it in the
  ledger and tell the user. Do not keep repeating the same run.
- **Fuzz or Miri findings** are fixed at the cause, with a regression test in `cargo test`
  (the minimal input becomes a fixed case). A finding without a fix does not close the task.
- Messages and docs in Portuguese, like the rest of the project. *(Later translated to
  English.)*

## Review Focus

1. **`Verified` × checked divergence.** If `verified()` accepts a message, every accessor of
   the verified view returns exactly the same value as the checked accessor (`read()`). This
   holds also for required fields in a nested table, in a union variant and in a table list.
   → Task 2, the `differential` target over `conformance::Root` and `edge::Edge`.
2. **A new vector that the generated code reads differently from the interpreter.** The
   gate in `tests/generated.rs` must iterate over **all** vectors, including those of new
   tables, not only those of `Root`. → Task 1, `conformance_vectors_generated` with an
   updated minimum count.
3. **Evolution through the envelope.** v1, v2 and v3 have the same namespace and the same
   table name, so the `SCHEMA_ID` is equal: `from_bytes` of one version accepts an envelope
   of another, and the result follows the matrix. → Task 3, `envelope_cross_versions`.
4. **Input that crashes the compiler.** A schema mutated from a valid one (a valid prefix,
   deep nesting) never makes `parse`/`check`/`generate` panic. → Task 2, the `schema` target.
5. **Miri too slow.** This is solved by reducing cases (`PROPTEST_CASES`, sizes), never by
   skipping the tests that exercise `unsafe` (the `Verified` views, `VListR`, `VStrList` and
   `str_unchecked`). → Task 4.

## Decisions in this plan (by the assistant)

- **New tables in the conformance schema.** The new vectors use new tables in
  `conformance.zidl` (`Extra`, `Holder`), not new fields in `Root`. Touching `Root` would
  change the meaning of the frozen vectors. Adding a table is an allowed change (§9).
- **`TooLarge` (writer)** leaves the list of §10.3: it is not a reader rule, so it does not
  fit in a vector. It is already covered by `crates/zidl/tests/writer_limits.rs`. §10.3 will
  say so.
- **The "round 3" benchmark report** (part E): covered by `benchmarks/2026-11-12-envelope.md`
  (the CRC's cost by size) and `benchmarks/2026-11-13-codegen.md` (generated code). There is
  no new measurement.
- **Miri on `zidl-compiler`:** only library tests and integration tests that do not spawn
  processes (`parse`, `check`, `compat`, `lock`, `names`, `codegen`). `cli.rs` spawns
  processes, which Miri does not support. The compiler has no `unsafe`; it is included
  because the spec asks for it.
- **The writer's `checked_mul` and `should_panic`** (part E) were already done in Plan 1 and
  are not repeated.
- **Pending items left out** (cost higher than the risk for now):
  - a `[[retired]]` list in the lock: removing a table is already an error;
  - `Error` names frozen in the vectors (only the variant is normative);
  - diagnostic spans pointing at the keyword;
  - the compiler ↔ schemas dev-dependency cycle;
  - `Crate`/`Super` rejected in enums.

---

### Task 1: Vectors for the rules of §10.3

**Files:**
- Modify: `crates/zidl-conformance/schemas/conformance.zidl` and its `.lock` (via `zidlc update`)
- Modify: `crates/zidl-conformance/src/model.rs` (definitions of the new tables for the interpreter)
- Modify: `crates/zidl-conformance/src/vectors.rs` (new cases **after** the existing ones)
- Modify: `crates/zidl-conformance/tests/generated.rs` (gate: every vector, of any table)
- Create: `testvectors/v1/<name>.{bin,json}` (only the new ones)
- Modify: `testvectors/v1/README.md`, `docs/spec/zidl-format-v1.md` §10.3

**Interfaces:**
- Consumes: `vectors::cases()`, `Case`, `valid`/`invalid` (helpers in `vectors.rs`), `reader_schema(name)`, `evaluate(bytes, schema, enveloped)`.
- Produces: `pub const EXTRA: &str = "conformance.v1.Extra"` and `pub const HOLDER: &str = "conformance.v1.Holder"` in `vectors.rs`, and `reader_schema` recognizing both.

New tables (append at the end of `conformance.zidl`):

```zidl
struct Flags { on: bool, c: Color }
struct Empty {}

// Vectors of §10.3 that Root cannot reach without changing its meaning.
table Extra {
  flags  @0: Flags?;
  bools  @1: [bool]?;
  colors @2: [Color]?;
  names  @3: [string]?;
  blobs  @4: [bytes]?;
  pick   @5: Choice;        // required union
  leaves @6: [Leaf]?;
  e      @7: Empty?;        // zero-size slot
  @8: reserved u64;
  tail   @9: string?;       // a pointer after the reserved field
}

// Reads `[Leaf]` with an elem_fixed_len different from the reader's layout.
table Holder { items @0: [Leaf]; }
```

New vectors (name → message → expected result). Each error case has **one** defect over a
valid base, like the existing ones:

| vector | message | expect |
|---|---|---|
| `bool_struct_and_lists` | `flags = {on: 02, c: 1}`, `bools = [00, 01, 07]`, `colors = [0, 2, 9]`, `pick` = Leaf | value: `on = true`, `bools = [false, true, true]`, `colors = [Red, Blue, Unknown(9)]` |
| `strlist_null_elem` | `names = ["a", <ptr 0>, "c"]`, `blobs = [<ptr 0>]` | value: `names = ["a", "", "c"]`, `blobs = [[]]` |
| `union_tag_null_ptr_optional` | Root with `choice` tag 1 and pointer 0 | value: `choice` absent |
| `union_tag_null_ptr_required` | Extra with `pick` tag 1 and pointer 0 | error `Missing` |
| `union_unknown_tag_bad_ptr` | Root with `choice` tag 9 and a pointer beyond the buffer | error: the variant that the interpreter **and** the generated code give today (spec §6.2: `BadOffset` or `OutOfBounds`); record the chosen one in the README |
| `reserved_slot_partial` | Extra with `fixed_len` cutting slot `@8` in half | value: the fields before `@8`; `tail` absent |
| `ptr_slot_partial` | Extra with `fixed_len` cutting the pointer slot of `tail` in half | error `Truncated` |
| `table_list_elem_wider` | Holder with a `[Leaf]` whose `elem_fixed_len` = layout + 8 extra bytes | value: the `Leaf`s read, extras ignored |
| `table_list_elem_narrower` | Holder with `elem_fixed_len` covering only `n` | value: `Leaf` with `s` and `next` absent |
| `table_list_elem_zero` | Holder with `elem_fixed_len = 0` and 2 elements | value: 2 `Leaf`s with `n` absent; if the field is required in the reader ⇒ `Missing` (use the result the interpreter and the generated code agree on and record it) |
| `zero_size_slot_at_end` | Extra with `e` present and `fixed_len` ending exactly at the offset of `e` (`o == fixed_len`) | value: `e` present (the bit decides, §4.2) |
| `env_concat` | two `Root` envelopes concatenated | value: the first one (bytes after the CRC are not an error, §8.6) |
| `env_payload_len_short` | an envelope with `payload_len` one less, CRC recomputed over the new size | whatever §8.4 gives (expected: a value or a truncated-message error; record it) |
| `env_payload_len_long` | an envelope with `payload_len` larger than the buffer | error `Incomplete` |

- [ ] **Step 1:** Add the tables to `conformance.zidl`, run `cargo run -p zidl-compiler --bin zidlc -- update crates/zidl-conformance/schemas/conformance.zidl` and check that `check --locked` passes. Mirror `Extra`, `Holder`, `Flags` and `Empty` in `model.rs`, in the same format as `conformance_root()`.
- [ ] **Step 2:** Write the failing test. In `tests/conformance.rs`, `new_vectors_present` asserts that each name in the table above exists in `cases()` and in `testvectors/v1/`, and that `cases().len() == 27 + 14`.
- [ ] **Step 3:** Run `cargo test -p zidl-conformance new_vectors_present`. Expected: FAIL.
- [ ] **Step 4:** Add the cases in `vectors.rs`, after the last existing one, using `valid`/`invalid`/`encode_with`. For the table rows that say "record", the expected value is what the interpreter gives, checked against the spec's text. If the interpreter contradicts the spec, stop: it is a bug in the interpreter.
- [ ] **Step 5:** Generate with `cargo run -p zidl-conformance --bin gen-vectors`. Then check with `diff` that **no** pre-existing file in `testvectors/v1/` changed: compare against a copy made before Step 4 in the scratchpad (`vectors-before`).
- [ ] **Step 6:** In `tests/generated.rs`, change the per-schema dispatch to cover `Extra` and `Holder` (`to_value_extra`, `to_value_holder`) and assert that the number of vectors evaluated equals `cases().len()`. The existing mutation gate now includes the new valid vectors.
- [ ] **Step 7:** Update the documentation:
  - `testvectors/v1/README.md`: one line per new vector, with the recorded choices.
  - Spec §10.3: each covered rule cites its vector (`[vector: name]`); `TooLarge` becomes "a writer rule, covered by a runtime test, not by a vector".
- [ ] **Step 8:** Run `cargo test --workspace`. Expected: all green.

### Task 2: Fuzzing with `cargo-fuzz`

**Files:**
- Create: `fuzz/Cargo.toml` (crate `zidl-fuzz`, outside the workspace: an empty `[workspace]` in its own `Cargo.toml`), `fuzz/fuzz_targets/{read,verify,envelope,differential,schema}.rs`
- Create: `scripts/fuzz.sh`
- Create: `crates/zidl-conformance/tests/fuzz_regressions.rs` (only if there is a finding)
- Modify: `docs/zidl-language.md`, or create `docs/testing.md`, with how to run it

**Interfaces:**
- Consumes: `zidl_schemas::{bank,game,iot,stream}::*`, `zidl_conformance` (the generated code of `conformance.zidl`; expose it as `pub mod generated` if it is not public yet), `zidl_codegen_tests::edge` (likewise), `zidl::envelope::{decode, envelopes}`, `zidl_compiler::{parse, check, codegen::generate, lock::lock_from_ir}`.

Targets:

| target | input | property |
|---|---|---|
| `read` | bytes | `X::read` of each root (`bank::Transaction`, `game::Frame`, `iot::Telemetry`, `stream::Chunk`, `conformance::Root`, `Extra`, `edge::Edge`) plus **every** accessor, descending into tables, lists and unions, never panics |
| `verify` | bytes | `X::verified` of the same roots never panics; if it accepts, it walks every accessor of the `Verified` view |
| `envelope` | bytes | `decode` and `envelopes(..)` never panic; the iterator stops at the first error; `decode(encode(p))` returns `p` |
| `differential` | bytes | for `conformance::Root`, `Extra` and `edge::Edge`: if `verified(b)` is `Ok`, a `snapshot` function that reads every field through the verified view equals the same `snapshot` through the checked accessors of `read(b)` (which must give `Ok`) |
| `schema` | an arbitrary `String` and mutations of a valid schema | `parse` → `check` → `lock_from_ir` → `generate` never panic |

Details of the `schema` target:
- Use `arbitrary` with a 50% chance of starting from one of the repo's `.zidl` files and applying edits (delete, duplicate or swap a token, nest `[` 1000 times).
- Initial corpus for the byte targets: `testvectors/v1/*.bin`. Corpus for the `schema` target: the repo's `.zidl` files.

- [ ] **Step 1:** Run `cargo install cargo-fuzz` (needs the network). If it fails, stop and tell the user. Do not switch tools.
- [ ] **Step 2:** Create the `fuzz/` crate and the 5 targets. The "every accessor" walks and `snapshot` live in `fuzz/src/walk.rs`, one function per root, written from the schema. For nested required fields, use the accessor that returns `Result`.
- [ ] **Step 3:** Write `scripts/fuzz.sh [seconds=600]`:
  - copies the initial corpus;
  - runs the 5 targets **in parallel**: `cargo +nightly fuzz run <target> -- -max_total_time=$S -rss_limit_mb=2048`, with no `-dict`;
  - exits with a non-zero code if any target finds a crash and prints the artifact's path.
- [ ] **Step 4:** Do a short run, `scripts/fuzz.sh 60`. Expected: compiles and runs the 5 targets.
- [ ] **Step 5:** Do the acceptance run, `scripts/fuzz.sh 600` (about 10 min of wall clock, targets in parallel). Expected: no findings. For each finding:
  - minimize with `cargo +nightly fuzz tmin`;
  - turn the input into a test in `fuzz_regressions.rs`;
  - fix the cause;
  - repeat the run **only for the affected target**.
- [ ] **Step 6:** Document how to run it and the recommended time (≥ 1 h per target before a release). Run `cargo test --workspace`.

### Task 3: The `bank` v1/v2/v3 evolution matrix

**Files:**
- Create: `crates/zidl-schemas/tests/evolution.rs`

**Interfaces:**
- Consumes: `zidl_schemas::bank` (v1), `zidl_schemas::evo::{bank_v2, bank_v3}`, each with `Transaction::{read, verified, from_bytes}`, `TransactionW` and `TransactionW::seal`/`to_bytes`.

The matrix. Each cell is checked through both paths, `read` + accessors and `verified` + accessors:

| writer \ reader | v1 | v2 | v3 |
|---|---|---|---|
| v1 (every field, `note = "n"`) | equal | `from_shard = Some`, `fee = None` | same as v2 |
| v2 (`from_shard = Some(3)`, `fee = Some(-5)`) | ok, `note = None` | equal | ok, `to_shard` not exposed |
| v2 (`from_shard` omitted) | `Missing { field: "from_shard" }` | ok, `from_shard = None` | ok |
| v3 (`to_shard` omitted, since it is reserved) | `Missing { field: "to_shard" }` | `Missing { field: "to_shard" }` | ok |

- [ ] **Step 1:** Write `evolution_matrix`, with one function per writer that builds the message and one assertion per cell of the table.
- [ ] **Step 2:** Write `envelope_cross_versions`:
  - assert that `bank::Transaction::SCHEMA_ID == bank_v2::Transaction::SCHEMA_ID == bank_v3::Transaction::SCHEMA_ID`;
  - repeat the matrix with `to_bytes` on the writer and `from_bytes` on the reader; the result must be the same as the corresponding row.
- [ ] **Step 3:** Run `cargo test -p zidl-schemas --test evolution`. Expected: PASS on the first try, because the behavior already exists. If any cell diverges, the matrix above is right per spec §4.2/§9, and the divergence is a bug.

### Task 4: Miri

**Files:**
- Create: `scripts/miri.sh`
- Modify: tests with proptest or large buffers, only to reduce cases under `cfg(miri)`

- [ ] **Step 1:** Write `scripts/miri.sh`, which runs:

  ```
  MIRIFLAGS="-Zmiri-strict-provenance" PROPTEST_CASES=8 cargo +nightly miri test \
    -p zidl -p zidl-layout -p zidl-schemas -p zidl-codegen-tests -p zidl-conformance
  ```

  And then, separately:

  ```
  cargo +nightly miri test -p zidl-compiler --lib \
    --test parse --test check --test compat --test lock --test names --test codegen
  ```

  `cli.rs` is left out because it spawns processes.
- [ ] **Step 2:** Run `scripts/miri.sh 2>&1 | tee …/scratchpad/miri.log` in the background. Limit: 40 min of wall clock.
  - A test that does not finish within the limit: reduce it under `#[cfg(miri)]` (fewer iterations, smaller buffers). Never use `#[cfg_attr(miri, ignore)]` on a test that exercises `Verified`/`VListR`/`VStrList`/`str_unchecked`/the envelope.
  - `#[cfg_attr(miri, ignore)]` only for tests that use files/processes (for example `include_bytes` does not need it, but `std::fs` does), listed in the ledger.
- [ ] **Step 3:** Expected: no UB reports. For each report: a regression test, a fix at the cause and a new run of only the affected package.

### Task 5: Small pending items

**Files:** each item names its own file.

- [ ] **Infinite float literal:**
  - Problem: `1e999999` becomes `inf` as a default.
  - Fix: `check` refuses it with "float literal out of range" (`crates/zidl-compiler/src/check.rs`).
  - Test: `float_literal_overflow_rejected` in `tests/check.rs`.
- [ ] **Offset overflow on 32 bits:**
  - Problem: `first + size` in `TableW::tables`/`tables_fixed` (`crates/zidl/src/write.rs`) can overflow on 32 bits.
  - Fix: `checked_add` arithmetic with the same documented programmer panic (`"list too large"`).
  - Test: a `should_panic` in `tests/writer_limits.rs` with `n`/`elem_fixed_len` that would overflow `u32`.
- [ ] **Exposed CRC:**
  - Problem: `#[doc(hidden)] pub mod crc` exposes `zidl::crc::*`.
  - Fix: make the module private. If the bench or the tests use it, expose only what is needed via `zidl::envelope`. For `sw_matches_accel`, use `#[cfg(feature = "std")]`.
- [ ] **Old names:** rename the `*_hand_written` tests (`zidl-compiler/tests/check.rs`, `codegen.rs`) to names that say what they compare today. Delete `layouts_match_hand_written` if it is tautological; if not, document what it still proves.
- [ ] **Charging order in verify:**
  - Problem: the generated code does `charge → enter`, the interpreter does `enter → charge`.
  - Fix: one sentence in spec §7.3 saying that the order between charging and entering is not normative, because it only changes which of `BudgetExceeded`/`DepthExceeded` comes out when both overflow together.
- [ ] Run `cargo test --workspace`, `cargo test -p zidl --no-default-features` and `cargo clippy --workspace --all-targets`. Expected: green, and clippy with no new warnings.

### Task 6: Wrap-up

- [ ] Run `cargo test --workspace`, `scripts/fuzz.sh 60` (smoke) and `zidlc check --locked` on every `.zidl`.
- [ ] Do a single final review, on a capable model, of the whole plan's diff. Fix the critical and important findings directly; record the minor ones.
- [ ] Update spec §10.3, `testvectors/v1/README.md` and the testing doc. The ledger ends with the list of rulings.
