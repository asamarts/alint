# v0.21 R8 release-readiness report

Status: **HOLD** (2026-10-29). The pre-tag audit in
[`pre-release-audit.md`](pre-release-audit.md) found that `key_parity` misses
real differences (W1), that several "pass" rows below have no gate behind them
(W2), and that the go/no-go assessment of the port was never written (W2-8).
The performance and memory evidence below still stands: the benchmark
generator writes well-formed Rust text, the Rust import-gate literal prefilter
removes the remaining S2 slowdown, clean-workload 1M S2 RSS is 118,924 KB
(within v0.18.2 +10% and below v0.20.1), and S2 is faster than v0.20.1. The
verdict is revisited once W1 and W2 land.

## Acceptance matrix

| Gate | Evidence | Status |
|---|---|---|
| `key_parity` across eight formats | rule scenarios + 19 unit tests; dotted keys, empty peers, and gitignored literal peers are missed (audit W1-1 to W1-4; fix in #407) | **fail** |
| `key_parity` `--changed`, trace, fingerprints | e2e scenarios + golden fingerprints (715 tests); the unrelated-edit scenario cannot fail and no determinism test exists (W1-11, W1-12; fix in #407) | **gap** |
| `cross_file` `equals` shadow | e2e + property generator; 0 differences | pass |
| Corpus shadow replay | expanded-scope run 40912352842 vs `v0.20.1`: 106/110 replayed, 4 expected skips, 0 crashed, 0 differ | pass |
| Contract traits and bridge | adapter/analyzer bridge tests; `key_parity` uses `ContractAnalyzer`, but the rule drops the analyzer's errors and the analyzer misses dotted keys (W1-1, W1-3; fix in #407); the bridge does not validate or confine finding paths (W2-6) | **fail** |
| Relation limits | defined with defaults and caps; `RowsPerRun` and `DistinctValues` are not enforced and the join limits have no consumer (W2-3) | **gap** |
| Invariants 6-8: visibility, feature isolation, contract boundary | module visibility test; `check-shadow-feature-tree.sh`; text-scan import test. Three cache methods are public and undocumented as hidden (W2-5), the feature check passes with a leak injected (W2-2), and the import test misses non-`use` paths (W2-4) | **gap** |
| Registry injection | CLI, LSP, testkit (`Runner::with_registry`), DSL catalog; `alint test`, `rules show`/`list`, and LSP hover bypass it, and no test runs a custom kind (W2-1) | **gap** |
| WIT draft | `witforge check wit/alint-plugin/0.1.0` (v2 wire lock), run by hand; no CI check (W2-7) | pass (manual) |
| Instruction reference S1-S3 | deterministic + v0.18.2 reference gates green on `main`/PRs | pass |
| Relation scenario | `relation_coalescing.yml` 1k/10k recorded | pass |
| S4 workload | #399 changed the polyglot tree's content without a revision bump (W2-9) | **gap** |
| Go/no-go assessment of the port | not written (W2-8) | **missing** |
| Generated artifacts | preflight: schema, facts, architecture, docs export, roadmap | pass |
| Formatting, clippy, workspace tests | preflight | pass |
| Cross-platform CI | Linux/macOS/Windows green | pass |
| 100k wall-clock vs `v0.18.2` | kbench, S1-S3 × full/changed, 30 runs | pass (worst +0.27%) |
| 1M wall-clock vs `v0.18.2` | kbench, same-session, one warm-up + three runs | pass (worst +2.15%) |
| 1M peak RSS vs `v0.18.2` | clean workload, 1M persistent tree | **pass** (118,924 vs 118,044 KB, +0.75%) |
| 1M peak RSS vs `v0.20.1` | clean workload, 1M persistent tree | **pass** (118,924 vs 119,420 KB) |
| 1M S2 wall-clock vs `v0.20.1` | clean workload, 5 runs | **pass** (5.357 s vs 5.861 s, −8.6%) |

## Performance evidence already recorded

- Deterministic S1-S3 gates and the v0.18.2 reference gate pass in CI.
- Relation scenario: 42,816,797 Ir at 1k and 297,682,329 Ir at 10k on the
  recording host; 839 read / 10,898 stat / 632 open / 804 dents at 10k.
- The combined relation + per-file configuration reads the same files once:
  `key_parity` alone 839 reads, per-file rule alone 441, both 839 at 10k.
- Final expanded-shadow corpus replay (2026-10-29, run 40912352842): 106/110
  replayed, 49 with `cross_file` `equals`, four expected skips, zero crashes,
  zero shadow differences against `v0.20.1`.
- kbench 100k (30 runs, same-session v0.18.2 reference): S1 −8.80%/−9.37%,
  S2 −4.01%/−5.63%, S3 +0.27%/+0.25%.
- kbench 1M (same-session v0.18.2 reference, 3 runs): S1 −2.88%/−2.55%,
  S2 −3.19%/−0.60%, S3 −3.24%/+2.15%.
- Benchmark-generator correction: generated `.rs` sources now end with a
  newline, so S2 does not fabricate one `final_newline` finding per file.
  S1-S3 carry workload revision 2; S4 stays at revision 3 (W2-9).
- Clean-workload 1M S2 peak RSS: v0.18.2 118,044 KB; v0.20.1 119,420 KB;
  current 118,924 KB (+0.75% vs v0.18.2, below v0.20.1).
- Clean-workload 1M S2 wall-clock: v0.20.1 5.861 s; current 5.357 s (−8.6%).
  S1 remains at parity (4.15 s).
- The Rust import-gate default pattern now prechecks for the literal `use`
  before running the per-line regex; generated source without imports skips
  the scan. Full S2 100k drops from 567 ms to 521 ms; 1M from 5.861 s to
  5.357 s.

## Remaining release work

1. W1 and W2 of [`pre-release-audit.md`](pre-release-audit.md): fix
   `key_parity` and give it a determinism test (W1, #407), then close the
   gaps behind the rows the audit disputes (W2: registry injection, the
   shadow feature-tree check, limits, the import-boundary test, the cache
   methods' visibility, the contract bridge's path checks, the WIT check,
   S4's workload revision), each with a gate.
2. Write the go/no-go assessment the release acceptance requires: whether the
   ported path became simpler and more consistent, with evidence.
3. Complete release preparation: version bump, changelog, generated artifacts,
   full release preflight and cross-platform checks.
4. Tag only after those checks pass.

## Go/no-go

**HOLD.** Memory passes the v0.18.2 +10% gate and is below v0.20.1; S2
wall-clock is also below v0.20.1; the expanded-shadow corpus replay is
zero-diff. Those results stand, but they cover performance and the replay
only. The release acceptance also requires a correct `key_parity` and a
written judgment of the port, and the audit found neither yet.
