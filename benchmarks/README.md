# Reproducing benchmarks

Start with the [upstream calibration report](UPSTREAM_CALIBRATION.md). Historical
generic rANS adapters are retained as controls, not advertised as upstream-speed
implementations. The build requires the complete pinned checkout (LF source files);
source/header hashes are verified before extracting original coding blocks.

## Original programs and adapter calibration

After checking out the pinned upstream revision below, on x86-64 Linux:

```sh
# Original Makefile flags, unchanged source/timers, original book1 and u8 output.
# Use a new output directory; no files are written into the upstream checkout.
cmake -DUPSTREAM=/tmp/ryg_rans -DOUTPUT_DIR=/tmp/rans-original-results \
  -DCPU=2 -P benchmarks/run_upstream_original.cmake
# With the SIMD-enabled build described below:
taskset -c 2 build/calibrate_upstream /tmp/ryg_rans/book1
taskset -c 2 build/calibrate_upstream_scalar /tmp/ryg_rans/book1
```

Calibration preserves upstream normalization (14-bit scalar, 12-bit SIMD), tests
both u8/u32 I/O, and verifies encoded bytes against a scalar-per-symbol oracle.
The scalar-only executable omits the SSE4.1 compiler flag. Both retain upstream
assertions. `--check` covers empty blocks with a supplied model, short tails and
several alphabets; these checks also run through CTest. CI runs original programs
with `CPU=none` for correctness only, not as stable performance measurements.

New shared-harness rows distinguish provenance:

- `rans_byte_upstream_2`, `rans64_upstream_2`: extracted original two-state loops.
- `rans64_upstream_style_4`: derived four-state loop, preserving lookup-all,
  step-all, renormalize-all ordering; byte-checked against the generic encoder.
- `rans_sse41_upstream_8`: extracted original two-group/eight-state SIMD loop.
- Older `rans64_4`, byte and SSE4 rows: our historical adapters using upstream
  headers, not the complete original demonstration loops.

## Shared probability / u32 benchmark

No benchmark dependency is downloaded automatically. For the upstream comparison:

```sh
git clone https://github.com/rygorous/ryg_rans.git /tmp/ryg_rans
git -C /tmp/ryg_rans checkout c9d162d996fd600315af9ae8eb89d832576cb32d
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release \
  -DDELAYED_CODING_RANS_DIR=/tmp/ryg_rans
cmake --build build -j
taskset -c 2 ./build/compare_rans 4096
# Optional real byte file, measured as one block (1 byte becomes 1 u32 symbol):
taskset -c 2 ./build/compare_rans --file /tmp/ryg_rans/book1
```

Choose an available CPU on your machine; omit `taskset` on non-Linux systems.
Run several block sizes (e.g. 16, 64, 4096, 65536). Output is CSV. Each cell is
the median of seven samples, each repeating at least 262144 symbols unless a
single block exceeds that size. No universal speed claim follows from this small
suite. The byte and rANS64 baselines use their optimized reciprocal encoders.
Alias rows use the upstream demo's division-based encoder (explicitly not the
best possible rANS encoder); their main purpose is a compact-lookup decoder comparison.

## Contract and limitations

- Same generated u32 symbol arrays, same normalized 16-bit weights, same process.
- Both sides write u32 symbols, avoiding an output-width mismatch. `ns/symbol` is
  the primary metric; payload bits/symbol is reported beside it.
- rANS uses unmodified `rans_byte.h` / `rans64.h`. Loop provenance and state count
  are explicit above; an unchanged header is not a claim of optimal outer loops.
- Alias rows include unmodified upstream `main_alias.cpp`, with only the demo
  entry point renamed. Models use exactly the same weights without renormalizing.
  Decoder tables occupy 5.5 KiB (`divider`, `slot_adjust`, `slot_freqs`, `sym_id`),
  versus 64 KiB for the direct symbol lookup plus frequencies/starts. The upstream
  alias encoder also allocates a 256 KiB remap table. All 65,536 code points are
  checked before timing; state/cursor and block roundtrips are checked too.
  These rows are enabled only on x86 Linux/Windows because of the upstream demo's
  platform helper. `compare_rans N --check` skips timing for correctness checks;
  CTest covers short blocks and non-multiple-of-four tails when rANS is enabled.
- Delayed Coding uses the Rust C ABI once per block, bounded reads and final-state
  checks. rANS's inner input reads are unchecked; this is disclosed, not hidden.
- Models and buffers are built before timing; encoder workspace is warmed and
  reused. Timings include state reset and payload finalization. The volatile
  checksum makes results observable; roundtrips are checked outside timing.
- Repeated data and models are cache-warm. Synthetic data is generated from the
  benchmark's model. This does not cover mismatched models or changing distributions.
- File mode reads 1 byte..64 MiB, derives one model from the entire input using
  reserve-one/largest-remainder normalization, and supplies identical weights and
  symbols to every codec. File I/O, histogram construction and model setup are
  excluded. This is a zero-order entropy-kernel test, not whole-file compression;
  it omits model metadata and does not exercise chunked/random-access storage.
- The C++ harness currently covers fixed models. Randomized access, many-model comparisons
  against rANS, hardware counters and whole-block model/framing cost remain work.
- Do not mix CPU flags, compilers, probability precision or lane counts without
  stating it. The initial CSVs use generic x86-64 Rust and GCC Release defaults,
  no `target-cpu=native` or `-march=native`.

The Rust-only benchmark also reports table bytes, model build time and known
round-robin switching over 1/16/256 models. It uses a different deterministic
generator from the C++ comparison, so do not compare its rows to rANS CSVs:

```sh
cargo bench --bench throughput -- 4096
# Optional real byte file, <=64 MiB:
cargo bench --bench throughput -- 4096 /path/to/file
```

## SSE4.1 comparison (separate probability suite)

```sh
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release \
  -DDELAYED_CODING_RANS_DIR=/tmp/ryg_rans \
  -DDELAYED_CODING_BUILD_SIMD_BENCHMARK=ON
cmake --build build -j
taskset -c 2 ./build/compare_rans_simd 4096
taskset -c 2 ./build/compare_rans_simd --file /tmp/ryg_rans/book1
```

Requires x86 Linux/Windows, GCC/Clang and an SSE4.1-capable CPU. This separate
executable compiles C++ with `-msse4.1`; the Rust core retains its ordinary flags.
It uses the unmodified `rans_word_sse41.h` four-state decoder, in either the old
single-group adapter or the extracted upstream two-group/eight-state loop. It
fixes probability precision at 12 bits: the entire suite first constructs 12-bit
weights and multiplies them by 16 for all non-SIMD codecs. Thus probabilities and
input symbols match exactly within a run. Rows carry `_p12`; especially the
near-constant model differs from the ordinary 16-bit suite, so do not mix rows.

The upstream SIMD encoder uses division. Its decoder uses a 20 KiB table, then
widening stores to the same u32 output used by other codecs. State initialization,
scalar tails, normalization and final checks are included. Upstream requires eight
readable bytes after the payload; the harness allocates that padding but excludes
it from `payload_bytes`. This extra requirement does not apply to the bounded
Rust decoder. The upstream SIMD encoder cannot handle a full-frequency one-symbol
model, so that variant is explicitly skipped for such a file rather than changed.

These are illustrative upstream SIMD kernels, not a comparison against every
modern vectorized rANS implementation or a maximum-throughput byte-output test.

## Ablation

```sh
cmake -S . -B build-division -DCMAKE_BUILD_TYPE=Release \
  -DDELAYED_CODING_RANS_DIR=/tmp/ryg_rans \
  -DDELAYED_CODING_REFERENCE_DIVISION=ON
cmake --build build-division -j
taskset -c 2 ./build-division/compare_rans 4096
cargo test --features reference-division
```

Record `rustc -Vv`, C++ compiler version, `lscpu`, CPU affinity, dependency
commits, dirty diff (if any) and command line with every result. Measure setup and
container costs separately before using numbers to choose an application codec.

For the experimental flat alias layout, configure a separate CMake build with
`-DDELAYED_CODING_FLAT_ALIAS=ON`, or run Rust benchmarks with
`cargo bench --features flat-alias --bench throughput -- 4096`.
Compare fixed models **and** model switching; the latter regresses substantially
in the initial experiment, which is why this feature is not enabled by default.

## Layout / lookahead extension

The [scoped layout/lookahead report](LAYOUT_LOOKAHEAD.md) documents the optional
one-state decoder and the new `ramp256` nonuniform high-entropy input. The regular
harness now also includes explicitly labelled 512 KiB packed-table rANS adapters
for a direct-table control; upstream headers are unchanged. These adapters are
not claimed to be the best possible rANS kernels.

`compare_rans --records PATH [--check]` resets each coder for 8/16/32/64/256/4096
byte records, reuses one global model, validates every record, and reports total
payload plus the same u32 offset index. This mode measures sizes, not throughput;
it excludes shared model/framing metadata. `indexed_records` is the executable
Rust exact-layout/random-access example.

## Four-state and Blitzcrank bridge extension

The regular harness also reports opt-in four-state physical lookahead and grouped
capacity-planned DC decoding; `--records` includes one- and four-state size rows
for both families. See [the experiment report](BLITZCRANK_FOUR_STATE.md) for
canonical results, intermediate regressions and the real Census encoder bridge.
The bridge benchmark compares Rust DC to original C++ DC, **not** to rANS.

## Branchless speed paths and profiling

See [SPEED_KERNELS](SPEED_KERNELS.md) for the next optimized kernels, negative
ablations and their limits. Default builds expose branchless four-state decoding
as a separate API. To test optional speculative multi-lane encoding:

```sh
cmake -S . -B build-speed -DCMAKE_BUILD_TYPE=Release \
  -DDELAYED_CODING_RANS_DIR=/tmp/ryg_rans \
  -DDELAYED_CODING_SPECULATIVE_ENCODE=ON \
  -DDELAYED_CODING_BUILD_SIMD_BENCHMARK=ON
cmake --build build-speed -j
ctest --test-dir build-speed --output-on-failure
DC_BENCH_MIN_SYMBOLS=4194304 taskset -c 2 build-speed/compare_rans --file /tmp/ryg_rans/book1
```

For focused profiling, `DC_BENCH_CODEC` selects an exact codec row name and
`DC_BENCH_MIN_SYMBOLS` overrides the default sample-work target (1..2^30).
Actual repeats remain `max(1, target / block_length)`, rounded down. Seven median
samples are retained. `--check` ignores the codec filter and validates all codecs.
Environment settings are printed to stderr and must accompany recorded results.

```sh
DC_BENCH_CODEC=delayed24_4_branchless_both DC_BENCH_MIN_SYMBOLS=16777216 \
  perf record -o /tmp/dc-profile.data -- taskset -c 2 build-speed/compare_rans --file /tmp/ryg_rans/book1
perf report --stdio -i /tmp/dc-profile.data
```
