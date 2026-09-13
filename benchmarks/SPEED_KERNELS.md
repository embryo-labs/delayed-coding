# Speed checkpoint: branchless four-state DC

**Historical checkpoint; relative-rANS speed conclusions superseded.** The DC
before/after improvements remain measured, but the old rANS adapters did not
preserve upstream scheduling. The [corrected baseline](UPSTREAM_CALIBRATION.md)
has four-state rANS64 at 2.69 ns/symbol versus DC4 at 3.21 on book1. Do not cite
this report's DC/rANS lead as an advantage over optimized rANS.

2026-09-13. Source baseline: `1f8f872faab908065ca4c439bbe90475a461fef5`;
optimized source: the commit containing this report. This is a fixed-model
kernel improvement, not completion of the Blitzcrank application milestone.

## Implemented changes

1. Internal scheduling skips repeated frequency/overflow checks already guaranteed
   by immutable models and the checked block-size bound. Public `Layout` checks
   and all externally visible errors are retained. This applies to both model/event
   and selected-branch scheduling.
2. Four-state grouped decoders select direct/compact table mode once per block.
   The direct path borrows an exact-size array, letting safe Rust eliminate
   redundant lookup bounds checks. The scalar/error tail is outside the group loop.
3. New `decode_branchless4_into` / `dc_decode_branchless4` uses masks for physical
   versus virtual sources. A bounded eight-byte window permits discarded loads
   without requiring padding. Original ordinary/grouped decoders stay available.
4. Optional `speculative-encode` removes the conditional store in 4/8-state
   **model/event** encoding. Every speculative write is within the final payload
   and is overwritten when the corresponding physical slot is reached. Exact
   capacity, original bytes and untouched prefixes are preserved. Single-state
   and selected-branch embedding retain their previous loop. See the
   [invariants](../docs/ALGORITHM.md#branchless-four-state-windows-and-speculative-encoding).

No unsafe code, new input-padding contract, format change, additional runtime
allocation or per-symbol FFI call was introduced. Direct tables still trade memory
for speed. The new paths are explicit choices, not universal defaults.

## Reproducibility

Xeon Platinum 8474C, Linux x86-64, CPU 2 affinity, Rust 1.92/GCC 12.2, CMake
Release, generic x86-64 (no native CPU flags), Rust thin LTO/codegen-units=1.
Upstream ryg_rans `c9d162d996fd600315af9ae8eb89d832576cb32d`, unmodified headers.
book1: 768,771 bytes, SHA-256
`9ffa47cd93bccd732f20e0c304203cfbc1b8a91bedac536e2d8f6051003d9951`.

All codecs use the same u32 input/output representation, fixed normalized model,
reused buffers and seven median samples. Model construction, histogram and file
I/O are excluded; reset/finalization and the DC block C call are included.
Rust reads are bounded; ordinary upstream rANS inner reads are unchecked.
Synthetic/16-symbol runs use the default 262,144-symbol work target. Final book1
runs use `DC_BENCH_MIN_SYMBOLS=4194304`, i.e. five blocks per timing sample.
The old binary used one book1 block per sample; its measurements are recorded
as a before/after check, not an identical-duration statistical experiment.
Canonical timings were run without concurrent test/fuzz/benchmark jobs.

Raw data:

- Before: [synthetic](results/2026-09-13-speed-before-65536.csv),
  [book1](results/2026-09-13-speed-before-book1.csv).
- Optimized default feature set: [synthetic](results/2026-09-13-speed-default-65536.csv),
  [book1](results/2026-09-13-speed-default-book1.csv).
- With speculative encoding: [synthetic](results/2026-09-13-speed-speculative-65536.csv),
  [book1](results/2026-09-13-speed-speculative-book1.csv),
  [16-symbol blocks](results/2026-09-13-speed-speculative-16.csv).
- Separate matched 12-bit probabilities:
  [book1 with SSE4.1 baseline](results/2026-09-13-speed-speculative-book1-p12.csv).

See [build/profiling commands](README.md#branchless-speed-paths-and-profiling).
The final harness additionally rejects an unmatched profiling filter; that
bookkeeping-only change came after the canonical timings above.

## Results and limits

book1, 16-bit probability precision, ns/symbol (lower is better):

| Path | Encode | Decode | Payload bytes |
| --- | ---: | ---: | ---: |
| Previous DC4 direct encode / grouped direct decode (*) | 7.3825 | 4.1762 | 435,088 |
| Optimized DC4, direct tables, branchless decode | 5.7613 | 3.2057 | 435,088 |
| Same, speculative encoding enabled | 4.5926 | 3.2053 | 435,088 |
| Same-run upstream rANS64, 4 states | 3.2170 | 3.6691 | 435,080 |
| Same-run packed-table rANS64 adapter, 4 states | 3.2355 | 3.8706 | 435,080 |

(*) The previous encode/decode values come from separate rows selecting the
appropriate APIs; a combined old end-to-end pipeline was not timed. Compare
individual directions, not a fabricated combined pipeline measurement.

The new direct decoder's throughput is about 14% higher than this same-run
ordinary rANS64 baseline and 21% higher than the packed-table adapter. Encoding
still loses to rANS64. DC's direct decode table adds 512 KiB; ordinary rANS64
uses a 64 KiB symbol lookup plus starts/frequencies. The packed adapter uses a
512 KiB decode table as a further control, not a claim of optimal rANS design.
Direct encoding adds another 128 KiB on the DC side. These are lookup-table
policies, not total process RSS; the harness constructs multiple models at once.
For book1 the standalone `fast_block` example reports 667,136 bytes through
`Model::table_bytes()` for the combined DC model, excluding allocator/object
overhead and caller buffers.

The compact-table DC branchless path is 3.6669 ns/symbol here, roughly tied with
ordinary rANS64 but slower than the DC direct path. The gain is not solely a
new table: tables and mapping bytes are unchanged by source selection.

In the **separate probability-matched 12-bit suite**, direct DC4 branchless decode
is 3.2074 ns/symbol versus upstream SSE4.1 rANS 3.7196, with 435,636 versus
435,618 payload bytes. SIMD rANS uses a 20 KiB table and requires eight bytes of
readable padding, supplied by the harness; DC does not. This is a scoped win
over this upstream implementation, not all vectorized rANS or equal-memory
dominance. Do not mix these rows with the 16-bit suite.

Important losses:

- Uniform256, 65,536 symbols: new direct branchless decode 3.1985 versus rANS64
  2.1704 ns/symbol. Ordinary/grouped DC paths remain better choices than branchless
  for this predictable input; no automatic switch was installed.
- ramp256: direct branchless 3.1990 versus rANS64 2.6700. Grouped DC can be faster
  than branchless here. The four-state throughput win is workload-specific.
- Near-constant: branchless 3.1423 versus rANS64 2.1261. Extra unconditional work
  is wasteful when branch prediction already works.
- 16-symbol blocks: branchless decoding is around 46–51 ns/block on the tested
  sequences versus rANS64 around 23 ns/block. Small-block metadata/space may
  favor DC, but this kernel does not establish the random-access application win.
- Speculative encoding regresses predictable uniform/near-constant multi-lane
  cases by roughly 10–15% versus the optimized default in these runs. It stays
  disabled by default. The internal validated scheduling improvement is retained.

## Rejected directions and profiling

`perf record`/annotated machine code were used to inspect the actual loops.
An early long book1 run showed most combined encode/decode cycles in encoding
and remapping, consistent with the remaining slow compact encoder; it was not
a measurement of decoder-only bottleneck percentages.

Replacing variable shifts with conditional selection reintroduced unpredictable
branches: an intermediate direct decoder regressed to 4.6503 ns/symbol. Loading
one packed 64-bit word and selecting lanes with shifts also lost (3.7002 versus
the earlier mask/byte-load variant around 3.34). Applying speculative encoding
to **single-state** direct-table encoding regressed to about 10.1 ns/symbol on
uniform256; the retained feature is restricted to 4/8 states. Diagnostic CSVs:
[conditional select](results/2026-09-13-speed-rejected-select-book1.csv),
[packed window](results/2026-09-13-speed-rejected-window-book1.csv),
[single-state speculative stores](results/2026-09-13-speed-rejected-scalar-spec-65536.csv).
These intermediate sources were not separately committed and are not claimed as
independently reproducible ablations. Canonical before/after sources are versioned.

## Validation and use

- MSRV 1.88 Release, speculative Debug, all-features Release and strict Clippy pass.
- All 16 four-lane source masks, direct/compact tables, delay 16/24/32, every input
  cut in the constructed cases, and partial-output/error equality checked.
- Exact-capacity 4/8-lane encoding and untouched prefixes checked, including
  empty blocks, raw-frequency and long virtual runs. Independent scalar streams
  and the original C++ oracle remain the bitstream controls.
- C/C++ CTest: 18/18 default, 18/18 speculative, 18/18 speculative with C++
  ASan/UBSan. Miri passes both FFI tests with speculative encoding (145 seconds).
- ASan fuzzing with speculative encoding and branchless/ordinary malformed-input
  differential decoding: 30,895 executions in 61 seconds, no failure. This is
  bounded evidence, not proof of safety or corruption detection.

Run `cargo run --release --features speculative-encode --example fast_block -- FILE`
for the explicit direct-table fast path. The core and selected-branch interface
remain usable without this feature. Blitzcrank still uses historical scalar
encoding and conditional C++ decoding: these four-state kernel numbers must not
be presented as a database performance result. The next application work remains
record-level conditional decoding and a same-semantics rANS backend.
