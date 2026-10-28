# v0.21 R8 release-readiness report

Status: **no-go on the RSS budget** — all code, semantic, corpus,
generated-artifact, CI and wall-clock gates pass. The 1M S2/full peak RSS is
materially above the v0.18.2 reference; V21-6 requires a decision rather than a
waiver.

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
| 1M peak RSS vs `v0.18.2` | kbench, 1M persistent tree | **fail on S2/full (+65%)** |

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
- kbench 1M peak RSS: S1 163,920 KB vs 172,100 KB (−4.8%); S2 1,717,604 KB vs
  1,042,236 KB (**+64.8%**); S3 249,868 KB vs 253,200 KB (−1.3%).
  v0.20.1 measures 1,725,096 KB on S2 with the same scenario, so the increase
  predates v0.21 and is not caused by the relation layer.

## Remaining release blockers

1. Resolve the S2 1M RSS budget with V21-6: either fix the pre-existing
   memory growth, or record an approved budget revision with evidence.
2. Re-run the shadow corpus replay against `v0.20.1` on the final release
   candidate.
3. Replace this no-go with the final decision once the RSS budget is resolved.

## Go/no-go

**NO-GO** until the 1M S2 RSS budget is resolved. Phase 2's relation IR,
`key_parity`, shadow port, plugin contract, corpus evidence and wall-clock
budgets meet the bar. The S2 RSS increase predates v0.21 (v0.20.1 reproduces
it), but V21-6 applies to the cumulative release budget and the numbers must go
back for a decision; they are not waived here.
