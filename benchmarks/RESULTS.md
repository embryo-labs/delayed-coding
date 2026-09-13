# Performance experiments — 2026-09-12–13

Latest scoped results: [exact layouts, physical lookahead and independent short
records](LAYOUT_LOOKAHEAD.md). Includes negative results and stronger packed-table
rANS adapters. Earlier sections below retain the historical experiments.

Machine: Intel Xeon Platinum 8474C, Linux x86-64. CPU affinity: logical CPU 2.
Rust 1.92.0; GCC 12.2.0, CMake Release. Rust release uses thin LTO and one codegen
unit. No native-CPU flags. rANS upstream:
`c9d162d996fd600315af9ae8eb89d832576cb32d`.

The initial rows are cache-warm kernel measurements with fixed models and 4096 u32
symbols per block; later sections also measure a complete real text file. The
process has a Rust/C block boundary for Delayed Coding. Seven
samples per metric; report the median. See [methodology](README.md) before using
these numbers. This is not an application-level comparison. SSE4.1 rows use a
separate matched-probability suite as described below.

## Findings

The first safe Rust extraction showed a clear encoding bottleneck: linear scans
over the many disjoint alias segments of a high-frequency symbol. The early
development snapshot took about 40 ns/symbol on `skewed` and 66 ns/symbol on
`near_constant` at delay 24. Binary search lowered those to approximately 15 and
31 ns/symbol. A direct encode table lowered them further to about 8 and 9 ns,
at an additional 128 KiB/model. Uniform inputs did not benefit from the large table.

Four independent coding states reduce the decoder's serial dependence, and share
one stream without lane-length headers. The initial interleaved implementation
still trails four-state rANS. Direct decode tables have a 512 KiB/model cost and
can regress scalar decoding, so they remain opt-in.

From `results/2026-09-12-xeon-8474c-4096-interleaved.csv`:

| Distribution | Delayed24 scalar decode ns/symbol | Delayed24 4-state, direct tables | rANS64 4-state | Delayed/rANS64 payload bytes (4-state) |
| --- | ---: | ---: | ---: | ---: |
| uniform256 | 4.5402 | 2.9175 | 2.1883 | 4104 / 4128 |
| uniform16 | 6.1541 | 2.8879 | 2.1235 | 2064 / 2080 |
| skewed | 5.7652 | 3.0307 | 2.1653 | 2550 / 2556 |
| near_constant | 7.8079 | 2.9232 | 2.1101 | 40 / 52 |

These synthetic cases suggest useful short-block overhead and scalar-decoding
properties; they do not establish a general advantage over rANS. In particular,
rANS encoding remains considerably faster in this initial suite. Next work is
to shorten the virtual-word/state path, measure table footprint and cache effects,
and test alias/SIMD rANS, real data and whole-block costs.

The `initial`, `binary`, `tables` and `interleaved` CSVs were collected during
development before the first repository commit. They document experiment history,
not four separately versioned releases. Subsequent results should identify an
exact source revision and any dirty patch in a run manifest.

## Deferred normalization (2026-09-13)

Starting from `e9634dd`, normalization moved to the next read of a state. The
denominator encodes whether a virtual word exists, and the numerator holds its
bits; a separate Option and the second branch were unnecessary. All property
and original-C++ differential checks still pass.

`results/2026-09-13-xeon-8474c-4096-deferred.csv` records the same harness, CPU and
flags. Four-state compact decoding fell from about 3.1–3.3 to 2.58–2.64 ns/symbol
across these four distributions. This is approximately 17–19% less time, without
the 512 KiB decode table. Four-state rANS64 remained around 2.11–2.22 ns/symbol.
Scalar throughput was largely unchanged. This is a measured implementation
improvement, not a new compressed format.

A subsequent chunked-loop experiment attempted to expose fixed lane indices to
the optimizer. It regressed four-state decoding to roughly 3.4–3.9 ns/symbol in
this build and was reverted. The `2026-09-13-...-batched.csv` file retains those
negative results. The simpler per-symbol loop remains the default.

## Alias, real data and SSE4.1 baselines (2026-09-13)

The unmodified upstream alias decoder takes about 2.13–2.34 ns/symbol with four
states on the original 4096-symbol synthetic suite. Its division-based encoder
is not representative of the best rANS encoder. Adding alias coding does not
remove the performance gap shown by byte/64-bit rANS.

On the full 768771-byte `book1`, timings are materially different from the small
repeated synthetic blocks. At baseline `19199fe`, adjacent measurements gave:

| 16-bit probability suite, full book1 | Decode ns/symbol | Payload bytes |
| --- | ---: | ---: |
| Delayed24, four-state compact | 6.6705 | 435088 |
| Delayed24, four-state direct tables | 4.2740 | 435088 |
| rANS byte, four-state | 4.8791 | 435067 |
| rANS64, four-state | 3.7132 | 435080 |
| rANS alias, four-state | 5.1335 | 435067 |

This is zero-order coding with a model trained on the whole file. Neither model
storage nor training/I/O/index overhead is counted. It is not a file-compressor
benchmark or evidence of end-to-end Blitzcrank throughput.

The SSE4.1 header is fixed at 12-bit precision. A separate suite gives it 12-bit
frequencies and multiplies these by 16 for every other coder, preserving identical
probabilities. On `book1`, four-state SSE4.1 measured 3.7124 ns/symbol versus
3.6968 for rANS64 in that same suite. The SIMD wrapper widens results to u32 and
uses one four-lane register; it is not the upstream eight-state byte-output demo
or a best-of-all-SIMD claim. Required eight-byte readable padding is allocated
but excluded from payload size. See the complete [run manifest](results/2026-09-13-MANIFEST.md).

## Flat alias layout: real-data improvement with a small-block tradeoff

Assembly inspection of `19199fe` showed a conditional jump for selecting a
bucket's left/right slot. Flat cutoff and slot arrays permit boolean-index
addressing and remove that data-dependent selection branch. Core revision
`ce542e51807aae06bcb2ec5caee4225ead0f6998` records that experiment. Subsequent
many-model evidence led to restoring the original layout by default; flat slots
are now available only through the opt-in `flat-alias` Cargo feature.

On full `book1`, four-state compact decoding falls from 6.6705 to 4.5623 ns/symbol
(about 32% less decoding time). Scalar decoding changes from 7.9588 to 7.6874.
Payloads remain identical; compact decode storage is still 28 bytes per bucket,
now split between two allocations. It does not add a 512 KiB direct table.
The extra model-header/allocation bookkeeping and construction costs must still
be included when evaluating whole applications.

This is not a universal win. Four-state uniform/near-constant 4096-symbol loops
move from roughly 2.6–2.7 to 2.9–3.0 ns/symbol. Skewed short-block measurements
are especially variable. More importantly, the native Rust benchmark's model
switching regresses from 6.16/8.28/10.99 ns per symbol at 1/16/256 models to
9.03/11.53/18.96. Model-table bytes are identical, but building the four synthetic
models is about 6–8% slower. These observations outweigh the fixed-model gain
for Blitzcrank's conditional workloads: flat slots are **not the default**.
They remain an opt-in experiment, and the intermediate indexed-bucket layouts
are retained as patches/results. The native harness has a different generator
and call pattern, so its rows should not be compared directly to the C++ rows.

Encoding remains the larger gap on `book1`: about 20.3 ns/symbol for four-state
compact Delayed Coding, 8.9 with direct encoding tables, versus 3.2 for rANS64.
Next work must address inverse alias lookup, conditional-model/FFI costs and
small-record behavior; a decoder-only headline would hide this gap.
