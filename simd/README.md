# Optional DC SIMD block codec

This companion isolates intrinsics from the safe portable core. It is an explicit
bulk-throughput option, not a universal replacement for single-record DC.

Default builds use scalar code (Rust 1.88+). Feature `avx512` requires Rust 1.89+
and runtime AVX2, AVX512F/BW/VL/VBMI2 and POPCNT. Unsupported CPUs always fall
back. No global CPU flags are required; scalar and accelerated files are identical.

```rust
use delayed_coding_simd::{SimdModel, EncodeWorkspace};
let model = SimdModel::new_cumulative(&[32768, 32768])?;
let symbols = vec![0, 1, 0, 0, 1];
let mut bytes = vec![0; symbols.len() * 2];
let range = model.encode64_into(&symbols, &mut bytes, &mut EncodeWorkspace::default())?;
let mut restored = vec![0; symbols.len()];
model.decode64_bounded(&bytes[range], &mut restored)?;
assert_eq!(restored, symbols);
# Ok::<(), delayed_coding::Error>(())
```

`new_cumulative_scalar` forces portable execution without direct decode tables;
it is useful for memory budgets and differential checks. `backend()` reports the
selected backend. `memory_bytes()` estimates owned heap, not whole-process RSS.

## Contract and costs

- At most 256 symbol IDs; frequencies sum to 65,536, including zero-frequency IDs.
- No implicit quantization. Frequencies all divisible by 16 allow an exact
  16 KiB packed table; otherwise accelerated decoding uses 512 KiB in two tables.
  The object also retains scalar-model and frequency metadata.
- `encode64_into` / `decode64_bounded`: delay 16, 64 round-robin states,
  cumulative mapping, big-endian 16-bit physical words. This is **not** the
  core alias payload or its 1/4-state format. Store that metadata and symbol count.
- Encoding reuses one u16 scheduling mask per 16 symbols. Output needs at most
  two bytes per symbol. Validation/output-capacity checks precede output writes.
- Decoding reads only the exact supplied slice; no padding or overread contract.
  Output can contain a decoded prefix on failure. Final states are not a checksum.
- 64 states incur startup/space overhead on short blocks. Model preparation,
  extra tables and CPU frequency effects can negate throughput gains. Not default.
- Alias/16-/32-state methods remain diagnostic interfaces; the Blitzcrank bulk
  integration uses cumulative/64 only. This companion has no C ABI yet.

## Validation

Run both `cargo test -p delayed-coding-simd --release` and
`cargo test -p delayed-coding-simd --features avx512 --release`. Tests cover
scalar/accelerated byte identity, exact and unaligned buffers, tails, constant
and sparse models, all reciprocal frequencies and malformed payloads.
AVX-512 execution must additionally be tested on capable hardware; ordinary CI
may exercise only the fallback. See [release review](../docs/RELEASE_REVIEW.md).
