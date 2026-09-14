# Delayed Coding

A standalone entropy-coding library for structured data, developed from
[Blitzcrank](https://github.com/embryo-labs/Blitzcrank). Rust API, C/C++ interface,
reusable buffers, forward decoding and explicit per-symbol model selection.

**Status: preview.** The portable core is usable independently; the optional
SIMD block codec has a separate payload contract. This is not a self-describing
file compressor, an online forward encoder, or a production storage engine.
Not yet published to crates.io; pin a Git commit.

## Quick start

Rust 1.88+ for the portable core and C ABI; Rust 1.89+ for AVX-512.

```sh
git clone https://github.com/embryo-labs/delayed-coding.git
cd delayed-coding
cargo test --workspace --locked
cargo run --release --example roundtrip
cargo run --release --example conditional
cargo run --release --example indexed_records
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

For a downstream Cargo project:

```toml
[dependencies]
# Replace REVISION with the full commit you have tested.
delayed-coding = { git = "https://github.com/embryo-labs/delayed-coding", rev = "REVISION" }
```

## How it works

Encoding makes a forward pass to schedule physical versus delayed words, then
embeds information backwards into the resulting payload. Decoding consumes
symbols forwards, using the same sequence of probability models.

The capacity schedule depends on realized frequencies, independently of the
information numerator. `Layout` exposes exact payload lengths and physical
offsets from those frequencies. Conditional models can be selected explicitly
by the caller during decoding; the library does not learn those models itself.

Probabilities sum to 65,536; physical words are 16 bits. The generic delay
threshold is 16–32 bits, distinct from probability precision. See
[the algorithm and invariants](docs/ALGORITHM.md).

## Choose an interface

| Workload | Interface | Default policy |
| --- | --- | --- |
| Fixed-model symbols | `Model`, `encode_into`, `decode_into` | Compact alias tables |
| Heterogeneous records | `Event`, `Decoder::read`, `read_prepared` | Caller-selected model per event |
| Prepared decoding | `DecodeModel`, `decode_prepared_into` | Packed decode-only metadata |
| Existing semantic branches | `Branch`, `encode_branches_into` | Preserve supplied interval mappings |
| Explicit interleaving | `encode_interleaved_into`, `decode_interleaved_into` | 1/2/4/8 states |
| Large SIMD blocks | [`delayed-coding-simd`](simd/README.md) | Opt-in; scalar fallback |
| C/C++ embedding | [C header](include/delayed_coding.h) | One/four states; delay 16/24/32 |

Models are immutable and shareable. Each encoder owns a reusable `Workspace`;
each decoder owns its state. Caller-owned buffers avoid per-call payload
allocation. The core forbids unsafe Rust; pointer handling is isolated in
`ffi/`, and CPU intrinsics in the optional `simd/` companion.

Large direct tables, numerical slot fusion, speculative encoding, reduced
probability precision and the separate logarithmic schedule are explicit
experiments. They are **not** selected automatically: cache pressure, short
records and model preparation can outweigh their benefits.

## AVX-512 blocks

```sh
cargo test -p delayed-coding-simd --features avx512 --release --locked
cargo run -p delayed-coding-simd --example block_bench --features avx512 --release -- /path/to/byte/file
```

The companion provides exact DC16 arithmetic, cumulative intervals, up to
256 symbol IDs and a 64-state throughput path. AVX-512 is detected at runtime;
unsupported CPUs use the byte-identical scalar backend. No caller padding,
global `target-cpu=native`, or mandatory probability quantization is required.

This is a bulk-throughput option, not the default for short random-access
records. Its mapping and lane count must be recorded by the container. The
Blitzcrank Rust preview integrates it through explicit `compress-simd`, while
its ordinary single-record path remains portable.

## Measured DC performance

A September 2026 measurement of the release-candidate DC16/64 AVX-512 module
on **book1**, one Intel Xeon Platinum 8474C thread, reported:

| Probability precision | Encode ns/symbol | Decode ns/symbol | Payload bytes |
| --- | ---: | ---: | ---: |
| 16 bits, no reduced-precision quantization | 0.8590 | 0.9546 | 441,260 |

These are **kernel** results with prebuilt models and reusable u32
symbol buffers, not end-to-end Blitzcrank latency or guaranteed performance.
They exclude model construction, file I/O, container metadata and indexing.
Each of three process runs contains seven timed samples; the table reports the
median of their sample medians. Owned model heap is 537,088 bytes, including
524,288 bytes of direct decode tables. See the
[raw samples](benchmarks/results/2026-09-14-simd-release.csv) and
[measurement provenance](benchmarks/results/2026-09-14-simd-release.json).
The earlier [hardware analysis](benchmarks/AVX512.md) documents a related
prototype and explicitly reduced-precision experiments separately. Measure the
intended workload; these numbers are not a release-wide speed guarantee.

## C/C++

```sh
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build -j
ctest --test-dir build --output-on-failure
```

Link `DelayedCoding::delayed_coding` from a downstream CMake project.
See the [C example](examples/c_roundtrip.c) and
[integration guide](docs/BLITZCRANK_INTEGRATION.md).
The existing C ABI does not expose the SIMD companion.

## Format, validation and limits

Raw payloads contain big-endian 16-bit words. Store the mapping, model,
delay, lane count and symbol count externally. Incorrect metadata can produce
incorrect symbols without an error. Final-state checks are **not a checksum**.
Bound input, model allocation and requested output sizes at your application
boundary; allocation failure follows Rust's allocator policy.

Tests cover all 16-bit code points, mixed models, exact buffers, reciprocal
division boundaries, interleaving, scalar/SIMD agreement, tails and malformed
streams. Historical scalar delay-24 payloads are checked against the original
Blitzcrank implementation. See [format policy](docs/FORMAT.md) and
[release review](docs/RELEASE_REVIEW.md). Tests and review are not a security audit.

## Origin and contributing

Based on Yiming Qiao, Yihan Gao and Huanchen Zhang's VLDB 2024 paper,
[Blitzcrank: Fast Semantic Compression for In-memory Online Transaction Processing](https://www.vldb.org/pvldb/vol17/p2528-zhang.pdf).
MIT licensed; original notices and history are retained.

Contributions are welcome: structured-data integration, reproducible workloads,
portability and malformed-input regressions. Include the commit, CPU, compiler,
model/table configuration, block size, payload bytes, preparation cost and
encode/decode timing. See [CONTRIBUTING.md](CONTRIBUTING.md).
