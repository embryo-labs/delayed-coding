# Contributing

Core Rust code must remain safe by default and dependency-free. Isolate C ABI
unsafe code in `ffi/` and CPU intrinsics in the optional `simd/` companion,
document pointer ownership and prevent Rust panics from
unwinding through C. Do not add unchecked decoder reads to the default interface.

Before submitting a change:

Use Rust 1.89+ for all-feature SIMD checks. The portable workspace also supports
Rust 1.88; test that separately without enabling the `avx512` feature.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --release
cargo test --features reference-division
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build -j
ctest --test-dir build --output-on-failure
```

Changes touching decoder bounds or the C ABI should also run the
[fuzz and Miri checks](fuzz/README.md). The separate fuzz package keeps development
dependencies out of the runtime crate. CI includes a short fuzz smoke test; longer
local runs are useful before releases.

Algorithm changes should preserve golden/reference behavior or explicitly change
the documented experimental format. Include an independent oracle or property
that would fail for the original bug. Never silently modify the original
Blitzcrank checkout to make differential tests pass.

Performance PRs should explain the bottleneck and report both speed and payload
size on affected workloads, including regressions. Use identical compiler/CPU
settings before and after. Large tables need memory and model-switching evidence;
interleaving needs small-block overhead and equivalent-lane rANS comparisons.

For a bug report, include a minimal model and input (or minimized malformed
payload), delay, lanes, commit and observed error. For a benchmark, also include
CPU, compiler versions, build flags and the exact command. Please avoid attaching
private datasets; a synthetic reproducer is preferable.
