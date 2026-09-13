# Delayed Coding / Blitzcrank execution plan

Owner: YimingQiao. Started 2026-09-12. Expected scope: 2–3 days of engineering,
with progress recorded here; this is not a claim of unattended scheduled execution.

Research objective: a usable standalone library, efficiently called by Blitzcrank,
and a real application with a defensible DC advantage over the best tested rANS
configuration. Four states are available to both sides; do not force a scalar
comparison or treat an alias implementation detail as the algorithmic contribution.

## Architecture

Ongoing speed work: an opt-in logarithmic DC derivative now has Rust/C interfaces
and independent inverse checks. It is a separate format and remains outside the
Blitzcrank compatibility path. See [the algorithm](docs/LOG_SCHEDULE.md) and
[the measured checkpoint](benchmarks/LOG_SCHEDULE.md); original DC and a new
derivative must not be conflated in speed or format claims.

Latest checkpoint (2026-09-13): [upstream baseline calibration](benchmarks/UPSTREAM_CALIBRATION.md)
supersedes earlier relative-rANS speed claims. Original programs now run separately;
shared benchmarks include extracted upstream loops and a labelled four-state
derived control. Four-state rANS64 is faster than current DC4 on book1 (2.69 vs
3.21 ns/symbol); upstream SSE8 is faster still, with a different state count.
The application-level DC advantage remains an open research objective.

`YimingQiao/delayed-coding` owns the standalone entropy coder, normalized models,
symbol lookup, bounded memory APIs, reference tests, and reproducible rANS comparisons.
`embryo-labs/Blitzcrank` owns semantic models, learning, storage, indexes and record
access. Dependency direction is Blitzcrank → delayed-coding, pinned to an exact
revision. The original research implementation remains a compatibility reference.

Implementation language: Rust (user steering). Keep the early C++ prototype outside
this repository as an unpublished reference. Use safe Rust for the codec and isolate
unsafe code in the C ABI crate. Measure per-symbol FFI overhead explicitly.

Core requirements: no database/file/JSON dependencies; immutable reusable models;
caller-owned output and reusable encoder workspace; forward symbol decoding with
per-symbol model selection; explicit 16/24/32-bit delay configurations; explicit
byte order and errors. Keep raw entropy payloads separate from container framing.
Do not promise online forward encoding: delayed coding uses a forward scheduling
pass followed by a backward embedding pass.

## Milestones and acceptance

### A — independent reference and baseline (day 1)

- [x] Standalone Rust crate, C ABI and CMake integration, minimal examples.
- [x] Fixed and changing-model Rust APIs, bounded reader/writer, model validation.
- [x] Property and boundary tests; all 65,536 code points checked for model inverses.
- [x] Bit-exact comparison against Blitzcrank main `0ed9c97` at delay 24 (1120 blocks).
- [x] Reproducible scalar and four-state delayed / upstream rANS byte / rANS64 benchmarks.
- [x] Debug, Release, Clippy, Rust/C consumer and sanitizer checks.
- [x] Personal GitHub repository with source, tests, CI and reproducibility instructions.

### B — measured optimization (day 1–2)

- [ ] Profile current loops and record baseline with CPU/compiler/commit metadata.
- [x] Compare exact reciprocal encoding against division; preserve reference oracle.
- [x] Compare compact alias and direct-table lookup, including many-model workloads.
- [x] Experiment with independent states; benchmark four-state DC against four-state rANS.
- [x] Report encoding/decoding, payload bits/symbol, table memory and setup cost together.
- [ ] Retain only changes with evidence; never generalize a scalar win to SIMD rANS.

### C — integration and adoption (day 2–3)

- [x] Thin block C interface and error contract; C caller example and test.
- [x] Fuzz harness and corrupted/truncated payload tests; retain inputs if bugs are found.
- [x] Opt-in Blitzcrank encoder adapter on an isolated branch; research default untouched.
- [x] Encoder bridge: Census table roundtrip and random seeks against legacy files.
- [ ] Migrate and measure conditional record decoding; encoder-only is not full migration.
- [ ] Format/compatibility policy; keep new container formats explicitly experimental.
- [ ] Quickstart, algorithm walkthrough and candid benchmark report.
- [ ] Tag a release only after acceptance checks; publicity text is draft only.

## Benchmark contract

Compare the same symbol sequences and normalized weights; exclude setup only in
explicitly labelled kernel measurements. Separately measure model building and
whole blocks. Cover uniform, skewed, near-deterministic, rare-symbol, real text,
small records and large blocks. Include model switching and index/record overhead
in application measurements. Report median samples, compiler flags, CPU affinity,
raw CSV, upstream rANS revision and validation. Do not use the old rANS README's
historical throughput as a present-machine comparison. Both single- and multi-state
rANS matter; SIMD and alias variants remain mandatory before broad claims.

## Working locations

- New core: `/home/yiming/projects/delayed-coding`.
- Blitzcrank integration: `/home/yiming/projects/Blitzcrank-delayed-coding`, branch
  `improve-delayed-coding`, base `0ed9c97908c51440b30a2eef3c1b90325dd2c87c`.
- Original dirty checkout `/home/yiming/projects/Blitzcrank` must be preserved.
- rANS reference: `/tmp/blitzcrank-ryg-rans`, upstream
  `c9d162d996fd600315af9ae8eb89d832576cb32d`.

## Progress

- 2026-09-13: added a bounded branchless four-state decoder, once-per-block table
  specialization, validated internal scheduling, and opt-in speculative 4/8-state
  model/event encoding. The `fast_block` example exercises the complete fast path.
  On book1, four-state direct encoding improves from 7.38 to 4.59 ns/symbol and
  decoding from 4.18 (previous group) to 3.21. Same-run rANS64 four-state decode
  is 3.67. Uniform/short records still favor other paths, and encoding still loses
  to rANS. This is a fixed-model kernel result, not the application milestone.
  See [speed report](benchmarks/SPEED_KERNELS.md).

- 2026-09-13: exact selected-branch Rust/C encoding now supports original disjoint,
  numerical and raw mappings, including four states. Blitzcrank's opt-in scalar
  encoder calls this once per block, preserving historical files. 20,000 Census
  records reconstruct exactly; payload/model/index bytes match legacy and 8,200
  shuffled/boundary record seeks pass. The decoder remains C++.
- Added capacity-planned four-state group decoding and interleaved lookahead.
  Grouping improves DC compact-table book1 decoding from 6.53 to 4.44 ns/symbol;
  its direct-table path is 4.11 versus rANS64 four-state 3.66. Keep both opt-in;
  uniform data and four-state lookahead show losses. See
  [full scope and raw experiments](benchmarks/BLITZCRANK_FOUR_STATE.md).

## Next application milestone

1. Compile semantic record operations into a reusable decode plan, keeping
   conditional model selection inside a record-level Rust/C call. First support
   categorical, equal-width and raw operations, then numerical tails. A per-field
   FFI loop is a correctness reference, not the intended high-performance design.
2. Add a same-semantics rANS backend using the same learned distributions and
   independent records. Preserve the legacy format; introduce an explicit version
   for new DC lane counts and rANS rather than silently reinterpreting files.
3. Evaluate compressed in-memory record access: encode/update cost, random-read
   median/p99 and scans versus total resident bytes (payload, indexes, models,
   padding and workspaces). Sweep record length, model count and 1/2/4/8 states
   for both algorithms; include applicable vectorized rANS controls.
4. Select the storage/latency Pareto frontier, not only matched lane counts.
   A candidate research target is 2x random-read throughput at comparable space,
   or 20–30% less resident space at comparable p99. These are acceptance targets,
   not achieved results or promises. If neither materializes, report it and
   revisit the workload/algorithm rather than promoting a synthetic win.

## Earlier progress

- 2026-09-13: implemented frequency-only `Layout` offsets/length, exact-size
  allocating encoding, opt-in single-state physical lookahead (Rust/C), and an
  exactly allocated indexed-record example. Differential tests cover layout
  invariance and lookahead behavior on valid/malformed input. See
  [scoped results](benchmarks/LAYOUT_LOOKAHEAD.md): nonuniform high-entropy
  single-state decode and short-record size wins, not a general rANS win.

- Repository boundaries and milestone acceptance specified.
- Original Blitzcrank Release build passed with GCC 12.2.
- Safe Rust core, C ABI, CMake target and examples implemented. The initial C++
  prototype is preserved outside the new repository at
  `/home/yiming/projects/delayed-coding-cpp-prototype`.
- Debug/Release, Rust 1.88 MSRV, Clippy and original-C++ differential checks pass.
- Reciprocal encoder, binary segment lookup, opt-in direct tables and 1/2/4/8
  scalar states implemented; four-state C ABI measured against four-state rANS.
- Initial benchmark report records wins and losses; no general rANS superiority claim.
- Added unmodified alias and SSE4.1 rANS comparisons, including matched-probability
  12-bit workloads and real `book1` data. Short/tail/file adapters pass ASan/UBSan.
- Flat alias slots reduce four-state compact `book1` decoding from 6.67 to 4.56
  ns/symbol in an adjacent comparison, with unchanged payload/table storage.
  Native 256-model switching regresses from 10.99 to 18.96 ns/symbol, so this
  experiment is opt-in (`flat-alias`), not the default. All results are reported.
- Published https://github.com/YimingQiao/delayed-coding; initial Rust/MSRV/C/legacy CI passed.
- 2026-09-13: libFuzzer with AddressSanitizer completed 36,946 inputs in 61 seconds
  without a crash. Miri passed the C ABI ownership/buffer test (144 seconds).
  These are bounded checks, not a proof of memory safety or corruption detection.
- Independent downstream CMake consumer passed; safety and downstream checks added to CI.
- The [conditional C++ integration status](docs/BLITZCRANK_INTEGRATION.md) tracks
  mixed alias/interval/raw-word requirements and the implemented encoder bridge.
- Pending: continued loop optimization, broader comparisons, deeper fuzzing,
  record-level conditional decoding and full end-to-end Blitzcrank migration.
