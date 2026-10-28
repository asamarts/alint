# v0.21 R8 release-readiness report

Status: **conditional go** — all code, semantic, corpus, generated-artifact and
CI gates pass. The release remains blocked on the kbench 100k/1M wall-clock and
1M RSS measurements required by section 9.6.

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
| 100k wall-clock vs `v0.18.2` | kbench, S1-S3 × full/changed, 30 runs | **not run** |
| 1M wall-clock vs `v0.18.2` | kbench, one warm-up + three runs | **not run** |
| 1M peak RSS vs `v0.18.2` | kbench, ≤ +10% | **not run** |

## Performance evidence already recorded

- Deterministic S1-S3 gates and the v0.18.2 reference gate pass in CI.
- Relation scenario: 42,816,797 Ir at 1k and 297,682,329 Ir at 10k on the
  recording host; 839 read / 10,898 stat / 632 open / 804 dents at 10k.
- The combined relation + per-file configuration reads the same files once:
  `key_parity` alone 839 reads, per-file rule alone 441, both 839 at 10k.
- Full corpus replay: 106/110 replayed, 49 with `cross_file` `equals`, zero
  shadow differences.

## Remaining release blockers

1. Run on the canonical kbench host with caches dropped:
   - S1-S3 × {full, changed} at 100k, 30 runs, median ≤ +3% vs v0.18.2;
   - S1-S3 × {full, changed} at 1M, one warm-up + three runs, ≤ +3%;
   - peak RSS at 1M ≤ +10%.
2. Re-run the shadow corpus replay against `v0.20.1` on the final release
   candidate.
3. Fill the measured numbers into this report and replace the conditional
   verdict with the final go/no-go.

## Go/no-go

**Conditional GO.** Phase 2's relation IR, `key_parity`, shadow port, plugin
contract and corpus evidence meet the semantic and correctness bar. The
release must not be tagged until the kbench budgets above are measured and
within budget. If any budget is exceeded, the numbers return for a V21-6
decision rather than being waived.
