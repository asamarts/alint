# v0.12.0 micro (criterion) snapshot — baseline conditions

Criterion micro-bench snapshot for v0.12.0 (33 groups, matching prior versions).

**Provenance.** Recorded on the shared dev box (self-hosted AMD Ryzen 9 3900X) under
VARIABLE load: the run started at 1-min load ~1.9, and a co-tenant spike pushed it
to ~26 over the ~17-minute run, so the late-running groups (alphabetically:
`walker`, `structured_query`, `single_file_*`) saw more contention than the early
ones. Criterion's outlier rejection is applied (per-bench outlier counts live in
the samples), and the absolute numbers held up (tight CIs, normal-ish outlier
rates) — but treat these as BASELINE-conditions reference data, not a pristine run.
The box would not quiesce during work hours; the overnight window used for the
macro was not available for this snapshot.

**For the regression verdict, do NOT diff these wall-clock micro numbers against a
prior version.** Use the load-immune deterministic gate
(`ci/scripts/det-perf-gate.sh`) and the verified-quiet **macro** snapshot
(`../../../../macro/results/linux-x86_64/v0.12.0/`). The full analysis — including
why v0.12 has no real regression (instruction counts flat; the macro S12 "+16% vs
v0.11.0" is a stale-baseline artifact) — is in
`docs/benchmarks/investigations/2026-06-v0.12-perf-validation/`.
