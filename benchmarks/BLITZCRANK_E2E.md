# Blitzcrank full-data end-to-end checkpoint

Date: 2026-09-14. This measures the **existing single-state delay-24 encoder
bridge**, not the independent AVX-512 prototype. Both builds retain the C++
decoder. It is DC versus DC, not DC versus rANS, and is a tabular CLI test,
not an OLTP database transaction benchmark.

## Source, data and timing contract

- Blitzcrank: `903fdde93cde71e80eee5f519daf17f9abb6742d`, clean checkout at
  `/home/yiming/projects/Blitzcrank-delayed-coding`. Same-source CMake Release
  builds with `BLITZCRANK_RUST_ENCODER=OFF` and `ON`; these are not a new run
  against the unmodified historical legacy revision.
- Rust dependency: `7e6c5a2b216967682ad33b045cd277f2dea549a1`. Core/FFI/include
  paths pass the integration's pinned-source check. Separate, uncommitted
  AVX-512 experimental files are not compiled into either application.
- Intel Xeon Platinum 8474C, GCC 12.2.0, Rust 1.92.0, Linux 6.1.0-45-amd64.
  C++ `-O3 -DNDEBUG`. All timed application processes pinned to logical CPU 2,
  single-threaded; sibling CPU 98 and the rest of the shared host not isolated.
- Full USCensus1990: **2,458,285 rows, 69 fields, 358,885,350 input bytes**.
  CSV SHA-256 `7832d5412b8304ae23b697bcf7226a073326a672ef9b187117579bfe0eae6021`;
  config SHA-256 `d72d29c8a7f43103d746991edce72a60c219352906735f83408c6bea9990d744`.
- Comma delimiter, `skip_learning=1`: skips structure search, **not** frequency
  fitting. The actual model-learning pass still runs over the data. This does
  not characterize learned cross-column dependency workloads.
- Three runs per backend/configuration, order C++/Rust, Rust/C++, C++/Rust.
  File input was already read before timing; no cache flushes. Output closes
  normally but is not `fsync`ed. These are page-cache-backed file operations,
  not durable/cold-storage throughput.
- Full `-c`/`-d` time is external process wall time, including setup, input,
  output and teardown; compression includes CSV parsing and model fitting,
  decompression includes CSV string conversion and writing. Each output is
  checked using `cmp` outside the timed region.
- `-b` application timers separately exclude parsing, training and decoder
  initialization. Encoding includes field-to-interval work and payload output;
  decoding reconstructs typed rows in memory, without formatting/writing CSV.
  Neither timer measures just the entropy kernel. The CLI truncates input MiB
  before printing throughput, so reported throughput below is recomputed from
  exact input bytes. MB/s means decimal 1,000,000 bytes/s.
- Block threshold is the count of probability intervals, checked after each
  tuple, **not** a row count or a SIMD lane count. Threshold 1 gives single-row
  blocks here; threshold 20,000 gives roughly 286 rows/block.

## File-to-file results

Medians; [all wall/user/system/RSS samples](results/2026-09-14-blitzcrank-e2e.csv).

| Threshold | Backend | Compress seconds | Compress MB/s | Decompress seconds | Decompress MB/s |
| --- | --- | ---: | ---: | ---: | ---: |
| 20,000 | C++ DC | 16.34 | 21.96 | 8.41 | 42.67 |
| 20,000 | Rust DC bridge | 15.92 | 22.54 | 8.46 | 42.42 |
| 1 | C++ DC | 16.26 | 22.07 | 8.42 | 42.62 |
| 1 | Rust DC bridge | 16.07 | 22.33 | 8.51 | 42.17 |

At threshold 20,000 the bridge reduces compression time by **2.57%**;
threshold 1 gives **1.17%**. Full-file decoding shows no gain. Its small
differences (0.6–1.1% median increases) should not be overinterpreted with only
three runs on a shared host. Compression maximum RSS is about **6.4 GiB**:
the CLI materializes the full dataset as attribute objects. This is not the
entropy coder's working-set requirement. Decoder maximum RSS is roughly
36–37 MiB for large blocks and 79 MiB for single-row blocks.

## In-memory application work and random access

[Raw application-timer samples](results/2026-09-14-blitzcrank-internal.csv).
Bulk results use threshold 20,000. These medians include semantic field/model
dispatch, not only entropy coding:

| Phase | C++ seconds | Rust-bridge seconds | C++ MB/s | Rust-bridge MB/s |
| --- | ---: | ---: | ---: | ---: |
| Encode already-parsed rows | 4.24491 | 3.86515 | 84.54 | 92.85 |
| Decode to typed rows | 2.81543 | 2.92442 | 127.47 | 122.72 |

Encoding time falls **8.95%** but retained-C++ decoding time rises **3.87%**.
The decoder regression is not explained causally by this timing experiment;
bridge-dependent object layout/code generation remain possible factors.

For threshold 1, the CLI issues 300,000 uniformly selected record queries per
run with deterministic `mt19937(0)`, after loading payload/model/index. Each
query includes index lookup, state reset and typed-row reconstruction; setup,
CSV formatting and application transactions are excluded. Median of three
per-run **mean** latencies: **1.53797 us/row C++**, **1.56010 us/row Rust build**.
This is about 650k versus 641k rows/s, not a p50/p99 latency distribution, and
does not show a random-access improvement. Query results are not individually
compared inside this timed loop; the separate seek regression checks results.

The gap between the internal stages (3.87/2.92 seconds) and complete CSV
commands (15.92/8.46 seconds) demonstrates substantial work outside those
stages. These are separate runs, so their difference is an approximate overhead
estimate rather than a precisely instrumented phase breakdown.

## Size and correctness

Both backends produce identical bytes for all three artifacts on every timed
full-data run; every full-data CSV decode is byte-identical to the source.

| Threshold | Payload including model | Enumeration sidecar | Index | Total bytes | Input/total |
| --- | ---: | ---: | ---: | ---: | ---: |
| 20,000 | 31,031,819 | 935 | 34,388 | 31,067,142 | 11.55× |
| 1 | 40,725,009 | 935 | 9,833,148 | 50,559,092 | 7.10× |

The first-20,000-row regression also passed at both thresholds: exact restored
CSV, identical payload/model/index, and 8,200 shuffled/boundary record seeks
with all fields verified across the two backends and two configurations.

## Hotspot evidence and next engineering priorities

One additional Rust-build run per file-to-file command used `perf record -e
cycles:u -F 199`, separately from the timing samples above. No lost samples
were reported. These are **self sample percentages**, not inclusive call-tree
costs or wall-time fractions; there was no call-graph collection. Inlining and
unresolved symbols limit attribution. Selected hotspots:

| Command | Symbol | Self sample share |
| --- | --- | ---: |
| Compress | `__memcmp_evex_movbe` | 23.14% |
| Compress | Rust `Branch::split` | 13.44% |
| Compress | `EnumTranslate` | 7.92% |
| Compress | `std::getline` | 6.95% |
| Compress | Rust `encode_branches_into` | 5.78% |
| Compress | `LoadDataSet` | 5.22% |
| Compress | `TableCategorical::FeedAttrs` | 5.14% |
| Decompress | `CategoricalSquID::Decompress` | 33.97% |
| Decompress | string `_M_assign` | 15.45% |
| Decompress | `std::ostream::put` | 7.12% |
| Decompress | `std::__ostream_insert` | 6.10% |
| Decompress | filebuf `xsputn` | 4.83% |

This supports working on **both** application data movement and the coding
path, not describing the present application as hardware-limited. `memcmp`
caller attribution needs a call-graph experiment before assigning all its
cost to enum lookup. In particular:

1. Separate a reusable typed-record/batch API from CSV import/export and
   model fitting; reduce the full-table object materialization cost.
2. Investigate branch splitting and field-to-branch preparation on the actual
   application path. The fast fixed-model SIMD kernel does not replace this
   work automatically.
3. Add a versioned SIMD-capable integration with conditional-model semantics,
   explicit block/state layout, fallback and record-seek tests before reporting
   an AVX-512 end-to-end result. This checkpoint makes no such integration change.
4. Compare an actual rANS application backend under the same model, record
   boundaries, index, output semantics and hardware before claiming superiority.

## Reproduction

Retained scratch: `/tmp/blitzcrank-e2e.1VuAHO`. `run.sh` records full-file
times, checks reconstructions and compares compressed artifacts; `internal.sh`
records built-in bulk/random-access times; `profile.sh` samples the Rust build.
Per-run logs, sidecars, reconstructed data and time files are retained there.
Do not run CLI benchmarks in a directory with valuable `_enum.dat` or
`_temp.index`: the historical CLI uses fixed sidecar names and `-b` deletes them.

Rebuild both variants from the source above. In a **new dedicated scratch
directory**, place `input.csv` and `input.config`, create separate backend/mode
subdirectories and run these commands inside the relevant subdirectory:

```sh
# PROGRAM is the chosen absolute executable; BLOCK is 1 or 20000.
/usr/bin/time -f '%e,%U,%S,%M' -o encode.time taskset -c 2 "$PROGRAM" -c ../input.csv payload.bin ../input.config 0 1 "$BLOCK"
/usr/bin/time -f '%e,%U,%S,%M' -o decode.time taskset -c 2 "$PROGRAM" -d payload.bin restored.csv ../input.config 0 "$BLOCK"
cmp ../input.csv restored.csv
sha256sum payload.bin _enum.dat _temp.index
# Run these in separate directories from retained file-to-file artifacts.
taskset -c 2 "$PROGRAM" -b ../input.csv ../input.config 0 1 20000
taskset -c 2 "$PROGRAM" -ra ../input.csv ../input.config 0 1 1
```

Executable SHA-256:

- OFF (`/tmp/blitzcrank-bridge-default/tabular_blitzcrank`):
  `a099892b0be401caa1387cfcf8466b214e9c22430240644a0e3266aa9965baeb`.
- ON (`/tmp/blitzcrank-rust-encoder/tabular_blitzcrank`):
  `f5486a2f7723ab8a8118e2f228de633cfc434088be6449e570432c87439f7594`.
