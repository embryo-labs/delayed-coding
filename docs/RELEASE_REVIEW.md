# Standalone preview review — 2026-09-14

This is a maintainer-style source review and regression exercise, **not** an
independent security audit, a proof of optimal performance, or a production
certification. Portable defaults are retained; workload-sensitive variants
remain explicit. The main README reports only this project's own performance.

## Scope and findings

Reviewed the shipping Rust core (`src/`), C ABI (`ffi/src/`), promoted SIMD
companion (`simd/src/`), their entry-point contracts, tests, examples, Cargo/CMake
integration and CI. The original SIMD prototype remains an explicitly historical
experiment, separate from the promoted implementation. Historical comparison
reports are retained as evidence, not presented as current release claims.

- Core alias construction: normalized sums, inactive symbols, interval coverage,
  inverse mapping and reciprocal arithmetic bounds. Packed prepared models
  preserve the mapping for every 16-bit codeword; larger tables are opt-in.
- Prepared DC16 state bounds: normalized capacities below 2^16, frequencies at
  most 2^16; valid states fit u32. Slice lengths, lane configuration, tails and
  final-state checks inspected. Malformed streams cannot be treated as
  authenticated merely because decoding returns success.
- FFI: handle ownership, lifetime/alignment assumptions, exclusive workspaces,
  disjoint buffers, panic containment. Added consistent metadata/handle alignment
  checks before dereferences. These cannot validate arbitrary foreign pointers;
  the documented live-allocation/aliasing contract remains the caller's duty.
- SIMD: ISA guards; masked physical-word counts before loads; table indices
  bounded by the 16-bit word; exact output capacity before stores; scalar tails;
  normalized numerator/capacity check before vector multiplication. Unsafe code
  is isolated outside the core and annotated with the required bounds.
- Promoted scalar fallback reuses the compact mask schedule and caller output,
  rather than allocating another payload and a byte flag per symbol. Added a
  forced-scalar model with no direct LUTs for differential tests and memory budgets.
- Cumulative/64 payloads are explicitly distinct from core alias/1/4 payloads.
  No CPU-specific file marker, silent precision reduction or default SIMD switch.
- Rust 1.88 remains the portable MSRV; AVX-512 uses stabilized intrinsics requiring
  1.89+. CI separates those feature sets and adds a native ARM fallback job.

## Local verification completed

- Workspace default Debug and all-feature Release tests; strict all-target
  all-feature Clippy and formatting. Portable Rust 1.88 all-target workspace check.
- Core tests cover exhaustive short inputs, all code points, random/mixed models,
  reciprocal boundaries, raw/selected branches, sticky errors and truncation.
- SIMD tests execute on an AVX-512-capable Xeon: scalar/automatic byte identity,
  16/32/64-state diagnostics, zero/constant/sparse models, unaligned/exact buffers,
  short tails, corrupted/truncated streams and all reciprocal frequencies.
- AddressSanitizer: SIMD tests with `nightly-2026-09-13`, `-Zsanitizer=address`,
  release build, explicit x86_64 target; all five tests passed.
- Miri: both ordinary C ABI tests and the optional logarithmic C ABI test passed.
- CMake Release: C roundtrip, legacy selected-branch differential and legacy
  symbol-stream differential passed against pristine Blitzcrank
  `0ed9c97908c51440b30a2eef3c1b90325dd2c87c`.
- [Current SIMD measurements](../benchmarks/results/2026-09-14-simd-release.json):
  prebuilt-model full-precision DC16/64 on book1, three process runs, no concurrent
  agent benchmark/build/test. Not a whole-application or universal speed claim.

## Release boundaries

No unchecked-input API is introduced. All raw payloads still require external
model/configuration/count metadata and application checksums/resource budgets.
Allocator exhaustion may abort. Default and SIMD paths are not constant-time.
Short-record and many-model performance may favor scalar coding; wide tables and
many states are not made default on the strength of one benchmark.

The organizational repository retains history; the personal repository is not
deleted, transferred or force-pushed. Publication uses a reviewed commit,
followed by a clean-checkout build and GitHub CI inspection. Any subsequent
source correction must be reviewed and retested before the preview is finalized.
