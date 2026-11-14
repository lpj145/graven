# Benchmarks

The reference implementation compared with FlatBuffers and Cap'n Proto on the same data.
These numbers come from one machine and one run. Treat them as a measurement of this
implementation, not of the format in general, and run them on your own hardware before
relying on them. How we got to these numbers is in [History](history/index.md).

## Setup

| | |
|---|---|
| Machine | Intel Core i9-14900HX (32 threads), Linux 7.3 (CachyOS) |
| Compiler | rustc 1.94.0, `--release` |
| Libraries | flatbuffers 25.12.19 (flatc 25.12.19), capnp 0.27.3 (capnp 1.5.0), criterion 0.8.2 |
| Run | `--warm-up-time 1 --measurement-time 3`, median of the samples; run-to-run noise is about ±5% |
| Date | 2026-11-13 |

To reproduce:

```sh
cargo bench -p graven-bench --bench codec
cargo run --release -p graven-bench --bin sizes
cargo run --release -p graven-bench --bin allocs
```

### Scenarios

The same four messages are implemented in all three formats, and a test
(`crates/graven-bench/tests/equivalence.rs`) checks that every implementation reads the same
values for 20 random seeds per scenario.

| scenario | message |
|---|---|
| `bank/N` | a transaction with N entries, each with an optional memo string |
| `frame/N` | a game frame with N entities (position, velocity, health, flag) |
| `telemetry` | one IoT reading: five scalars, one optional |
| `chunk/N` | a stream chunk carrying N bytes of payload |

### Keeping the comparison fair

- **Reused contexts.** Graven reuses its `Builder`, FlatBuffers its `FlatBufferBuilder`
  (`reset()`), Cap'n Proto its scratch allocator.
- **Same output contract.** The `graven` write column hands out a borrowed slice, as
  FlatBuffers' `finished_data()` and Cap'n Proto's output `Vec` do. `graven_owned` returns an
  owned, reference-counted buffer per message; `graven_batched` packs messages into one
  buffer for sending.
- **Lists.** Cap'n Proto must know a list's length before writing it. `graven` in `bank`
  writes the list as a stream, without knowing its length; `graven_known` passes the
  length, as Cap'n Proto does.
- **Strings.** Graven validates UTF-8 on every read in its default mode. FlatBuffers'
  `root_unchecked` and Cap'n Proto do not. `graven_trusted` reads a message that was already
  verified, without validating again.
- **Verification.** FlatBuffers' verifier and Graven's `verify` walk the whole message
  before reading. Cap'n Proto checks lazily, while reading, and does not validate UTF-8, so
  its verified column is not the same amount of work.

## Size

Bytes per message. Smaller is better.

| scenario | Graven | FlatBuffers | Cap'n Proto | Cap'n Proto packed |
|---|---:|---:|---:|---:|
| bank/1 | 118 | 168 | 144 | **106** |
| bank/10 | 418 | 576 | 480 | **352** |
| bank/100 | 3607 | 4536 | 4080 | **2977** |
| frame/10 | 331 | 488 | 360 | **322** |
| frame/500 | **15521** | 20104 | 16040 | 15827 |
| telemetry, with humidity | **24** | 48 | 40 | 28 |
| telemetry, without humidity | 24 | 48 | 40 | **23** |
| chunk/64 | **87** | 112 | 104 | 90 |
| chunk/65536 | **65559** | 65584 | 65576 | 65626 |

Graven is smaller than FlatBuffers and unpacked Cap'n Proto everywhere. Packed Cap'n Proto
is smaller for messages with many small integers, and pays CPU time to pack and unpack.

## Allocations

Allocations per written message, after warm-up.

| scenario | Graven (stream) | Graven (known) | FlatBuffers | Cap'n Proto |
|---|---:|---:|---:|---:|
| bank/100 | 0 | 0 | 0 | 1 |
| frame/500 | 0 | 0 | 0 | 1 |
| telemetry | 0 | 0 | 0 | 1 |
| chunk/65536 | 0 | 0 | 0 | 1 |

## Time

Nanoseconds per message, median. Lower is better; bold is the fastest.

### Writing

| scenario | Graven (stream) | Graven (known) | FlatBuffers | Cap'n Proto |
|---|---:|---:|---:|---:|
| bank/1 | 31.4 | **22.9** | 56.3 | 57.0 |
| bank/10 | 85.6 | **56.6** | 343.1 | 82.1 |
| bank/100 | 632.4 | **378.2** | 3130.9 | 414.7 |
| frame/10 | **26.3** | | 333.8 | 61.9 |
| frame/500 | **777.4** | | 17751.0 | 876.4 |
| telemetry | **6.0** | | 21.5 | 33.8 |
| chunk/64 | **9.7** | | 20.0 | 44.0 |
| chunk/65536 | **966.4** | | 1731.6 | 2819.6 |

Only `bank` has a list whose length can be unknown; in the other rows the first column is
the only Graven measurement.

Owned and batched messages:

| scenario | borrowed | batched | owned |
|---|---:|---:|---:|
| telemetry | 6.0 | 7.1 | 22.6 |
| chunk/64 | 9.7 | 11.4 | 27.5 |
| frame/500 | 777.4 | 951.9 | 926.0 |

An owned message pays an atomic reference count; batching amortizes it over many messages.
A large owned message is also slower than a borrowed one because it needs fresh memory
instead of rewriting the same cache-hot buffer.

### Reading every field

| scenario | Graven | Graven (trusted) | FlatBuffers (unchecked) | Cap'n Proto |
|---|---:|---:|---:|---:|
| bank/1 | 18.8 | **7.5** | 22.6 | 67.7 |
| bank/10 | 57.9 | **23.6** | 79.3 | 111.9 |
| bank/100 | 476.0 | **194.6** | 685.4 | 633.4 |
| frame/10 | **58.9** | | 122.6 | 96.1 |
| frame/500 | 2414.5 | | 6433.0 | **2296.1** |
| telemetry | **1.4** | | 10.6 | 34.5 |
| chunk/64 | **2.4** | | 7.1 | 45.9 |
| chunk/65536 | **2.4** | | 7.0 | 45.9 |

### Reading one field

| scenario | Graven | FlatBuffers | Cap'n Proto |
|---|---:|---:|---:|
| bank/1, /10, /100 | **1.3–1.4** | 2.3 | 32.3 |
| frame/10, /500 | **2.5–2.6** | 4.1–4.2 | 41.7–42.4 |
| telemetry | **1.3** | 2.2 | 32.7 |
| chunk/64, /65536 | **1.2** | 2.3 | 32.6 |

### Verifying and reading every field

| scenario | Graven | FlatBuffers | Cap'n Proto (lazy) |
|---|---:|---:|---:|
| bank/1 | **28.7** | 114.6 | 66.5 |
| bank/10 | **86.5** | 466.4 | 111.8 |
| bank/100 | 712.4 | 4084.4 | **620.3** |
| frame/10 | **67.5** | 557.6 | 94.9 |
| frame/500 | 2705.1 | 27858.0 | **2250.2** |
| telemetry | **3.2** | 51.6 | 33.9 |
| chunk/64 | **4.7** | 39.0 | 45.4 |
| chunk/65536 | **4.6** | 38.7 | 45.3 |

## The envelope

The optional [envelope](spec/graven-format-v1.md#8-envelope) adds 24 bytes and a CRC32C over
the whole message.

| scenario | write, owned | write, enveloped | verify + read one field | `from_bytes` + read one field |
|---|---:|---:|---:|---:|
| telemetry | 21.8 | 32.0 | 2.4 | 12.0 |
| chunk/64 | 26.4 | 39.2 | 2.6 | 12.6 |
| bank/10 | 70.3 | 108.8 | 61.1 | 87.9 |
| chunk/65536 | 1567.2 | 2549.4 | 2.6 | 1622.1 |

Verification does not touch payload bytes, so a 64 KiB chunk verifies in under 3 ns; the
CRC reads all of them. For small messages the envelope costs about 10 ns on each side.

CRC32C throughput, hardware instruction against the portable software version (used
without `std`):

| size | hardware | software |
|---|---:|---:|
| 64 B | 6.0 ns | 11.7 ns |
| 1 KiB | 29.7 ns | 347.7 ns |
| 64 KiB | 1614.1 ns (≈ 40 GB/s) | 23463.0 ns (≈ 2.8 GB/s) |

## Where Graven loses

- **Size against packed Cap'n Proto** for messages made of many small integers.
- **Reading every field of `frame/500`** against Cap'n Proto: 2414 ns against 2296 ns, about 5%,
  within the noise of one run.
- **Verified reading of large messages** (`bank/100`, `frame/500`) against Cap'n Proto,
  whose check is lazy and skips UTF-8; against FlatBuffers' full verifier Graven is 6–10×
  faster.
- **Writing a list of unknown length** costs 1.4–1.7× a list whose length is known.
