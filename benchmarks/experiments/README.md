# Rejected performance prototypes

These are research artifacts, not library dependencies or supported APIs.

`avx2-prototype` was tested on Xeon Platinum 8474C. Four-lane AVX2 gathers from a
512 KiB direct table made the logarithmic DC decoder slower (~5.75 ns/symbol on
book1, versus ~2.6 for scalar compact lookup). The prototype is not enabled in
the library. Its unsafe code has not received release-level validation.

Other rejected changes: packed 64-bit physical input windows; packing four log
budgets into 16-bit fields of one u64; automatic native-target vectorization;
counting schedule flags in a separate pass. None improved the selected book1
four-state path. Reverse reconstruction of logarithmic schedules removes the
workspace but was slower; it is retained only as an independent test oracle.
