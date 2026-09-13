# Executable structure and bounded advantages — 2026-09-13

## What changed

- `Layout<DELAY, LANES>` predicts exact physical offsets/raw size using only
  realized frequencies. It is also the encoder's actual scheduling implementation.
- Allocating `encode` requests exactly the payload length, without a 2*n payload
  buffer or a final payload move. Its n-byte schedule workspace is not eliminated.
- `decode_lookahead_into` / `dc_decode_lookahead` predecode one physical word using
  a fixed model. No additional model memory or dynamic allocation; same payload
  and validation as the serial decoder. The default decoder is unchanged.
- `indexed_records` demonstrates exact arena allocation and independent reads in
  shuffled order, with a shared model and a u32 offset index. It is an in-memory
  example, not a serialized container or completed Blitzcrank integration.

These are implementation results, not a claim of global algorithmic novelty.

## Single-state decode: a nonuniform high-entropy win

Workload `ramp256`: frequencies f[i] = 2*i+1, i=0..255, total 65536; 65,536
symbols sampled with the harness's deterministic MT19937 seed 123456. This is
not equal-probability bit packing. Every codec gets the identical input and
frequencies. Results below come from `2026-09-13-lookahead-final-65536.csv`.

| Codec | States | Payload bytes | Decode ns/symbol |
| --- | ---: | ---: | ---: |
| DC24 existing compact | 1 | 63,256 | 5.3800 |
| DC24 physical lookahead, compact | 1 | 63,256 | 5.0142 |
| rANS64 upstream-header baseline | 1 | 63,256 | 7.0638 |
| byte-rANS upstream-header baseline | 1 | 63,254 | 7.5247 |
| rANS64 packed-table adapter | 1 | 63,256 | 8.3989 |
| byte-rANS packed-table adapter | 1 | 63,254 | 9.0080 |
| rANS64 upstream-header baseline | 4 | 63,276 | 2.7641 |
| DC24 direct tables | 4 | 63,262 | 3.2775 |

The one-state lookahead decoder is about 1.41x the throughput of one-state
rANS64 here, at the same payload length. Most of that advantage already exists
in the old DC single-state kernel; lookahead itself reduces DC time by about 6.8%.
Do not attribute the whole 1.41x to this patch. Four-state rANS64 is faster than
both single-state DC and the tested four-state DC configurations.

The packed rANS adapters perform one 512 KiB lookup yielding symbol, frequency
and remainder, then apply the same rANS update/unchanged upstream renormalizer.
They remove a dependent symbol-to-parameters lookup, but are slower on this
machine. They are additional controls, not claimed optimal rANS implementations.
The compact winning DC path does not need that 512 KiB table. Symbol mappings
differ between DC and contiguous rANS, so these are implementation comparisons,
not an isolated causal proof about normalization. Alias is not the claimed novelty.

On full `book1`, compact DC24 goes from 8.0756 to 7.1311 ns/symbol with lookahead
(11.7% less time). Single-state rANS64 is 7.1960: treat that as approximately a
tie, not a meaningful real-text win. DC emits 435,080 bytes; rANS64 emits 435,060.
Four-state rANS64 remains much faster at 3.7032 ns/symbol. DC compact encoding is
17.55 ns/symbol versus rANS64's 3.79: this patch does not close the encoding gap.

## Small independent records: a storage win

Split the same 768,771-byte `book1` into fixed-width independent records, with
one short final record. Reuse one model built from the complete input. Reset the
coder for every record. Decode/compare every record. All rANS state flush bytes
are counted. Add the same 4*(record_count+1)-byte offset index for every codec.
Shared model, original length, width, checksum and file framing are external.

| 16 input bytes/record; 48,049 records | Raw payload | Offset index | Total bytes |
| --- | ---: | ---: | ---: |
| DC16 | 559,644 | 192,200 | 751,844 |
| DC24 | 598,496 | 192,200 | 790,696 |
| byte-rANS | 603,319 | 192,200 | 795,519 |
| rANS64 | 760,956 | 192,200 | 953,156 |

DC16 uses 5.5% less total storage than byte-rANS and 21.1% less than rANS64 in
this workload. This is an exact size result, not a timing result. The runnable
Rust `indexed_records` example independently reproduces the DC totals and
verifies all 48,049 records in shuffled order.

The useful setting is many independently accessible short records sharing a
model; it does not require pretending rANS cannot support random access. These
are chopped text bytes, not real database tuples or a Blitzcrank end-to-end test.
Specialized rANS termination/container designs may reduce its overhead further.

The tradeoff reverses as blocks grow: at 4096 bytes/record, DC16 plus index uses
442,358 bytes, byte-rANS 436,467, and DC24 436,476. The lower delay reduces short
record overhead but increases finite-precision redundancy. Do not make DC16 the
unconditional default. At 8-byte records all measured totals including indexes
exceed the original file size; a relative codec win is not necessarily compression
versus storing the bytes directly.

## Negative results and scope

The first implementation predecoded batches of 1/8/32/128 physical words into
an on-stack buffer. Most rows were unchanged or slower; retain this experiment
only in `examples/physical_lookahead.rs`. One-word lookahead keeps a pending
entry without that batch machinery and performed better on text/skewed inputs.
Near-constant and some uniform cases do not improve, so lookahead is opt-in.

This exploits a source-level dependency separation; no hardware-counter/assembly
study here establishes which microarchitectural effect caused the speed changes.
All timings are warm fixed-model kernels, not cold-start or conditional-model
results. Conditional next-model dependencies prevent using this API unchanged.
The general case still has a serial capacity chain, and virtual words still
depend on the information-state chain. There is no universal rANS/SIMD win.

## Reproduction and provenance

Implementation is in the commit containing this report, based on parent
`1a33e6a666ba9e575f3e7712fa2e57145d7b14c4`. Canonical result files:

- `results/2026-09-13-lookahead-final-65536.csv`
- `results/2026-09-13-lookahead-final-book1.csv`
- `results/2026-09-13-record-sizes-book1.csv`

The earlier `lookahead-rans-*` CSVs are pre-Layout-refactor runs of the same new
decoder/harness; the later final files repeat their findings. The
`physical-lookahead.csv` file is the preliminary native Rust batch-size probe,
before moving its one-word function into the library. Its random generator,
normalization and call boundary differ from the C++ harness: do not compare
native and C++ timing rows as an ablation. The new ramp distribution also changes
the RNG position of later synthetic distributions relative to historical CSVs.

Machine: Xeon Platinum 8474C, Linux x86-64, logical CPU 2. Rust 1.92.0, GCC 12.2.0;
CMake Release, Rust thin LTO/one codegen unit, no native CPU flags. Median of seven
samples per metric with max(1,262144/n) repetitions/sample. Both sides use u32 input/output;
DC has a C ABI boundary and bounded reads, upstream rANS inner reads are unchecked.
Models and output buffers are reused; model building and serialization excluded.
Benchmark rows include encoder time and size, not only the winning decoder time.

Upstream headers remain unmodified at
`c9d162d996fd600315af9ae8eb89d832576cb32d`. `book1` is from that checkout (not
redistributed), SHA-256:
`9ffa47cd93bccd732f20e0c304203cfbc1b8a91bedac536e2d8f6051003d9951`.

```sh
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release \
  -DDELAYED_CODING_RANS_DIR=/path/to/ryg_rans
cmake --build build -j2
ctest --test-dir build --output-on-failure
taskset -c 2 build/compare_rans 65536
taskset -c 2 build/compare_rans --file /path/to/ryg_rans/book1
taskset -c 2 build/compare_rans --records /path/to/ryg_rans/book1
cargo run --release --example indexed_records -- /path/to/ryg_rans/book1 16
cargo run --release --example physical_lookahead -- 65536 /path/to/ryg_rans/book1
```

The main table is the 16-bit probability suite, not the separate SSE4.1/12-bit
suite. The latter also builds and passes tail/file correctness checks; no new
SIMD speed claim is made here. Reported timing ratios are exploratory on one
machine and one seed; broader input/core/compiler measurements remain work to do.

## Validation

Default Debug/Release and all-feature Release tests pass, as do Rust 1.88 MSRV,
formatting and all-target/all-feature Clippy. New tests check exact offsets against
the actual byte cursor, frequency-only/prefix invariance, overflow/error atomicity,
exact allocating output capacity, and lookahead differential behavior on valid,
truncated, trailing and arbitrary malformed input. The original C++ D24 oracle
still agrees byte-for-byte. Release CTest passes all 17 checks; the ASan/UBSan C++
configuration passes all 16 of its checks (including packed rANS and short records).

The extended libFuzzer harness compares serial/lookahead errors and partial output,
as well as valid roundtrips. With nightly-2026-09-13 and libFuzzer seed 3159929567,
it completed 32,736 executions in 61 seconds without a crash or differential
failure (`-max_total_time=60 -max_len=4096`; starting from the existing local corpus).
This bounded fuzz run is not exhaustive and is not an integrity guarantee.
The extended FFI ownership/buffer test also passes Miri (144.60 seconds).
