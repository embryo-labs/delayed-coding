# Logarithmic DC: a bounded four-state reference win

2026-09-13. Source is the commit containing this report, based on the calibrated
checkpoint `2ba34dc342c3461615cf266f24ff36e08979f874`.

## Result and scope

The new optional **logarithmic DC derivative** beats the tested upstream-style
four-state rANS64 reference in both encoding and decoding on book1. This is a
fixed-model, four-state kernel result on one machine, **not** a general claim
that DC is the best entropy coder. The original DC format remains unchanged and
does not acquire these numbers. The new format costs a little compression ratio.

Identical 16-bit normalized frequencies, same u32 input/output, four states,
prebuilt models and reused buffers, same executable and generic CPU flags:

| Implementation | Payload bytes | Encode ns/symbol | Decode ns/symbol |
| --- | ---: | ---: | ---: |
| Log-DC4 | 435754 | 2.4073 | 2.5425 |
| Upstream-style rANS64, 4 states | 435080 | 2.6517 | 2.6472 |

These are medians across five alternating-order process pairs; each cell in a
pair is itself the median of seven samples, with 21 book1 blocks per sample.
Log-DC takes **9.2% less encoding time and 4.0% less decoding time**, or about
10.2% / 4.1% more symbols per second. Payload is **674 bytes / 0.155% larger**.
Across the five pairs, log-DC encoding ranged 2.4068–2.4107 and decoding
2.5281–2.5610 ns/symbol; rANS64 ranged 2.6492–2.6529 and 2.6462–2.6523.
All five pairs showed both wins. These are repeatability observations, not
confidence intervals or cross-machine validation.

The rANS row uses unchanged upstream headers and the explicitly derived
lookup-all/step-all/renormalize-all four-state loop introduced during
[baseline calibration](UPSTREAM_CALIBRATION.md). It is not relabelled as an
unmodified upstream four-state demo: that upstream scalar demo has two states.
Original two-state and eight-state SIMD loops remain in the full suite.
This is not an exhaustive search over optimized rANS implementations.

Raw data: [five paired runs](results/2026-09-13-log-book1-paired.csv),
[independent scripted repeat](results/2026-09-13-log-book1-paired-repeat.csv),
[full book1 suite](results/2026-09-13-log-book1.csv),
[matched 12-bit suite](results/2026-09-13-log-book1-p12.csv),
[65536-symbol synthetic blocks](results/2026-09-13-log-65536.csv),
[16-symbol blocks](results/2026-09-13-log-16.csv).

## What changed

- A conservative fixed-point logarithmic capacity schedule replaces the exact
  capacity multiplication with integer additions and conditional subtraction.
  The layout still depends on realized frequencies alone, not the information
  numerator. A proof and the rate-cost identity are in [the algorithm note](../docs/LOG_SCHEDULE.md).
- A separate cumulative-interval model avoids the old encoder's disjoint-interval
  lookup. This is a mapping choice available to both families, not a unique DC
  contribution. Probabilities are identical to rANS, but payloads/rate differ.
- Four-state validation/scheduling is grouped, using a small log table. The
  numerator update uses `n + q*(65536-f) + start`, selecting it or q according to
  the precomputed schedule. This removes variable-shift/mask work from encoding.
- The byte-alphabet decoder uses a 64 KiB symbol lookup and a 2 KiB packed
  frequency/start/log table; field layout and source selection avoid unnecessary
  masks and variable shifts. Total allocated model tables for 256 IDs are
  72192 bytes, versus the previous DC speed path's extra 640 KiB direct tables.

The combined gain is not an isolated causal ablation of the logarithmic schedule.
The core remains safe Rust with `forbid(unsafe_code)`; no per-symbol FFI call,
input padding or hot-path allocation was added. Models are immutable and shared;
each encoder reuses an ordinary Workspace. The C API makes one call per block.
The [failed prototypes](experiments/README.md), including an AVX2 gather attempt,
are retained as research artifacts but are not library paths or dependencies.

## Where it still loses

The full 12-bit-probability suite reports log-DC4 decode at 2.5401 ns/symbol,
versus the original eight-state SSE4.1 loop at **1.7641**. Log-DC8 is slower
still (3.1108). This is not a win over the best tested SIMD decoder. The newly
added eight-state rANS64 control is also faster than log-DC8 at decoding.

Synthetic uniform256 encoding is 2.3996 vs rANS64 four-state 1.2855 ns/symbol;
near-constant encoding is 2.4047 vs 1.3002. Even on skewed synthetic data where
log-DC encodes faster (2.4033 vs 2.6653), it decodes slightly slower (2.5721 vs
2.5023). The book1 win must not be generalized to these distributions. On the
near-constant block the extra 72 payload bytes are a substantial **12.4%** size
penalty relative to rANS's 580 bytes, despite a small absolute bits/symbol cost.
Sixteen-symbol blocks remain slower than rANS, though several payloads are smaller.

There is no conditional-model API for LogModel yet and no new-format Blitzcrank
integration, model serialization, framing, storage/seek benchmark or publication
claim. The existing bit-compatible Rust bridge remains on ordinary DC. An
application-level advantage and faster eight-state SIMD decoding remain open.

## Reproduce and validate

Xeon Platinum 8474C, Linux x86-64, CPU 2 affinity. Rust 1.92.0 / LLVM 21.1.3,
GCC 12.2.0, CMake Release, Rust thin LTO/codegen-units=1. No native CPU flags;
only the separate SSE suite adds `-msse4.1` to C++. Timed processes ran serially,
without concurrent compilation/tests/fuzzing. Data/models were cache-warm. Model
construction, normalization, allocation, file I/O, serialization and indexing
are excluded; state initialization/finalization and DC's block C ABI call are
included. rANS inner reads are unchecked; DC reads are bounded. SSE's required
eight readable padding bytes are allocated but not counted as payload.

Upstream ryg_rans: `c9d162d996fd600315af9ae8eb89d832576cb32d`, unchanged checkout.
book1: 768771 bytes, SHA-256
`9ffa47cd93bccd732f20e0c304203cfbc1b8a91bedac536e2d8f6051003d9951`.

```sh
cmake -S . -B build-log -DCMAKE_BUILD_TYPE=Release \
  -DDELAYED_CODING_RANS_DIR=/tmp/ryg_rans \
  -DDELAYED_CODING_LOG_SCHEDULE=ON \
  -DDELAYED_CODING_SPECULATIVE_ENCODE=ON \
  -DDELAYED_CODING_BUILD_SIMD_BENCHMARK=ON
cmake --build build-log -j4
ctest --test-dir build-log --output-on-failure
cmake -DBENCH="$PWD/build-log/compare_rans" -DINPUT=/tmp/ryg_rans/book1 \
  -DOUTPUT=/tmp/log-pairs.csv -DCPU=2 -P benchmarks/run_log_pairs.cmake
DC_BENCH_MIN_SYMBOLS=4194304 taskset -c 2 build-log/compare_rans --file /tmp/ryg_rans/book1
DC_BENCH_MIN_SYMBOLS=4194304 taskset -c 2 build-log/compare_rans_simd --file /tmp/ryg_rans/book1
taskset -c 2 build-log/compare_rans 65536
taskset -c 2 build-log/compare_rans 16
```

Validation completed locally: all-feature Rust tests in Debug and Rust 1.88
Release; all-target/all-feature Clippy; Release CTest 18/18; C++ ASan/UBSan CTest
20/20 including legacy differential checks (the Rust core is not instrumented by
those C++ flags). Miri passed the new C ABI ownership/buffer test. The expanded
fuzzer completed 23467 runs in 46 seconds without a crash; this is a smoke test,
not a security audit. CI definitions include the new paths; remote CI has not run.

Default APIs and formats are unchanged. Enable `log-schedule` explicitly in Rust
or `DELAYED_CODING_LOG_SCHEDULE` in CMake, and identify this format separately in
any container. Do not silently use it to overwrite existing Blitzcrank payloads.
