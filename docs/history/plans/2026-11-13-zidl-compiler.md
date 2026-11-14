# zidl v1: the `zidlc` compiler + lockfile (Plan 2 of 3)

> **Historical document.** Written on 2026-11-13, when the project was called zidl (later
> Graven, with the compiler `gravenc`). It is the plan that built the compiler and the
> lockfile. Names and some details changed afterwards (for example, the lockfile became
> `<schema>.graven.lock` and namespaces no longer carry versions); the current references are
> the [language guide](../../graven-language.md) and the
> [format specification](../../spec/graven-format-v1.md).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `.zidl` → Rust compiler (`zidlc` + a `build.rs` API) with a lockfile that refuses
incompatible changes. It generates exactly the pattern that is hand-written in
`zidl-schemas` today. At the end, `zidl-schemas` uses only generated code, and the generated
code of `conformance.zidl` passes every frozen vector.

**Architecture:** First the generatable pattern is **frozen by hand** (Task 1): the support
moves to the runtime, there is a single kind of view, the header is charged by the caller and
every `unsafe` primitive lives in the runtime. Then the compiler is built in pure layers:
- `parse` (text → AST);
- `check` (AST → resolved IR with layout);
- `lock` (IR ↔ TOML);
- `compat` (old lock × new IR → errors/warnings);
- `codegen` (IR → Rust, via `quote` + `prettyplease`).

The CLI and the `build.rs` API only orchestrate those layers. The switch in `zidl-schemas`
only happens with the current tests passing without changing assertions and the benchmark
within ±5%.

**Tech Stack:** Rust 2024; in the compiler: `serde` + `toml` (lockfile), `quote` +
`proc-macro2` + `syn` + `prettyplease` (codegen), `proptest` (dev). No new dependencies in the
runtime.

**Spec:** `design/2026-11-12-zidl-v1-hardening-design.md` (part D) and the normative spec
`docs/spec/zidl-format-v1.md` (§3.3 layout, §4 presence, §5–6 types, §7 verify, §8 envelope,
§9 compatibility). Decisions inherited from Plan 1 (record: the Plan 1 progress ledger, lines
"Final: Ruling" and "CARRY TO PLAN 2").

## Global Constraints

- No git: no commits and no branches. Where the template asks for a commit, run
  `cargo test --workspace` and move on.
- `testvectors/v1/` is frozen: no existing file changes. This plan adds no vectors (that is
  Plan 3's job).
- The wire format does not change. Generated messages are byte-for-byte equal to those of the
  hand-written code for the same calls.
- The `zidl` runtime keeps building with `--no-default-features`.
- No `unsafe` function reachable by safe code outside `#[doc(hidden)]`. All `unsafe` lives in
  the `zidl` runtime. The generated code has no `unsafe` blocks of its own: it only calls
  runtime `unsafe` functions with a fixed contract and is emitted with `#![deny(unsafe_code)]`
  + `#[allow(unsafe_code)]` per item.
- Performance with generated code: within ±5% of the current numbers
  (`benchmarks/2026-11-12-envelope.md`, post-fix tables), measured with interleaved A/B runs
  against the pre-plan state.
- The generated code keeps the 6 "Invariants for the code generator" (the doc of
  `crates/zidl-schemas/src/lib.rs`, which in Task 1 moves to `zidl::rt`).
- Compiler error messages: `file:line:column: message`, in Portuguese. *(Later changed to
  English.)*

## Review Focus

1. A schema with a recursive type (`Leaf.next: Leaf?`) ⇒ layout, lock and codegen terminate
   (no infinite recursion) and the generated reader gives `DepthExceeded` on the `self_cycle`
   vector. → Task 6, test `conformance_vectors_generated`.
2. The same bytes reached through two edges as different types ⇒ the generated view only
   exists through the edge that `verify` walked. → Task 6, `compile_fail` doctest + review
   of the invariants.
3. A lockfile edited by hand in an invalid way (broken TOML, missing field, unknown
   `format`) ⇒ a clear error with the file path, never a panic. → Task 4,
   `lock_rejects_malformed`.
4. `build.rs` without a lockfile ⇒ an error telling you to run `zidlc update <schema>`,
   without writing anything into `src/`. → Task 7, `build_missing_lock_errors`.
5. A union with an unknown tag in a verified message ⇒ the view returns `Unknown(tag)` and
   does not descend. → Task 6, the `unknown_union_tag` vector via generated code.

## Decisions in this plan (by the assistant)

- **Dependencies between crates:** `zidl-compiler` has `zidl-schemas` and `zidl-conformance`
  only as **dev-dependencies** (parity tests in Tasks 3 and 6). In Task 8, `zidl-schemas`
  gets `zidl-compiler` as a **build-dependency**. Cargo accepts the cycle because it goes
  only through dev-deps. If it does not, the parity tests move to `zidl-schemas/tests`. In
  Task 8, `generated_equals_hand_written_bytes` loses its hand-written reference: it is
  deleted, and from then on the Task 6 snapshots guard against regressions.
- **Header charged by the caller** (Final review #2): `T::verify` never charges its own
  header. `verified()` charges `2 + fixed_len` for the root. A table/union field charges
  `2 + fixed_len` of the target before calling the child's `verify`. `ListR::verify` charges
  the elements (as today). The totals are identical to spec §7.3.
- **A single kind of view** (#3): every table generates `XVerified<'a>` with explicit
  accessors (never `Deref`) and `pub fn reader(&self) -> X<'a>`. A string accessor uses
  `str_unchecked`, a nested table accessor returns `YVerified`, a table list accessor returns
  `VListR<YVerified>`, a `[string]` accessor returns `VStrList`, a union accessor returns an
  enum `UVerified` whose unknown variant is `Unknown(u16)` (it does not descend).
- **All `unsafe` in the runtime** (#6): `zidl::rt` contains `FromVerified`, `VListR`,
  `VStrList` and the function `unsafe fn str_unchecked`. The generated code only calls them,
  with `// SAFETY:` citing the invariant.
- **Every table is "root-capable"**: it generates `SCHEMA_ID`, `read`, `verified`,
  `from_bytes` and `to_bytes`. There is no `root` marker in the language.
- **Typed envelope writing** (#5): `XW::to_bytes(b: &mut Builder<'_>, f: impl FnOnce(&mut XW<'_>)) -> Result<Bytes, Error>`
  (builder in envelope mode; uses `X::SCHEMA_ID`). `XW::seal(b, f) -> Result<(), Error>` is
  the variant for batches. The name `from_bytes` stays (the user asked for "to and from
  bytes"); the doc says it expects an envelope.
- **`schema_id`**: the first time the table appears, the FNV-1a of `"<namespace>.<Table>"`.
  From then on it is **read from the lock**. Renaming a table is not supported in v1: a table
  in the lock missing from the schema ⇒ the error "table removed or renamed".
- **Reserved field**: syntax `@n: reserved;` or `@n: reserved T;`. Without a lock, the type
  is mandatory. With a lock, if the type is given in the schema, it must match the lock.
  `conformance.zidl` becomes `@21: reserved u32;` (#9).
- **Contiguous IDs**: the IDs of a table MUST be `0..n` without gaps (a gap = a field
  removed without `reserved` ⇒ an error).
- **Confirming a warning**: the attribute `#[allow_breaking_readers]` on the line of the
  reserved field that used to be required.
- **The lock is never written by `build.rs`**: `build.rs` only reads the lock and fails if it
  is missing or would need to change. Only `zidlc update` writes the lock. So a `cargo build`
  never changes versioned files.
- **`[bool]`, `[enum]`, an enum in a struct, a bool in a struct** follow spec
  §5.2/§5.3/§6.3 (1 byte for bool). The runtime gets `impl Le for bool` with the spec's read
  rule.

## File Structure

| File | Responsibility |
|---|---|
| `crates/zidl/src/rt.rs` (new) | support for generated code, moved from `zidl-schemas/src/lib.rs` + `Req::Union`, `VStrList`, the invariants doc |
| `crates/zidl/src/le.rs` (mod) | `impl Le for bool` |
| `crates/zidl-schemas/src/*.rs` (mod, Task 1; removed in Task 8) | the final pattern, hand-written |
| `crates/zidl-compiler/` (new) | lib + bin `zidlc` |
| `…/src/ast.rs`, `…/src/parse.rs` | AST with spans; hand-written lexer + recursive-descent parser |
| `…/src/ir.rs`, `…/src/check.rs` | resolved IR + layout (`zidl-layout`) |
| `…/src/lock.rs` | TOML lockfile ↔ IR, `schema_id` |
| `…/src/compat.rs` | the rules of spec §9 |
| `…/src/codegen/{mod,read,write,view}.rs` | IR → `TokenStream` → `prettyplease` |
| `…/src/build.rs` (lib), `…/src/main.rs` | `build.rs` API, CLI |
| `…/tests/{parse,check,lock,compat,codegen}.rs`, `…/tests/compat/<case>/`, `…/tests/snapshots/` | tests |
| `crates/zidl-schemas/build.rs`, `schemas/*.zidl.lock`, `schemas/evo/*.zidl` | switch to generated code |
| `crates/zidl-conformance/build.rs`, `schemas/conformance.zidl.lock` | generated code of `conformance.zidl` for the vectors gate |
| `docs/zidl-language.md` (new) | reference for the `.zidl` language and `zidlc` |

---

### Task 1: Freeze the generatable pattern by hand

**Files:**
- Create: `crates/zidl/src/rt.rs`
- Modify: `crates/zidl/src/lib.rs` (`pub mod gen;` with `#[doc(hidden)]` on the items only generated code uses), `crates/zidl/src/le.rs`, `crates/zidl/src/read.rs` (the doc of `str_unchecked` points to `gen`), `crates/zidl-schemas/src/{lib,bank,game,iot,stream}.rs`, `crates/zidl-schemas/src/evo/*.rs`, `crates/zidl-bench/src/zidl_impl.rs`
- Test: `crates/zidl/tests/rt.rs` (new), the existing `zidl-schemas` tests

**Interfaces:**
- Produces (`zidl::rt`, which everything else consumes):
  - moved: `Req`, `check_required`, `ReqMask`, `mask_for`, `check_required_fast`, `FromTable`, `ListR`, `Elem`, `ListW`, `StreamW`, `FromVerified`, `VListR`, `open_envelope`; the "Invariants for the code generator" doc goes with them.
  - new: `Req::Union { name: &'static str, off: u16 }` (present ⇔ slot inside, `tag != 0` and pointer `!= 0`, spec §4.3).
  - new: `pub struct VStrList<'a>` with `#[doc(hidden)] pub unsafe fn new(inner: Option<StrList<'a>>) -> Self`, `len`, `is_empty`, `get(i) -> Result<&'a str, Error>` (without validating UTF-8), `get_bytes(i)`, `iter`.
  - `TableRef::str_unchecked` stays `#[doc(hidden)] pub unsafe` (generated code calls it).
  - `impl Le for bool` (1 byte; the read rule for values ≠ 0/1 as in spec §5.3/§6.3).
- Produces (the pattern in `zidl-schemas`, which the Task 6 codegen reproduces):
  - `T::verify` does not charge its own header. `X::verified` does `ctx.charge(2 + fixed_len)` for the root before `verify`.
  - every view has explicit accessors + `reader()`. `FrameVerified::entities() -> VListR<EntityVerified>` (`EntityVerified` is now produced).
  - `XW::to_bytes(b, f)` / `XW::seal(b, f)` on every table, and `pub const SCHEMA_ID` as an associated const `X::SCHEMA_ID` (the module const stays, so tests do not break).

- [ ] **Step 1: Failing tests**:
  - `rt.rs` / `req_union_missing_when_ptr_zero`: a raw table with a union slot `tag=1, ptr=0` ⇒ `check_required(&t, &[Req::Union{name:"u", off:0}])` gives `Missing{field:"u"}`. `tag=0` ⇒ `Missing`. A valid tag and ptr ⇒ `Ok`.
  - `rt.rs` / `vstrlist_reads_without_validation_after_verify`: a list written with `set_strs` read by `VStrList` gives the same strings as `StrList::get`.
  - `zidl-schemas/tests/verified.rs` / `frame_verified_entities_are_views`: `Frame::verified(buf)?.entities()?.get(0)?.id()` == the checked one.
  - `zidl-schemas/tests/verified.rs` / `to_bytes_roundtrip_typed`: `TransactionW::to_bytes(&mut b, |t| { … })` ⇒ `Transaction::from_bytes` ok; the envelope's `schema_id` is `Transaction::SCHEMA_ID`.
  - `zidl-schemas/tests/verified.rs` / `budget_totals_unchanged`: for fixed bank and game messages, the `VerifyCtx::budget()` remaining after `verified` equals the value measured **before** the change (run it and record the number in the test before touching anything).
- [ ] **Step 2:** `cargo test --workspace` → the new tests fail.
- [ ] **Step 3:** Implement. `zidl-schemas/src/lib.rs` becomes `pub use zidl::rt::*;` (no code of its own).
- [ ] **Step 4:** `cargo test --workspace` passes, including the mutation gate `verified_matches_checked_under_mutation`. A grep for `unsafe` in `crates/zidl-schemas/src` shows only `#[allow(unsafe_code)]` + calls to runtime functions, no `from_utf8_unchecked`.
- [ ] **Step 5:** `cargo bench -p zidl-bench --bench codec --no-run` compiles. The ±5% measurement is left for Task 8.

### Task 2: The `.zidl` parser

**Files:**
- Create: `crates/zidl-compiler/{Cargo.toml,src/lib.rs,src/ast.rs,src/parse.rs}`
- Test: `crates/zidl-compiler/tests/parse.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Span { pub line: u32, pub col: u32 }
  pub struct Schema { pub namespace: (String, Span), pub items: Vec<Item> }
  pub enum Item { Enum(EnumDecl), Struct(StructDecl), Table(TableDecl), Union(UnionDecl) }
  pub struct EnumDecl { pub name: String, pub base: TypeRef, pub values: Vec<(String, i128, Span)>, pub span: Span }
  pub struct StructDecl { pub name: String, pub fields: Vec<(String, TypeRef, Span)>, pub span: Span }
  pub struct TableDecl { pub name: String, pub fields: Vec<FieldDecl>, pub span: Span }
  pub struct FieldDecl { pub id: u16, pub name: Option<String> /* None = reserved */, pub ty: Option<TypeRef>,
                         pub optional: bool, pub default: Option<Literal>, pub attrs: Vec<String>, pub span: Span }
  pub struct UnionDecl { pub name: String, pub variants: Vec<(String, u16, TypeRef, Span)>, pub span: Span }
  pub enum TypeRef { Named(String), List(Box<TypeRef>) }   // u8..i128, f32, f64, bool, string, bytes = Named
  pub enum Literal { Int(i128), Float(f64), Bool(bool), Ident(String) }
  pub struct ParseError { pub span: Span, pub msg: String }
  pub fn parse(src: &str) -> Result<Schema, ParseError>;
  ```
  Grammar: the one in §6 of the original design + `@n: reserved [T];` + `#[name]` attributes before a field + `//` and `/* */` comments. The 5 current `.zidl` files (`bank`, `game`, `iot`, `stream`, `conformance`) parse unchanged, except `conformance.zidl`, which becomes `@21: reserved u32;` (edit the file in this task).

- [ ] **Step 1: Failing tests**:
  - `parses_all_repo_schemas`: the 5 files parse. `bank.zidl` gives 2 tables and 1 enum. In `conformance.zidl`, field 21 of `Root` has `name: None, ty: Some(Named("u32"))`.
  - `error_has_line_col`: `"namespace a;\ntable T { x @0 u8; }"` ⇒ a `ParseError` with `line 2`, `col` pointing at `u8` and a message containing `':'`.
  - `attrs_and_defaults`: `#[allow_breaking_readers] @3: reserved;` and `c @1: Color = BRL;` parse with the right fields.
  - proptest `parse_never_panics(a String of 0..400 chars, the grammar's alphabet + garbage)`.
- [ ] **Step 2:** `cargo test -p zidl-compiler --test parse` → fails.
- [ ] **Step 3:** Implement (hand-written lexer + recursive descent, no deps).
- [ ] **Step 4:** Passes.

### Task 3: Semantic checking and layout (IR)

**Files:**
- Create: `crates/zidl-compiler/src/{ir.rs,check.rs}`
- Test: `crates/zidl-compiler/tests/check.rs`

**Interfaces:**
- Consumes: `ast::Schema`, `zidl_layout::{FieldKind, compute_layout, TableLayout}`.
- Produces:
  ```rust
  pub enum Ty { Scalar(Prim), Bool, Enum(EnumId), Struct(StructId), Str, Bytes,
                List(Box<Ty>), Table(TableId), Union(UnionId) }
  pub enum Prim { U8, U16, U32, U64, U128, I8, I16, I32, I64, I128, F32, F64 }
  pub enum Presence { Required, Optional, Default(Value), Reserved { allow_breaking: bool } }
  pub struct Field { pub id: u16, pub name: String /* reserved: "_reserved_<id>" */, pub ty: Ty, pub presence: Presence, pub span: Span }
  pub struct Table { pub name: String, pub fields: Vec<Field>, pub layout: TableLayout, pub all_scalar: bool, pub ptr_slots: Vec<u16> }
  pub struct Ir { pub namespace: String, pub enums: Vec<Enum>, pub structs: Vec<Struct>, pub tables: Vec<Table>, pub unions: Vec<Union> }
  pub fn check(s: &ast::Schema, reserved_types: &dyn Fn(&str, u16) -> Option<Ty>) -> Result<Ir, Vec<Diag>>;
  pub struct Diag { pub span: Span, pub msg: String }
  ```
  - `reserved_types` comes from the lock (Task 4). No lock and no type in the schema ⇒ a `Diag`.
  - `ptr_slots`: the slot of each Ptr; for a union, **`slot + 2`**.
  - Rules:
    - contiguous IDs `0..n`;
    - unique names;
    - resolved types;
    - recursion allowed only via `Table` (a pointer). A struct cannot contain itself; a cycle of structs ⇒ a `Diag`;
    - enum base ∈ {u8, u16, u32} and the values fit the base;
    - unions only of tables, tags `> 0` and unique;
    - no `[[T]]` and no `[union]`;
    - defaults only on scalars/enums/bools and of the right type;
    - a `bool` in a struct takes 1 byte.

- [ ] **Step 1: Failing tests**:
  - `layouts_match_hand_written`: for `bank`, `game`, `iot` and `stream`, `fixed_len`, slots, bits and `ptr_slots` of every table == the consts of `zidl_schemas::<mod>::<table>` (a dev-dep on `zidl-schemas`).
  - `layout_matches_conformance_model`: `Root`, `RootV2` and `Leaf` == `zidl_conformance::interp::layout(model…)` (dev-dep).
  - `union_ptr_slot_is_off_plus_2`: `Root.choice` at slot 102 ⇒ `ptr_slots` contains 104.
  - One `Diag` per violated rule (one test per rule, with the expected text in `msg`): an ID with a gap, a recursive struct, enum base `u64`, a union with a non-table variant, `[[u8]]`, a default on a string, a reserved field without a type and without a lock.
- [ ] **Steps 2–4:** RED → implement → GREEN (`cargo test -p zidl-compiler --test check`).

### Task 4: Lockfile

**Files:**
- Create: `crates/zidl-compiler/src/lock.rs`
- Test: `crates/zidl-compiler/tests/lock.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Serialize, Deserialize, PartialEq, Debug)]
  pub struct Lock { pub format: u32 /* = 1 */, pub namespace: String,
                    pub tables: Vec<LockTable>, pub enums: Vec<LockEnum>, pub structs: Vec<LockStruct>, pub unions: Vec<LockUnion> }
  pub struct LockTable { pub name: String, pub schema_id: String /* "0x%016x" */, pub fixed_len: u16, pub fields: Vec<LockField> }
  pub struct LockField { pub id: u16, pub name: String, pub ty: String /* canonical form, e.g. "u32", "[Leaf]", "Color" */,
                         pub size: u16, pub slot: Option<u16>, pub bit: Option<(u16, u8)>,
                         pub presence: String /* "required" | "optional" | "default:<lit>" | "reserved" */ }
  pub fn lock_from_ir(ir: &Ir, prev: Option<&Lock>) -> Lock;   // schema_id: from prev if it exists, otherwise FNV-1a of "ns.Table"
  pub fn read_lock(path: &Path) -> Result<Option<Lock>, LockError>;   // None = the file does not exist
  pub fn write_lock(path: &Path, l: &Lock) -> io::Result<()>;          // deterministic TOML (order = schema order)
  pub fn reserved_types(l: &Lock) -> impl Fn(&str, u16) -> Option<Ty>;
  ```
  `LockError` gives the file path and the cause (invalid TOML, `format` ≠ 1, a required field missing).

- [ ] **Step 1: Failing tests**:
  - `roundtrip`: `lock_from_ir(conformance)` → `write_lock` → `read_lock` == the original. The TOML is stable (two writes give equal bytes).
  - `schema_id_from_fnv_then_sticky`: without prev, `Root.schema_id == format!("{:#018x}", zidl::envelope::schema_id("conformance.v1.Root"))`. With a prev holding an arbitrary `schema_id`, the prev's value is kept.
  - `reserved_type_from_lock`: `conformance.zidl` with `@21: reserved;` (no type) + lock ⇒ `check` ok and field 21 is `u32`.
  - `lock_rejects_malformed`: broken TOML, `format = 2` and missing `fields` each give a `LockError` with the path and no panic.
- [ ] **Steps 2–4:** RED → implement → GREEN.

### Task 5: Compatibility checking (spec §9)

**Files:**
- Create: `crates/zidl-compiler/src/compat.rs`, `crates/zidl-compiler/tests/compat.rs`, `crates/zidl-compiler/tests/compat/<case>/{old.zidl,new.zidl,expect.txt}`

**Interfaces:**
- Produces:
  ```rust
  pub enum Finding { Error { rule: &'static str, table: String, field: Option<String>, span: Span, msg: String },
                     Warning { rule: &'static str, table: String, field: Option<String>, span: Span, msg: String, confirmed: bool } }
  pub fn compat(old: &Lock, new: &Ir) -> Vec<Finding>;   // an unconfirmed Warning counts as an error for the caller
  ```
  Rule identifiers (used in `expect.txt` and in messages):
  - errors: `type-changed`, `id-reused`, `ids-reordered`, `field-removed`, `enum-base-changed`, `struct-changed`, `new-required`, `optional-to-required`, `table-removed`;
  - warning: `required-reserved`;
  - allowed (no finding): `new-optional`, `new-default`, `reserved-optional`, `required-to-optional`, `new-enum-variant`, `new-union-variant`, `field-renamed`.

- [ ] **Step 1: Corpus + failing test:** one directory per rule (`expect.txt` = `ok` or a line `error <rule> <Table>.<field>` / `warning <rule> <Table>.<field> confirmed|unconfirmed`). `compat_corpus` compiles `old.zidl` → lock (`lock_from_ir`), checks `new.zidl` against it and compares with `expect.txt`. Minimum coverage: the 10 error/warning rules above (the warning with and without `#[allow_breaking_readers]`) + the 7 allowed ones + the real `bank` evolution v1 → v2 (ok) → v3 (`warning required-reserved Transaction.to_shard`, which becomes `confirmed` with the attribute).
- [ ] **Steps 2–4:** RED → implement → GREEN.

### Task 6: Codegen

**Files:**
- Create: `crates/zidl-compiler/src/codegen/{mod,read,write,view}.rs`, `crates/zidl-compiler/tests/codegen.rs`, `crates/zidl-compiler/tests/snapshots/*.rs`, `crates/zidl-conformance/build.rs`, `crates/zidl-conformance/schemas/conformance.zidl.lock`, `crates/zidl-conformance/tests/generated.rs`

**Interfaces:**
- Consumes: `Ir`, `Lock` (for `SCHEMA_ID`), `zidl::rt` (Task 1).
- Produces: `pub fn generate(ir: &Ir, lock: &Lock) -> String` (Rust formatted by `prettyplease`; header `// @generated by zidlc from <file> — do not edit`).

  The code emitted per table is **the Task 1 pattern**:
  - layout consts (`mod <table>` with `FIXED_LEN`, offsets, `*_BIT`, `PTR_SLOTS`);
  - `REQ`/`MASK`, `FromTable`;
  - reader accessors (required ⇒ `T`, optional ⇒ `Option<T>`, default ⇒ `T`, enum ⇒ `from_raw`, string/list/table ⇒ `Result`);
  - an `#[inline]` `verify` with strings validated by `str()` and the header charged by the caller;
  - `read`/`verified`/`from_bytes`;
  - the `XVerified` view (explicit accessors + `reader()`);
  - the `XW` writer (`FixedW` if `all_scalar`, otherwise `TableW`; lists `x(n)` + `x_streaming()`; scalars `x(&[T])` + `x_streaming()`; `[string]` via `set_strs`; `[bytes]` via `set_bytes_list`; a nested table `x() -> YW`; a union `x_<variant>() -> YW`);
  - `XE: Elem`; `XW::to_bytes`/`seal`.

  An enum generates `enum X { …, Unknown(base) }` + `impl Le`. A struct generates `struct X` + `impl Le`. A union generates `enum U<'a> { V(T<'a>), Unknown(u16) }` + `enum UVerified<'a>`.

- [ ] **Step 1: Failing tests**:
  - `snapshots`: `generate` of the 5 schemas == `tests/snapshots/<schema>.rs`. `ZIDL_BLESS=1` rewrites them; on the first run the snapshots are created and reviewed by hand in the report.
  - `generated_equals_hand_written_bytes` (in `zidl-compiler/tests/codegen.rs`, with the generated modules included via `include!` from a test `OUT_DIR`, or in a test crate): for bank, game, iot and stream, the same writer calls in the generated and the hand-written code (Task 1) give equal bytes, and reading gives equal values.
  - `conformance_vectors_generated` (in `zidl-conformance/tests/generated.rs`): for **each** `.bin` in `testvectors/v1/`, the generated `Root::from_bytes`/`verified` (or `RootV2`, according to the JSON's `schema`) gives the same result (Ok + an equal value via a test function `to_value(&RootVerified) -> interp::Value`, or the same error variant name). For the valid, non-special vectors, the generated writer called in ID order reproduces the `.bin` byte for byte.
  - `mutation_gate_generated`: Plan 1's mutation test run over the generated `Root` of `conformance` (every view, including `VStrList`, the union and `child.next`) with 4096 cases and an `Ok` count > 0.
  - a `compile_fail` doctest in the generated code: `RootVerified` cannot be built outside its module.
- [ ] **Steps 2–4:** RED → implement → GREEN. `cargo build --workspace --all-targets` with no warnings, including in the generated code.

### Task 7: The `zidlc` CLI and the `build.rs` API

**Files:**
- Create: `crates/zidl-compiler/src/build.rs`, `crates/zidl-compiler/src/main.rs`, `docs/zidl-language.md`
- Test: `crates/zidl-compiler/tests/cli.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Build { /* private */ }
  impl Build {
      pub fn new() -> Self;
      pub fn schema(self, path: impl AsRef<Path>) -> Self;   // lock = "<path>.lock"
      pub fn out_dir(self, dir: impl AsRef<Path>) -> Self;  // default: env OUT_DIR
      pub fn compile(self) -> Result<(), CompileError>;     // writes <out>/<file without .zidl>.rs; NEVER writes the lock
  }
  ```
  - `compile` emits `cargo:rerun-if-changed` for the schema and the lock. It fails if the lock does not exist (message: `run: zidlc update <schema>`), if it would need to change, or if `compat` has an error or an unconfirmed warning.
  - CLI:
    - `zidlc check <schema>…`: exit 1 on error; prints the findings;
    - `zidlc update <schema>…`: applies compat and writes the lock if there is no error;
    - `zidlc gen <schema> -o <dir>`.
  - Error output in the format `file:line:column: message`.
  - `docs/zidl-language.md` (in Portuguese, later translated to English): grammar, types, presence, reserved, attributes, compatibility rules (a link to spec §9), the `update` × `build.rs` workflow.

- [ ] **Step 1: Failing tests** (`tests/cli.rs`, in a temporary directory):
  - `build_missing_lock_errors`: the message contains `zidlc update` and no file is created next to the schema.
  - `update_then_build_ok`.
  - `incompatible_change_fails_check_and_build`: changing `u32`→`u64` ⇒ `zidlc check` exits 1 with `type-changed` and `compile()` fails.
  - `update_refuses_unconfirmed_warning` / `update_accepts_confirmed_warning`.
  - `build_does_not_touch_lock`: the lock's mtime and bytes stay the same after `compile()`.
- [ ] **Steps 2–4:** RED → implement → GREEN.

### Task 8: `zidl-schemas` switches to generated code

**Files:**
- Create: `crates/zidl-schemas/build.rs`, `crates/zidl-schemas/schemas/{bank,game,iot,stream}.zidl.lock`, `crates/zidl-schemas/schemas/evo/{bank_v2,bank_v3}.zidl` (+ `.lock`)
- Modify: `crates/zidl-schemas/src/lib.rs` (only `include!` of the generated files), `crates/zidl-schemas/Cargo.toml` (`[build-dependencies] zidl-compiler`)
- Delete: `crates/zidl-schemas/src/{bank,game,iot,stream}.rs`, `crates/zidl-schemas/src/evo/*.rs`
- Test: all of `zidl-schemas` and `zidl-bench` (the assertions do not change; only call sites that the Task 1 pattern already changed)

- [ ] **Step 1:** Generate the locks with `zidlc update`.
  - For `evo`: the lock of `bank_v2` starts from the lock of `bank` v1; `zidlc update` on `bank_v2.zidl` with `--from schemas/bank.zidl.lock` (add this flag to the CLI if Task 7 did not) proves compat v1 → v2 and v2 → v3 (with `#[allow_breaking_readers]` on `to_shard`).
- [ ] **Step 2:** `build.rs` compiles the 6 schemas; `lib.rs` does `pub mod bank { include!(concat!(env!("OUT_DIR"), "/bank.rs")); }` etc. Delete the hand-written files.
- [ ] **Step 3:** `cargo test --workspace` passes without changing assertions (the `layout_consts`, `roundtrip`, `evolution`, `required_fast` and `verified` tests and the mutation gate keep holding over the generated code).
- [ ] **Step 4:** An interleaved A/B benchmark (medians of 3) of the generated code against the pre-plan state. Take an `rsync` snapshot of the tree before starting Task 1. Measure `write`, `read_all`, `read_one`, `read_all_verified` and `envelope` within ±5%. Record it in `benchmarks/2026-11-13-codegen.md` (same format as the earlier rounds).
- [ ] **Step 5:** `cargo clippy --workspace --all-targets` with no new warnings, including in the generated code (`OUT_DIR`).
