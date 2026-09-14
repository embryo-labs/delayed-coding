# Performance prototypes

These are research artifacts, not library dependencies or supported APIs.

`avx512-prototype` is a successful **exact DC16** co-design experiment, not a
Log-DC kernel: sixteen 32-bit lanes per vector, four interleaved vectors, packed
P12 tables and masked physical I/O. Both operations are below 1 ns/symbol on
book1 on the measured machine. See [scope, rate cost and throughput analysis](../AVX512.md).

## Rejected experiments

`avx2-prototype` was tested on Xeon Platinum 8474C. Four-lane AVX2 gathers from a
512 KiB direct table made the logarithmic DC decoder slower (~5.75 ns/symbol on
book1, versus ~2.6 for scalar compact lookup). The prototype is not enabled in
the library. Its unsafe code has not received release-level validation.

Other rejected changes: packed 64-bit physical input windows; packing four log
budgets into 16-bit fields of one u64; automatic native-target vectorization;
counting schedule flags in a separate pass. None improved the selected book1
four-state path. Reverse reconstruction of logarithmic schedules removes the
workspace but was slower; it is retained only as an independent test oracle.
