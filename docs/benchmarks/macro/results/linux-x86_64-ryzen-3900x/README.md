# Retired benchmark series: AMD Ryzen 9 3900X dev box

This is the **historical** alint benchmark corpus, measured on the 3900X
development machine (12-core / 62 GB / ext4 / rustc 1.95) from v0.5.6 through
v0.13.0. It is retained verbatim for continuity.

It is **no longer the canonical series.** As of 2026-07-15 the canonical host is
`kbench` (a dedicated, quiet Intel i7-6700HQ laptop); its results live at
[`../linux-x86_64/`](../linux-x86_64/) and drive `docs/benchmarks/HISTORY.md`
and the alint.org `/benchmarks/` trajectory. The 3900X was a contended daytime
box, which repeatedly contaminated CI bench-record runs — see
[`../../../../design/v0.14/bench-host-migration.md`](../../../../design/v0.14/bench-host-migration.md)
and the `investigations/2026-07-1m-writeback-contention/` write-up.

This is a deliberate **re-baseline**, not a continuation: per `METHODOLOGY.md`,
absolute numbers are not comparable across machines (kbench is ~1.5x slower
per-core), so the two series are kept separate rather than spliced. Its release
trajectory is retained in [`legacy/HISTORY.md`](../../../legacy/HISTORY.md).

The micro (criterion) counterpart is at
`docs/benchmarks/micro/results/linux-x86_64-ryzen-3900x/`.
