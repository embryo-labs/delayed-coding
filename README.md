# Delayed Coding

A standalone Rust entropy coder extracted from
[Blitzcrank](https://github.com/embryo-labs/Blitzcrank), with a C interface for
embedding in C/C++ systems.

**Status: experimental, pre-release.** The Rust core and C ABI work, and scalar
24-bit payloads are checked against the original Blitzcrank implementation.
The crate is not yet published to crates.io. API and new format conventions are
not frozen. Full Blitzcrank migration and release acceptance are still pending.

Delayed Coding encodes a sequence of symbols using caller-provided probability
models. It decodes forward and lets the caller select a different model for every
symbol. This is useful for structured records and conditional probability models.
It is an entropy coding building block, not a replacement for an LZ compressor or
a self-describing file format.

## Try it

Rust 1.88 or newer; no third-party dependencies for the core or C ABI.

```sh
git clone https://github.com/YimingQiao/delayed-coding.git
cd delayed-coding
cargo run --release --example roundtrip
cargo run --release --example conditional
cargo run --release --example indexed_records
cargo test --workspace
```

```rust
use delayed_coding::{Model, encode, decode_into};

let model = Model::from_counts(&[10, 5, 1])?;
let symbols = [0, 0, 1, 0, 2, 0];
let payload = encode::<24>(&model, &symbols)?;
let mut restored = [0; 6];
decode_into::<24>(&model, &payload, &mut restored)?;
assert_eq!(restored, symbols);
# Ok::<(), delayed_coding::Error>(())
```

For a downstream Cargo project, pin a Git revision until a crate release exists:

```toml
[dependencies]
# Replace REVISION with the full commit you have tested.
delayed-coding = { git = "https://github.com/YimingQiao/delayed-coding", rev = "REVISION" }
```

## Interfaces

- `Model::new`: normalized frequencies summing to 65,536, up to 65,536 symbol IDs.
- `Model::from_counts`: deterministic normalization preserving observed symbols.
- `encode_into`: caller-owned output and a reusable `Workspace`; returns the
  occupied range because the encoder writes backwards.
- `encode_events_into` / `Decoder::read`: explicit per-symbol model selection.
- `Branch` / `encode_branches_into`: import selected disjoint-interval branches
  without changing a semantic model's mapping. `Decoder::read_uniform` and
  `read_raw` handle numerical partitions/raw words without identity tables.
- `encode_interleaved_into::<24, 4>` / `decode_interleaved_into::<24, 4>`: four
  round-robin states sharing one payload. Rust supports 1/2/4/8 states.
- `Model::with_tables`: optional 128 KiB encode and/or 512 KiB decode tables.
  Compact alias tables are the default; table choices do not change payloads.
- `Layout::<24>::push_frequency`: exact physical offsets and payload length from
  realized frequencies alone, without running the information-state recurrence.
  `encode` now requests exactly the payload size, avoiding its old worst-case
  payload allocation and final move (scheduling workspace is still required).
- `decode_lookahead_into::<24>` / C `dc_decode_lookahead`: opt-in, single-state
  fixed-model decoding with one physical word of lookahead. Same bytes and checks;
  no additional table/allocation. Not available for conditional model selection.
- `decode_grouped4_into::<24>` / C `dc_decode_grouped4`: opt-in four-state
  capacity-planned groups. Plan physical offsets before the group's lookups;
  preserve the ordinary four-state bytes and error behavior. Interleaved physical
  lookahead is also available, but can be slower. Neither replaces the default.
- `decode_branchless4_into::<24>` / C `dc_decode_branchless4`: opt-in four-state
  source selection with masks and bounded eight-byte windows. No input padding;
  scalar reads handle the tail. Particularly useful to test on nonuniform blocks.

The optional Cargo feature `flat-alias` experiments with branch-free alias
addressing. It helps some fixed-model blocks but regresses measured conditional
model switching, so it is disabled by default. See the benchmark report before
enabling it; it does not change payload bytes.

The optional `speculative-encode` feature removes the physical/virtual store
branch in the model/event encoder for 4/8 states. Single-state and selected-branch
encoding retain their existing embedding loop. Writes remain inside the final
payload range, even with exact capacity. Predictable distributions can regress,
so the feature is off by default. See [speed experiments](benchmarks/SPEED_KERNELS.md).

Try the explicit fixed-model speed path with reusable buffers and direct tables:

```sh
cargo run --release --features speculative-encode --example fast_block
# Or add: -- /path/to/byte/file
```

This example uses four states, 128 KiB of extra encoding tables and 512 KiB of
extra decoding tables. It is not an automatic best choice for short records,
many conditional models or uniform data.

The core forbids unsafe code. Models are immutable and shareable; each encoder
owns its workspace and each decoder owns its state. The encoder still needs a
forward scheduling pass and a backward embedding pass; this is not an online
forward encoder. `24` is the delay threshold, not the probability precision.

An optional **separate-format derivative**, `LogModel`, replaces the exact
multiplicative capacity schedule with a conservative integer-log schedule.
It retains the same input probabilities but trades a small rate cost for speed;
its bytes are not compatible with ordinary DC or Blitzcrank. It supports fixed
models, 1/2/4/8 lanes, reusable Workspace and bounded Rust/C APIs:

```sh
cargo run --release --features log-schedule --example log_speed -- /path/to/file
```

See the [schedule and invertibility argument](docs/LOG_SCHEDULE.md) and
[measured comparison and limits](benchmarks/LOG_SCHEDULE.md). This experiment is
off by default and is not selected by the Blitzcrank bridge.

For C/C++:

```sh
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build -j
ctest --test-dir build --output-on-failure
```

See [the C example](examples/c_roundtrip.c) and [public header](include/delayed_coding.h).
Downstream CMake projects can use `add_subdirectory` and link
`DelayedCoding::delayed_coding`. The C ABI exposes fixed-model blocks and mixed
selected-branch encoding, delay 16/24/32, and one/four states. Rust additionally
exposes stateful conditional decoding. The opt-in Blitzcrank encoder adapter uses
one C call per block, including numerical/raw branches; its decoder is still C++.
See [integration status and limits](docs/BLITZCRANK_INTEGRATION.md).

## Performance and validation

[Benchmark instructions](benchmarks/README.md) distinguish unmodified pinned
upstream ryg_rans **programs**, mechanically extracted two-state/eight-state
loops, and explicitly labelled derived adapters. Unmodified headers alone do
not guarantee the performance of the upstream programs.
Both sides consume/produce the same u32 symbol representation and use identical
16-bit normalized frequencies. An opt-in SSE4.1 comparison uses a separate,
probability-matched 12-bit suite. Reports include payload size alongside speed.

There is no general claim of outperforming rANS. Current measurements are kernel
experiments on one x86-64 machine, with prebuilt models and reused buffers. They
exclude model serialization, indexing and whole-file overhead. See the
[initial findings](benchmarks/RESULTS.md) and raw CSVs.

**Baseline correction (2026-09-13):** the earlier book1 speed checkpoint compared
DC against underoptimized adapter loops. With upstream-style four-state scheduling,
rANS64 decodes in 2.69 ns/symbol versus DC4's 3.21, at identical probabilities and
u32 output. The extracted upstream eight-state SSE4.1 loop reaches 1.77 in the
separate matched 12-bit suite (not an equal-state-count comparison). The earlier
DC lead does not hold. See [source calibration and raw results](benchmarks/UPSTREAM_CALIBRATION.md),
including the remaining scalar-wrapper timing discrepancy.

The [layout/lookahead experiment](benchmarks/LAYOUT_LOOKAHEAD.md) identifies two
bounded advantages on this machine: faster single-state decoding on a nonuniform
high-entropy synthetic input, and smaller independently coded short text records
than the tested byte/64-bit rANS baselines. Four-state rANS still wins throughput;
DC encoding remains slower. The record comparison includes equal offset indexes,
but excludes shared model/framing bytes. This is not a whole-database result.
Packed 512 KiB rANS decode-table adapters are explicitly labelled in the harness;
they supplement, rather than replace, the unmodified-header baselines.

The [four-state and real integration report](benchmarks/BLITZCRANK_FOUR_STATE.md)
records a faster experimental DC group kernel, continued throughput losses to
four-state rANS, and bit-identical Census encoding through the Rust bridge.
Small-record four-state space wins are not yet a storage/latency application win.

Tests cover exhaustive short binary strings, every 16-bit code point, random and
conditional models, reciprocal-division boundaries, interleaving against independent
scalar streams, truncation, and malformed payloads. To compare against the original
research implementation:

```sh
cmake -S . -B build-legacy -DCMAKE_BUILD_TYPE=Release \
  -DDELAYED_CODING_BLITZCRANK_DIR=/path/to/original/Blitzcrank
cmake --build build-legacy -j
ctest --test-dir build-legacy --output-on-failure
```

The reference checkout must use `kDelayedCoding=24`; the tested base is
`0ed9c97908c51440b30a2eef3c1b90325dd2c87c`.

## Format and integration

Raw payloads contain big-endian 16-bit words. Store the model, delay, lane count
and original symbol count externally. Incorrect metadata can produce incorrect
symbols without an error. Final-state checks do not replace a checksum, and the
caller must bound requested output sizes. See [format policy](docs/FORMAT.md).

Random record access belongs to the container: independent blocks, reusable models
and an offset index. Blitzcrank remains the structured-data application; this
repository owns the entropy core. The [execution plan](PLAN.md) tracks migration,
performance work and release gates.

## Origin and contributing

Based on Yiming Qiao, Yihan Gao and Huanchen Zhang's VLDB 2024 paper,
[Blitzcrank: Fast Semantic Compression for In-memory Online Transaction Processing](https://www.vldb.org/pvldb/vol17/p2528-zhang.pdf).
The [original implementation](https://github.com/embryo-labs/Blitzcrank) remains
the research reference. MIT licensed; original notices are retained.

Useful contributions include reproducible workloads, C/C++ integration feedback,
ARM measurements, malformed-input regressions and profiling. Please report the
commit, CPU, compiler, delay, table mode, lane count, block size and both payload
size and speed. See [CONTRIBUTING.md](CONTRIBUTING.md).
