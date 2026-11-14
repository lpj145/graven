# How we got here

Graven started from FlatBuffers and Cap'n Proto. Both showed that a message can be read in
place, without parsing, and both made choices we wanted to revisit: FlatBuffers builds
messages back to front, and Cap'n Proto pads every value to 8 bytes. The project began as an
experiment called **zidl**. The question was simple: could a format be written front to back,
straight into a pooled buffer, smaller than both, and still win on speed? Every answer had to
come from a benchmark.

The documents below are the record of that exploration: the designs, the implementation
plans and the benchmark reports, in the order they were written. They are **not normative**.
Several decisions in them were changed later (the name, the magic bytes, versions in
namespaces). The [format specification](../spec/graven-format-v1.md) is the text that
counts.

## Timeline

### 1. The first design (2026-11-12)

[Original design](design/2026-11-12-zerocopy-idl-design.md): the goals, in priority order,
and the first version of the format. Tables with a fixed part and presence bits, relative
`i32` pointers, no alignment padding, and lists that can be written as a stream.

### 2. A runtime, and a fair fight

- [Plan: runtime and benchmarks](plans/2026-11-12-zidl-runtime-and-benchmarks.md): the
  runtime, hand-written schema code, and the same four scenarios (bank, game, IoT, stream)
  implemented in FlatBuffers and Cap'n Proto.
- [Baseline](benchmarks/2026-11-12-baseline.md): zidl was smaller than both and allocated
  nothing per message, but it lost on writing and on part of reading. The report lists
  each loss with a hypothesis for its cause.
- [Round 2](benchmarks/2026-11-12-round2.md): a methodology bias was fixed and each
  hypothesis was tested. Builder reuse, a required-field mask and scalar-only writers
  turned most of the losses into wins. The cases still lost are written down too.

### 3. Hardening into v1

- [Hardening design](design/2026-11-12-zidl-v1-hardening-design.md): what it takes to freeze
  a format others can implement. An envelope with a CRC32C and a schema id, verified views
  with no public `unsafe`, a normative spec, frozen conformance vectors, a compiler and a
  lockfile.
- [Plan 1: the format](plans/2026-11-12-zidl-v1-format.md): envelope, CRC, verified views,
  the dynamic reference interpreter and the vectors.
- [Envelope report](benchmarks/2026-11-12-envelope.md): what the envelope costs, and how a
  regression of about 10 ns was traced to the size of the error type.

### 4. A compiler

- [Plan 2: the compiler](plans/2026-11-13-zidl-compiler.md): parser, checker, lockfile,
  compatibility rules and code generation, which had to match the hand-written code.
- [Codegen report](benchmarks/2026-11-13-codegen.md): the generated code compiles to the same
  machine instructions as the hand-written code in 111 of 116 functions.

### 5. Robustness

- [Plan 3: tests and robustness](plans/2026-11-14-zidl-robustness.md): more conformance
  vectors, fuzzing (including a target that compares the verified and checked readers), the
  evolution matrix and Miri.

### 6. Graven

The project was renamed Graven, the magic became `GRVN`, and namespaces lost their
versions. Imports between schema files, doc comments, the full 128-bit literal range and the
tools came after that. The [changelog](../changelog.md) and
[proposal 0001](../proposals/0001-imports.md) cover that part.
