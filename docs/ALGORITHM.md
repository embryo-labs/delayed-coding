# Delayed Coding implementation notes

This document describes this implementation; the [paper](https://www.vldb.org/pvldb/vol17/p2528-zhang.pdf)
contains the original derivation. Probability precision is fixed at 16 bits.
Delay threshold D is independently selected in [16, 32] by the Rust API.

## Symbol mapping

A model assigns each supported symbol a positive integer weight w, and weights
sum to 65536. An alias table partitions all possible 16-bit code words into
power-of-two-sized buckets. A cutoff chooses one of two entries per bucket.
Each entry supplies symbol ID, weight and an adjustment that recovers a remainder
r in [0, w). A symbol may own multiple disjoint code segments.

The encoder performs the inverse mapping `(symbol, r) -> word`. Short segment
lists are scanned; long lists use binary search over cumulative segment ends.
Optional direct tables preserve exactly the same mapping:

- encode: 65536 u16 entries, indexed by symbol prefix weight plus r (128 KiB);
- decode: 65536 packed u64 entries containing symbol, w-1 and r (512 KiB).

The packing stores w-1 because weight 65536 must remain representable. Segment
boundaries can also equal 65536, so they cannot be stored in u16.

## Forward decode and backward encode

Decoder state starts at numerator n=0, denominator d=1, with no virtual word.
For each symbol:

1. Read a pending virtual word if one exists; otherwise consume a physical u16.
2. Look up `(symbol, w, r)` and update `n = n*w + r`, `d = d*w`.
3. If `d >= 2^D`, save `n & 65535` as the next virtual word, and shift both
   n and d right by 16.

The optimized implementation defers step 3 until the next read of that state.
It tests d directly and consumes n's low bits then. This removes the redundant
pending-word flag and a second branch, while preserving physical word order.
The unnormalized final numerator includes any pending word, so checking n=0
also validates that pending word. Delaying the shift does not change bytes.

The encoder first simulates the denominator updates to learn which symbols use
virtual words. It then walks backwards from n=0, splitting n into quotient and
remainder by w. The remainder identifies the symbol's code word. Virtual words
are embedded back into n; physical words are written backwards to the output.

### Executable layout independence

`Layout<DELAY, LANES>` implements the capacity-only recurrence. For a single lane,
with capacity d before a read, let v = [d >= 2^DELAY]. The next capacity is
`floor(d / 65536^v) * w`. The source decision and next capacity depend only on d
and the realized frequency w, not the interval position, remainder, or numerator.
Induction from d=1 proves that the ordered frequency sequence determines the
complete physical/virtual schedule. For interleaving, apply the induction to each
lane with a fixed lane assignment. Every physical word contributes exactly two
raw bytes, so the same computation gives exact offsets and payload length.

Tests compare the predicted offsets against the real decoder's byte cursor,
including reordered symbol mappings, prefixes and lane/delay configurations.
This is schedule independence, not codeword-value independence: a suffix can
change earlier physical words. Conditional frequencies may not yet be known at
decode time. The standard rANS information-state normalization does not have
this general frequency-only schedule property; this is not a global novelty claim.

The allocating `encode` API now schedules first and allocates exactly the payload,
then embeds directly into that allocation. It still stores one schedule byte per
symbol. The indexed-record example also computes layout before assigning slices
of a shared arena; its subsequent `encode_into` calls rebuild/validate schedules,
so that example trades an extra pass for exact preassigned storage.

### Opt-in physical lookahead

`decode_lookahead_into` keeps one predecoded physical word pending. Consuming it
triggers lookup of the next physical word, independently of the current numerator
update. Virtual words still require the numerator, and the capacity recurrence
still controls consumption. Only a fixed model is supported. Input reads remain
bounded and unused prefetched words do not count as consumed for final validation.
Larger on-stack batches were tested separately and did not show general benefits.
See the [experiment report](../benchmarks/LAYOUT_LOOKAHEAD.md) for wins and losses;
source-level independence alone is not proof of a shorter machine critical path.

Before multiplication d < 2^D and w <= 2^16, so the product is < 2^(D+16).
Valid backward states obey the corresponding bound. With D <= 32, the encoder
state is < 2^48, so u64 arithmetic has headroom. Malformed payloads do not inherit
the valid-numerator invariant; the decoder uses explicit wrapping arithmetic and
bounded input reads, avoiding Debug-only overflow panics. These checks are not an
integrity guarantee.

### Branchless four-state windows and speculative encoding

The optional branchless decoder calculates four source flags and prefix offsets
from the capacity states. If eight bytes remain, every possible physical source
load is within that window, including loads whose values are discarded for
virtual lanes. Bit masks select the real word and shift counts normalize each
state without a data-dependent source branch. A separate scalar tail handles
less than eight remaining bytes and non-multiple-of-four output sizes, including
malformed inputs. No padded allocation or unchecked read is required. Table mode
is selected once per block; an exact 65,536-entry array removes direct-lookup
bounds checks without unsafe code. The ordinary grouped decoder remains available.

The optional `speculative-encode` feature applies to 4/8-state model/event encoding.
Before each reverse event it writes a word at `position-2` and advances position
only for physical events. A virtual event's store will be replaced. Why is this
safe even with an exactly sized output? Every lane's first event is physical,
since its initial capacity is one. Every virtual event therefore has at least
one earlier physical event in its lane that is still unwritten in reverse order.
The speculative slot is inside the final payload, never before its start or
after its end. Physical output order and final bytes are unchanged. This property
is tested with empty/short blocks, long virtual runs and untouched output prefixes.

Internal scheduling also uses a validated-frequency path: immutable model
construction checked the frequency and a checked `2*count` bound protects total
byte accounting. The public incremental `Layout::push_frequency` still checks
each caller-supplied frequency and count overflow, with unchanged error semantics.

Both optimizations trade branch mispredictions for more unconditional work. They
are not uniformly faster; see the [measured report](../benchmarks/SPEED_KERNELS.md).

## Exact reciprocal division

For f >= 2 precompute `c = ceil(2^64 / f)`. Then the high half of `n*c` is
`floor(n/f)` for n < 2^48 and f <= 2^16. To see this, write
`c/2^64 = 1/f + e`, with 0 <= e < 1/2^64. Then n*e < 1/65536 <= 1/f,
smaller than the distance from any nonintegral multiple of 1/f to the next integer.
The integral case has error < 1 as well. Frequency 1 uses q=n directly.

Rust's u128 multiply expresses the required high product. The
`reference-division` feature uses ordinary division for differential tests and
performance ablation. Tests exercise all 65536 possible frequencies at state and
quotient boundaries. No floating-point approximation is involved.

## Interleaving

L independent states serve symbols in round-robin order: symbol i uses state
`i % L`. Physical words from these states share one stream in decode order.
Reverse encoding naturally emits the same stream. Each state starts from (0,1)
and is checked at the end. No separate stream offsets or serialized rANS-like
terminal states are needed, but additional states can increase short-block payloads.

The lane count is essential external metadata. Rust supports L=1/2/4/8; the C
block API currently dispatches L=1/4. Tests independently encode each scalar lane
and merge its physical words according to the forward schedule, then compare the
result byte-for-byte with the interleaved encoder.

Independent arithmetic states do not remove dependencies in a conditional model.
An application must still determine each context from already available symbols.
The current interleaved implementation is scalar, not SIMD.
