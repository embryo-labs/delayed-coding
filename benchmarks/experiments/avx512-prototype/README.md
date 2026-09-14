# Exact DC16 / AVX-512 prototype

Independent, unpublished Rust research crate. Not a root workspace dependency,
not an enabled C ABI path, and not a replacement for existing Blitzcrank files.
Read the [measurements and hardware-limit analysis](../../AVX512.md).

The optimized pair uses exact capacity arithmetic at delay 16, cumulative
intervals, 64 interleaved 32-bit states and big-endian 16-bit physical words.
It is **not Log-DC**. The smaller delay changes compression ratio relative to
delay 24. Cumulative and alias mappings are separate raw formats; lane count is
also external metadata. There is no framing, checksum or model serialization.

## Rust use

Requires Rust 1.89+ for the intrinsics (tested with 1.92). ISA support is checked
at runtime: AVX2, AVX-512F/BW/VL/VBMI2 and POPCNT; other machines use the scalar
fallback. The benchmark executable requires the SIMD-capable machine explicitly.
No third-party dependencies, only the local `delayed-coding` crate.

```rust
use dc_avx512_prototype::{EncodeWorkspace, SimdModel};

let model = SimdModel::new_cumulative(&[4096; 16])?;
let input: Vec<u32> = (0..4096).map(|i| i % 16).collect();
let mut bytes = vec![0; input.len() * 2];
let mut workspace = EncodeWorkspace::default();
let occupied = model.encode64_into(&input, &mut bytes, &mut workspace)?;
let mut output = vec![0; input.len()];
model.decode64_bounded(&bytes[occupied], &mut output)?;
assert_eq!(input, output);
# Ok::<(), delayed_coding::Error>(())
```

`SimdModel::new` instead preserves the original alias mapping; its independent
scalar encoder supports up to 64 lanes. The SIMD encoder currently accepts only
cumulative models. `decode16`, `decode32`, and `decode64` are window-reading
controls. Prefer `decode64_bounded` for the 64-lane path: it remains vectorized
when the remaining virtual symbols require no physical input.

The encoder includes validation, a forward capacity/schedule pass and backward
embedding. Reused masks consume ceil(symbols/16)*2 bytes. Exact output capacity
is checked before any write. Decoder input needs no padding. Errors may leave
a decoded prefix; successful final-state/consumption checks are not a checksum.
Unsafe intrinsics are contained in `encode.rs` and the `x86` module, with private
validated lookup tables and checked buffers. This is not a release-level audit.

## Run

From the repository root:

```sh
cargo test --release --manifest-path benchmarks/experiments/avx512-prototype/Cargo.toml
cargo build --release --manifest-path benchmarks/experiments/avx512-prototype/Cargo.toml
taskset -c 2 benchmarks/experiments/avx512-prototype/target/release/dc-avx512-prototype /tmp/ryg_rans/book1 12
```

Second argument is effective probability precision, 12 or 16. For synthetic
input, use `synthetic:uniform256`, `synthetic:skewed`, `synthetic:near_constant`,
or `synthetic:constant`; a third argument sets symbol count (default 65536).

`DC_BENCH_MIN_SYMBOLS` changes sample duration; default 16777216. The implementation
uses floor(minimum/block_length) complete blocks, at least one, with seven samples.
`DC_BENCH_CODEC` selects a CSV codec name, `store_floor`, or `copy_floor`.
Each CSV time is per symbol, with allocation, model building and I/O excluded.
Scalar-oracle rows are validation controls, not optimized scalar baselines.

For perf counters, create two FIFOs in a new temporary directory, pass their paths
as `DC_PERF_CTL` and `DC_PERF_ACK`, and run `perf stat -D -1 --control=fifo:CTL,ACK`.
Select exactly one codec. The program enables counters only around measured
samples and prints the exact counted symbol total. Full commands are preserved
in [the counter log](../../results/2026-09-14-avx512-counters.txt).

The [assembly snapshot](analysis/decode-p12.s) is a diagnostic artifact, not linked
code. Its labels were simplified for LLVM-MCA. Re-extract it after kernel changes;
do not assume its instruction model remains valid for another compiler or CPU.
