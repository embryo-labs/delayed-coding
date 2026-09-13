# Upstream rANS calibration and corrected baseline

2026-09-13. DC source: `0909cf4f3f5c0f480b06eb51422a2fc816cac72a`.
Harness: the commit containing this report. No Rust codec change in this checkpoint.
Upstream: [rygorous/ryg_rans](https://github.com/rygorous/ryg_rans/tree/c9d162d996fd600315af9ae8eb89d832576cb32d),
revision `c9d162d996fd600315af9ae8eb89d832576cb32d`.

## Conclusion

The earlier book1 DC4 lead over `rans64_4` does **not** establish a speed advantage
over optimized rANS. Our old outer loop serialized lookup/update/renormalization
per lane; the upstream demonstration separates these phases across lanes.
The old SIMD adapter also used only one four-lane group, whereas the upstream
SIMD demonstration overlaps two such groups. Unmodified headers did not make
those adapters equivalent to the complete upstream demonstrations.

After correcting these controls, current DC4 loses to four-state rANS64 on book1.
The original eight-state SIMD implementation is faster still. This does not
invalidate measured DC-versus-older-DC improvements, but it withdraws the prior
relative-rANS lead. No application-level or general algorithmic speed advantage
has been demonstrated. Alias variants are not part of this calibration.

## Same-probability, same-u32-I/O results

All rows below are kernel timings on book1, prebuilt models and reused buffers;
lower ns/symbol is better. DC uses four states, delay 24, direct encode/decode
tables and the optional speculative encoder. Both encode and decode are included
in the table; setup is not. Encoded bytes include state termination, not models.

16-bit normalized probabilities, same input/output and four states:

| Implementation | Payload bytes | Encode ns/symbol | Decode ns/symbol |
| --- | ---: | ---: | ---: |
| DC4 branchless/direct | 435088 | 4.5942 | 3.2082 |
| Old generic rANS64 adapter, 4 states | 435080 | 3.1519 | 3.6614 |
| Derived upstream-style rANS64, 4 states | 435080 | 2.6233 | 2.6929 |

The new four-state row is **derived**, not an original upstream four-state demo.
It extends main64.cpp's two-state phase ordering using explicitly expanded
four-lane operations and upstream symbol-step/renormalization functions. It also
uses upstream-style interleaved start/frequency decoder symbols. Its encoder
matches the old generic four-state encoder byte for byte. This combined control
is not an isolated ablation of scheduling alone. DC needs about 16% less decode
time to match it; rANS64 delivers about 19% more symbols/second in this run.

Separate suite with identical 12-bit probabilities for every codec (scaled
exactly to 16-bit weights where required), still u32 input/output:

| Implementation | States | Payload bytes | Encode ns/symbol | Decode ns/symbol |
| --- | ---: | ---: | ---: | ---: |
| DC branchless/direct | 4 | 435636 | 4.5948 | 3.2093 |
| Derived upstream-style rANS64 | 4 | 435628 | 2.6457 | 2.6340 |
| Old SSE4.1 adapter | 4 | 435618 | 3.7101 | 3.7144 |
| Extracted upstream SSE4.1 loop | 8 | 435630 | 3.9826 | 1.7662 |

Eight versus four is **not an equal-state-count experiment**. It shows an
available upstream implementation's throughput, not an inherent eight-state
limit for DC. No claim is made that this old reference is the fastest modern rANS.
DC direct tables add 128 KiB for encoding and 512 KiB for decoding; scalar rANS64
uses a 64 KiB symbol lookup plus small symbol tables, and SSE uses a 20 KiB table.
DC's bounded reads differ from unchecked rANS inner reads; SSE needs eight
readable padding bytes, excluded from payload size. These contracts remain visible.

Raw data: [16-bit shared suite](results/2026-09-13-upstream-corrected-book1.csv),
[12-bit shared suite](results/2026-09-13-upstream-corrected-book1-p12.csv).

## Actual original programs, then extraction calibration

The original main.cpp and main64.cpp interleaved demonstrations have **two**
states, 14-bit probabilities and u8 I/O. main_simd.cpp has **eight** states,
12-bit probabilities and u8 I/O. These use upstream normalization, which differs
from our shared suite. Their native numbers must not be mixed into the shared
16-bit/u32 table as though conditions matched.

The script builds the original sources with the Makefile's flags (`g++ -O3`,
`-lm -lrt`; SIMD additionally `-msse4.1`), without rewriting sources, entry points
or timers. All original roundtrip checks passed. Logs retain every sample:
[byte](results/2026-09-13-upstream-original-byte.txt),
[rANS64](results/2026-09-13-upstream-original-rans64.txt),
[SIMD](results/2026-09-13-upstream-original-simd.txt).

| Original interleaved program | Precision | Original u8 decode ns/symbol | Extracted u8 decode ns/symbol |
| --- | ---: | ---: | ---: |
| byte rANS, 2 states | 14 | ~4.68 | 4.8671 |
| rANS64, 2 states | 14 | ~3.43 | 3.8493 |
| SSE4.1, 8 states | 12 | ~1.77 | 1.7644 |

Original times are converted from the median of five printed throughput samples
using `1e9 / (MiB_per_second * 1048576)`. Some upstream labels say MB/s, but the
code divides by 1048576. Printed TSC clock counts are not claimed as core cycles.
Extracted calibration uses seven samples with five book1 blocks per sample.
Scalar rows here use the scalar-only build, without `-msse4.1`.

**Remaining calibration gap:** extracted scalar byte decoding is about 4% slower;
extracted two-state rANS64 is about 12% slower than its native original program.
The cause is not yet isolated. Compiler context, wrappers, layout and timing
method differ; exact probability precision, assertions and a separate no-SSE
build did not eliminate the gap. We do not claim scalar extraction is
performance-neutral, and do not replace native scalar results with slower ones.
The SIMD result matches closely; its extracted u32 variant is 1.7635 ns/symbol,
so u32 widening does not explain the old ~3.7 ns/symbol SSE adapter result here.

Calibration data: [scalar-only build](results/2026-09-13-upstream-calibration-scalar.csv),
[SSE-enabled build, both I/O widths](results/2026-09-13-upstream-calibration.csv).
Neither calibration CSV measures DC or uses the shared normalization policy.

## Source fidelity and correctness

`extract_upstream.cmake` verifies SHA-256 hashes of the three demo sources and
their rANS/platform headers, then extracts complete encode/decode blocks into a
build-local generated header. It preserves original lane order, lookup/advance/
renormalization phases and tail logic. No upstream checkout files are changed.
Wrapper differences are explicit:

- Input/output element type is templated for u8 and u32; scalar precision remains
  a compile-time constant (14 for calibration, 16 for shared comparisons).
- Caller-owned buffers and tables replace the demo's local allocations; timers
  are outside the extracted body. Final cursor/state checks are added after it.
- Two SIMD packed stores become memcpy for u8 or widening SIMD stores for u32;
  coding order is unchanged. Word buffers are aligned and SIMD padding retained.
- Calibration includes cassert before upstream headers and undefines NDEBUG to
  match original assertions. The shared Release benchmark retains its existing
  disabled upstream inner assertions; added final checks remain active.

An independent scalar-per-symbol encoder verifies encoded bytes and roundtrips
for both I/O widths, multiple alphabets and odd/short tails. Empty blocks are
tested with an externally supplied model. Unsupported full-frequency SIMD
models, and full-frequency 16-bit byte decoder symbols, are explicitly skipped
or rejected, not silently changed. The derived four-state row separately checks
its bytes against the generic encoder before measurement.

Validation: Release CTest **20/20**, C++ ASan/UBSan CTest **20/20**. The sanitizers
instrument the C/C++ harness and adapters, not the separately compiled Rust core.
CI now includes unchanged original-program roundtrips and both calibration
drivers; remote CI has not been run for this local checkpoint.

## Reproduction and next acceptance target

Xeon Platinum 8474C, Linux x86-64, CPU 2 affinity, GCC 12.2, Rust 1.92,
generic x86-64, no native CPU flags; Rust thin LTO/codegen-units=1. Timed runs
were serial, with cache-warm data/models. No file I/O, model construction,
serialization or indexes are timed. DC includes one C ABI call per block.
These are single-machine measurements, not confidence intervals across machines.
book1: 768771 bytes, SHA-256
`9ffa47cd93bccd732f20e0c304203cfbc1b8a91bedac536e2d8f6051003d9951`.

```sh
cmake -DUPSTREAM=/tmp/ryg_rans -DOUTPUT_DIR=/tmp/rans-original-results \
  -DCPU=2 -P benchmarks/run_upstream_original.cmake
cmake -S . -B build-speed -DCMAKE_BUILD_TYPE=Release \
  -DDELAYED_CODING_RANS_DIR=/tmp/ryg_rans \
  -DDELAYED_CODING_BUILD_SIMD_BENCHMARK=ON \
  -DDELAYED_CODING_SPECULATIVE_ENCODE=ON
cmake --build build-speed -j4
ctest --test-dir build-speed --output-on-failure
taskset -c 2 build-speed/calibrate_upstream /tmp/ryg_rans/book1
taskset -c 2 build-speed/calibrate_upstream_scalar /tmp/ryg_rans/book1
DC_BENCH_MIN_SYMBOLS=4194304 taskset -c 2 build-speed/compare_rans --file /tmp/ryg_rans/book1
DC_BENCH_MIN_SYMBOLS=4194304 taskset -c 2 build-speed/compare_rans_simd --file /tmp/ryg_rans/book1
```

Immediate targets are below ~2.69 ns/symbol for DC4 at matched 16-bit
probabilities and a direct eight-state DC comparison against the ~1.77 ns/symbol
SSE reference at matched 12-bit probabilities. Encoding, table footprint and
small/conditional workloads must also be retained in acceptance tests. A genuine
Blitzcrank advantage still requires end-to-end conditional decoding and record
access measurements; a warm book1 kernel cannot establish that contribution.
