# Four-state DC and the first real Blitzcrank encoder bridge

Date: 2026-09-13. This is a development checkpoint, not release acceptance or a
general rANS win. Source is the commit containing this report, based on `d95fb26`.
Blitzcrank integration is on `improve-delayed-coding`, based on unmodified
`0ed9c97908c51440b30a2eef3c1b90325dd2c87c`. Upstream ryg_rans remains pinned to
`c9d162d996fd600315af9ae8eb89d832576cb32d`.

## What the four-state experiment actually implements

DC already supports 1/2/4/8 independent round-robin states in Rust. The new
`decode_grouped4_into` plans the four physical-word flags and prefix offsets
from the capacity states **before** doing any of the group's symbol lookups.
It then extracts four words, looks them up and updates the information states.
This is a concrete use of capacity/information separation, not an alias novelty
claim. It retains the ordinary four-state payload and bounded-error contract.
It is scalar instruction-level parallelism, not a new SIMD implementation.

Generalized physical lookahead is another explicit alternative. It often loses
with four states. Neither experiment replaces the default; conditional model
selection is not supported by these fixed-model block kernels.

Environment: Intel Xeon Platinum 8474C, Linux x86-64, CPU 2 affinity, Rust 1.92,
GCC 12.2, CMake Release, generic x86-64, Rust thin LTO/codegen-units=1. Fixed
models, reused buffers, seven median samples, same 16-bit weights/u32 symbols.
Model setup and I/O excluded; state reset, final checks and one C call included.
Rust input reads are bounded; upstream rANS inner input reads are unchecked.

Canonical raw data: [65,536 synthetic symbols](results/2026-09-13-four-lane-inline-65536.csv)
and [book1](results/2026-09-13-four-lane-inline-book1.csv). Decode ns/symbol:

| Input | DC4 compact ordinary | DC4 compact grouped | DC4 direct ordinary | DC4 direct grouped | rANS64 4-state |
| --- | ---: | ---: | ---: | ---: | ---: |
| uniform256 | 2.6481 | 2.9422 | 2.6652 | 2.7616 | 2.1766 |
| ramp256 | 8.4990 | 3.1498 | 3.2329 | 2.9247 | 2.5741 |
| book1 | 6.5332 | 4.4353 | 4.2512 | 4.1090 | 3.6636 |

The group improves nonuniform DC decoding, but loses on uniform data and remains
slower than the listed four-state rANS baseline. Direct DC decoding adds a
512 KiB table; rANS64's ordinary symbol lookup is 64 KiB plus starts/frequencies.
The full CSV also includes compact alias and packed-table rANS controls. DC24
four-state book1 is 435,088 bytes versus rANS64 four-state 435,080. Encoding is
still a major weakness: DC4 direct-encode/direct-decode is 7.4454 ns/symbol on
book1 versus rANS64 four-state 3.1818. No claim of dominating rANS is justified.

All intermediate CSVs are retained under the same date. `lookahead-*` predates
grouping. `grouped-*` and `isolated-*` caught a refactor-induced default decoder
inlining regression; they are diagnostics, **not** the final baseline. The final
implementation keeps inner reads always-inline and isolates block kernels from
the growing C dispatch. Intermediate source snapshots were not separately
committed, so those timings are not independently reproducible ablations.
`flat-*` repeats with the optional flat lookup feature: book1 compact ordinary
4.5180, compact grouped 4.4651 ns/symbol. It stays off by default because earlier
many-model tests regressed. Do not attribute the whole compact gain to grouping
when a different lookup layout can already recover much of it.

Reproduce the canonical kernel suite using the setup in [README](README.md):

```sh
taskset -c 2 build/compare_rans 65536
taskset -c 2 build/compare_rans --file /tmp/ryg_rans/book1
taskset -c 2 build/compare_rans --records /tmp/ryg_rans/book1
```

book1: 768,771 bytes, SHA-256
`9ffa47cd93bccd732f20e0c304203cfbc1b8a91bedac536e2d8f6051003d9951`.

## Four-state short records: size is only one axis

The [record CSV](results/2026-09-13-four-lane-record-sizes-book1.csv) resets each
codec per record, shares one global model and includes an equal u32 offset index.
Every record is decoded/checked; this is a size test, not a latency benchmark.
For 16-byte records (48,049 records, final record partial):

| Codec | Payload bytes | Index bytes | Total bytes |
| --- | ---: | ---: | ---: |
| DC16, 1 state | 559,644 | 192,200 | 751,844 |
| byte-rANS, 1 state | 603,319 | 192,200 | 795,519 |
| DC16, 4 states | 826,982 | 192,200 | 1,019,182 |
| DC24, 4 states | 1,154,142 | 192,200 | 1,346,342 |
| byte-rANS, 4 states | 1,114,694 | 192,200 | 1,306,894 |
| rANS64, 4 states | 1,541,624 | 192,200 | 1,733,824 |

DC16 four-state uses 22.0% fewer total bytes than byte-rANS four-state, but
**single-state byte-rANS is smaller still**. DC24 four-state even loses to
byte-rANS four-state here. Four lanes do not automatically suit short records:
reset/startup costs are paid per lane. Shared model bytes, allocator overhead,
framing/checksums and random-read latency are not in this table. The application
must compare the best size/latency configurations, not enforce four lanes.

## Real Blitzcrank bridge, not a fixed-model substitute

The new selected-branch API preserves disjoint intervals, rare/numerical partitions
and raw-word mappings without reconstructing an alias model. Blitzcrank imports
immutable branch handles and calls Rust once per encoded block. The first bridge
keeps its original single-state delay-24 files and C++ decoder; four-state
integration requires a versioned container convention and is not enabled here.

Validation uses the first 20,000 rows of the repository's real USCensus1990 data,
69 fields (one integer, 68 categorical), not its Git LFS pointer. For block
thresholds 1 and 20,000, legacy/Rust compression produce identical payloads,
enumeration sidecars and indexes; each decoder output exactly matches input.
2,050 shuffled/boundary seeks per backend/threshold verify every field: 8,200
queries total. JSON and time-series targets build, but do not yet have matching
end-to-end data tests. Use the integration branch's `tests/rust_encoder_roundtrip.cmake`
and `tests/verify_record_seeks.cpp`; its README gives complete commands.

The [bridge timing CSV](results/2026-09-13-blitzcrank-bridge-census.csv) contains
five alternating legacy/Rust CLI `-b` runs on CPU 2. These are **DC vs DC**, not
rANS. Median wall times in seconds for the 20,000-row fixture:

| Block threshold | Legacy encode | Rust bridge encode | Legacy decode | Bridge-build C++ decode |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.031738 | 0.030508 | 0.022829 | 0.023856 |
| 20,000 | 0.033150 | 0.030765 | 0.023193 | 0.024022 |

The CLI's compression timer includes field-to-branch translation, coding and
payload writing, but excludes learning. Decode includes semantic reconstruction,
excluding model initialization. Runs are short, one machine, not a general
performance guarantee. The bridge encoder is about 4–7% faster in these runs,
while retained C++ decoding is 3.6–4.5% slower. Enlarging C++ Branch objects and
retaining Rust mapping snapshots may contribute to the decode regression; that
is a hypothesis, not a measured cache-causal result. Pointer gathering, additional
model memory and compatibility word copying remain engineering costs to remove.
These bridge timings preceded the fixed-model group kernel's final inline tuning;
the timed selected-branch encoder path was unchanged.

## Validation and next decision

- Rust Release/default on MSRV 1.88; all-features Release; strict Clippy pass.
- C/C++ tests: 18/18, including mixed-branch original-C++ oracle and short/tail
  four-state/SSE baseline adapters; 18/18 again with C++ ASan/UBSan.
- Miri passes both FFI ownership/buffer and branch-handle tests (145 seconds).
- Updated malformed-input differential fuzzing compares ordinary/lookahead and
  four-state groups: 32,248 executions in 61 seconds with ASan, no failure.
  This fuzz harness currently selects one/four states; Rust property tests also
  cover two/eight. Bounded fuzzing is not a safety proof.

Next: record-level conditional decoding and a same-semantics rANS application
backend, then random-access latency versus **total** memory over lane counts.
Until that exists, this checkpoint demonstrates working integration and a useful
DC kernel optimization, not the requested strong application-level advantage.
