# Delayed Coding / Blitzcrank execution plan

Owner: YimingQiao. Started 2026-09-12. Expected scope: 2–3 days of engineering,
with progress recorded here; this is not a claim of unattended scheduled execution.

## Architecture

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
- [ ] Compare exact reciprocal encoding against division; preserve reference oracle.
- [ ] Compare compact alias and direct-table lookup, including many-model workloads.
- [ ] Experiment with 2/4 independent states; compare like-for-like rANS configurations.
- [ ] Report encoding/decoding, payload bits/symbol, table memory and setup cost together.
- [ ] Retain only changes with evidence; never generalize a scalar win to SIMD rANS.

### C — integration and adoption (day 2–3)

- [x] Thin block C interface and error contract; C caller example and test.
- [x] Fuzz harness and corrupted/truncated payload tests; retain inputs if bugs are found.
- [ ] Blitzcrank dependency adapter on an isolated branch; default research checkout untouched.
- [ ] End-to-end table/record roundtrip and random-access checks after migration.
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
- The [conditional C++ integration design](docs/BLITZCRANK_INTEGRATION.md) identifies
  mixed alias/interval/raw-word requirements; a block-only link is not a migration.
- Pending: continued loop optimization, broader comparisons, fuzzing, explicit
  per-symbol/conditional C++ integration costs and end-to-end Blitzcrank migration.
