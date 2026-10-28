# v0.21 R8 release-readiness report

Status: **no-go on the original RSS budget, but the approved v0.20.1 target is
met.** The benchmark generator now terminates generated Rust sources, removing
one million accidental `final_newline` findings. On the corrected workload,
current 1M S2/full RSS is 923,708 KB vs v0.20.1 937,552 KB. The original
V21-6 v0.18.2 comparison is still above its +10% budget (v0.18.2 600,652 KB on
the corrected tree), so that gate requires an explicit decision.

## Acceptance matrix

| Gate | Evidence | Status |
|---|---|---|
| `key_parity` across eight formats | rule scenarios + 19 unit tests | pass |
| `key_parity` `--changed`, trace, fingerprints | e2e scenarios + golden fingerprints (715 tests) | pass |
| `cross_file` `equals` shadow | e2e + property generator; 0 differences | pass |
| Corpus shadow replay | run 40601610148 vs `v0.20.1`: 106/110 replayed, 4 expected skips, 0 crashed, 0 differ | pass |
| Contract traits and bridge | adapter/analyzer bridge tests; `key_parity` uses `ContractAnalyzer` | pass |
| Registry injection | CLI, LSP, testkit (`Runner::with_registry`), DSL catalog | pass |
| WIT draft | `witforge check wit/alint-plugin/0.1.0` (v2 wire lock) | pass |
| Instruction reference S1-S3 | deterministic + v0.18.2 reference gates green on `main`/PRs | pass |
| Relation scenario | `relation_coalescing.yml` 1k/10k recorded | pass |
| Generated artifacts | preflight: schema, facts, architecture, docs export, roadmap | pass |
| Formatting, clippy, workspace tests | preflight | pass |
| Cross-platform CI | Linux/macOS/Windows green | pass |
| 100k wall-clock vs `v0.18.2` | kbench, S1-S3 × full/changed, 30 runs | pass (worst +0.27%) |
| 1M wall-clock vs `v0.18.2` | kbench, same-session, one warm-up + three runs | pass (worst +2.15%) |
| 1M peak RSS vs `v0.18.2` | corrected workload, 1M persistent tree | **fail on S2/full (+53.8%)** |
| 1M peak RSS vs `v0.20.1` | corrected workload, 1M persistent tree | **pass** (923,708 vs 937,552 KB) |

## Performance evidence already recorded

- Deterministic S1-S3 gates and the v0.18.2 reference gate pass in CI.
- Relation scenario: 42,816,797 Ir at 1k and 297,682,329 Ir at 10k on the
  recording host; 839 read / 10,898 stat / 632 open / 804 dents at 10k.
- The combined relation + per-file configuration reads the same files once:
  `key_parity` alone 839 reads, per-file rule alone 441, both 839 at 10k.
- Full corpus replay: 106/110 replayed, 49 with `cross_file` `equals`, zero
  shadow differences.
- kbench 100k (30 runs, same-session v0.18.2 reference): S1 −8.80%/−9.37%,
  S2 −4.01%/−5.63%, S3 +0.27%/+0.25%.
- kbench 1M (same-session v0.18.2 reference, 3 runs): S1 −2.88%/−2.55%,
  S2 −3.19%/−0.60%, S3 −3.24%/+2.15%.
- Benchmark-generator correction: generated `.rs` sources now end with a
  newline, so S2 does not fabricate one `final_newline` finding per file.
  Generated-tree scenarios carry workload revision 2.
- Corrected-workload 1M S2 peak RSS: v0.18.2 600,652 KB; v0.20.1 937,552 KB;
  current 923,708 KB. Current is below v0.20.1 but remains above v0.18.2's
  +10% budget.
- Corrected-workload 1M wall-clock is at parity with v0.20.1: S1 4.153 s vs
  4.150 s (10 runs); S2 11.97 s vs 11.75 s (10 runs). It is not strictly below
  v0.20.1 on S2, so the release hold remains.
- Release bisect (same 1M tree, S2/full, same toolchain): v0.18.2 1,068,512 KB;
  v0.19.0 1,656,816 KB; v0.20.0 1,682,880 KB; v0.20.1 1,764,188 KB; current
  1,744,668 KB. A one-rule `file_exists` config is ~11.5 MB on all versions.
  The growth appears with per-file line rules; `final_newline`,
  `no_trailing_whitespace`, and `indent_style` each reproduce it in isolation.
  The v0.19.0 release is therefore the first affected release. A symbolized
  100k Massif run peaks at ~82 MB; its largest visible allocations are the
  `ignore` walker queues and `FileIndex` path storage, so the next step is an
  allocation-level profile of per-file index retention, not a speculative rule
  change.

## Remaining release blockers

1. Decide the original V21-6 v0.18.2 RSS gate: the approved v0.20.1 target is
   met, but v0.18.2's +10% budget is not. Record the decision or continue
   reducing findings-vector memory.
2. Re-run the shadow corpus replay against `v0.20.1` on the final release
   candidate.
3. Replace this report's verdict with the final decision.

## Go/no-go

**Conditional GO against the approved v0.20.1 target; NO-GO against the
original v0.18.2 RSS gate.** The generated-workload bug is fixed and current
RSS is below v0.20.1. The remaining v0.18.2 budget gap needs an explicit V21-6
decision; it is not silently waived.
