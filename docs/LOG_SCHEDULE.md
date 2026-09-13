# Conservative logarithmic Delayed Coding (experimental)

This is an opt-in **DC derivative and separate raw format**, not a compatible
replacement for original Blitzcrank/DC payloads. Its measured purpose is a faster
fixed-model entropy kernel. No claim of research priority is made here.

## Schedule and inverse

Let the radix be M = 65536, fixed-point scale Q = 256, threshold T = 24Q = 6144,
and normalization decrement R = 16Q + 2 = 4098. Each lane starts with integer
budget b = 0. For each realized frequency f (1..65536):

```
virtual = (b >= T)
b = b - virtual*R + lower_log(f)
```

`lower_log(f)` is an integer lower bound on Q*log2(f), computed using eight
rounds of integer squaring and truncation. No floating-point operation is used
to construct models or encode/decode. The physical layout therefore depends
only on realized frequencies, independently of the information numerator.
The implementation keeps one schedule byte per symbol in a reusable Workspace.

Symbols occupy cumulative contiguous intervals `[start, start+f)`; unlike the
ordinary Model, this model does not preserve Blitzcrank's alias mapping. For a
backward information state n, let q = floor(n/f) and r = n-q*f. The codeword is
`w = start+r`. A physical event writes w and keeps q; a virtual event keeps
`q*M+w`. Encoding uses the equivalent expression

```
embedded = n + q*(M-f) + start
w = embedded modulo M
n = virtual ? embedded : q
```

This avoids variable shifts and masks in the numerator update. Its arithmetic
resembles rANS's update; the distinction is the frequency-only schedule choosing
physical versus virtual events, rather than rANS information-state normalization.
Decoding selects the next physical word or the low 16 numerator bits, divides
the numerator by M in the latter case, then updates `n = n*f + (w-start)`.

## Why the approximation is reversible

Associate a proof capacity C(b) = ceil(2^(b/Q)) with each budget. This capacity
is not evaluated by the implementation. Initially C(0) = 1.

1. Multiplication is conservative:
   `C(b+lower_log(f)) <= f*C(b)`.
2. On a virtual event, b >= 24Q. Write x = 2^(b/Q-16), so x >= 256. The extra
   decrement 2 ensures `x*2^(-1/128) <= x-1 <= floor(x)`, hence
   `C(b-R) <= floor(C(b)/M)`. One elementary justification is Bernoulli's
   inequality: `(255/256)^128 > 1/2`, so `2^(-1/128) < 255/256`.
3. Work backwards from terminal numerator zero. If the next numerator is below
   its capacity, q is below the normalized predecessor capacity. A physical
   predecessor q is therefore below C(b); a virtual predecessor `q*M+w` is
   also below C(b), because 0 <= w < M and the normalized capacity is at most
   floor(C(b)/M).

Induction gives initial numerator < C(0) = 1, hence zero: no initial numerator
header is needed. Budgets stay in [0,40Q), so valid numerators are below 2^40 and
normalized quotients below 2^24. Existing exact reciprocal division is sufficient;
the `reference-division` feature selects integer division as an oracle.

The proof needs a *lower bound*, not an assumption that a floating log rounds
correctly. Normalizing f into [2^31,2^32), repeatedly squaring and truncating
always rounds downward; all products fit u64. Tests check all 65536 frequencies
against floating-point logs as a numerical sanity check, not as the proof.

## Rate cost and format contract

The frequencies and symbol probabilities do not change, but conservative
scheduling adds a small coding-rate cost. For N symbols, total integer logs S,
total virtual count V, and summed final budgets B, `S-R*V=B`. Thus:

```
payload_bits - ideal_cross_entropy
  = sum(log2(f) - lower_log(f)/Q) + (2/R)*(S/Q) + 16*B/R
```

All observed per-frequency log errors are below 1.001/Q bits. This gives a
numerically checked bound below about `0.01172*N + 40*lanes` extra bits; the
algebraic expression above is the exact bound in terms of the actual log errors.
Report actual payload size beside every speed result. Do not market unchanged
probabilities as unchanged compression ratio.

Payloads consist of big-endian 16-bit physical words, with lanes assigned round
robin. Model, lane count, symbol count and an explicit **log-DC format identity**
must be supplied externally. There is no model serialization or framing here.
There are no input-padding requirements. Reads are bounded, and input consumption
and final zero states are checked; these checks are not a checksum. Malformed
numerator arithmetic wraps, as in the ordinary decoder, without trusting input.

For <=256 IDs, the model uses a 64 KiB u8 lookup plus small immutable symbol/log
tables (72192 allocated table bytes for 256 IDs). Larger alphabets use a 512 KiB
packed direct lookup plus symbol tables. Padded byte-table accesses mask IDs;
high bits and absent-symbol markers are rejected before any payload writes.
The Rust core still forbids unsafe code. The failed AVX2 prototype is archived
outside the library and is not a dependency or enabled path.

## Independent checks and a deferred property

Tests include exact-capacity output, untouched prefixes, all supported lane counts,
wide and sparse alphabets, malformed input, truncation and invalid parameters.
An independent scalar decoder finds symbols by interval search instead of using
the optimized lookup tables. A second encoder reconstructs schedules backwards
from per-lane log sums and uses actual division, checking identical payload bytes.

The reverse schedule follows from its additive recurrence. After a prefix log sum
S, a normalized budget is `S` if S<T, otherwise
`(T-R) + ((S-(T-R)) mod R)`. This can eliminate the per-symbol workspace, but the
tested reverse encoder was slower and is retained only as a test oracle. This is
a potential memory/parallel-layout direction, not a measured application win.

The speed change combines logarithmic scheduling, contiguous mapping, compact
lookup, lane grouping and numerator arithmetic. It is **not** an isolated ablation
proving that logarithmic scheduling alone explains the improvement. Conditional
model switching, deployment benchmarks and research novelty remain separate work.
