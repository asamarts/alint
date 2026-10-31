# v0.21 R8 release-readiness report

Status: **HOLD** (2026-10-29). The pre-tag audit in
[`pre-release-audit.md`](pre-release-audit.md) found that `key_parity` misses
real differences (W1), that several "pass" rows below have no gate behind them
(W2), and that the go/no-go assessment of the port was never written (W2-8).
W1 has since been fixed (#407, merged 2026-10-30, and the review that followed it); W2 is under
way. The performance and memory evidence below still stands: the benchmark
generator writes well-formed Rust text, the Rust import-gate literal prefilter
removes the remaining S2 slowdown, clean-workload 1M S2 RSS is 118,924 KB
(within v0.18.2 +10% and below v0.20.1), and S2 is faster than v0.20.1. The
verdict is revisited once W1 and W2 land.

## Acceptance matrix

| Gate | Evidence | Status |
|---|---|---|
| `key_parity` across eight formats | 25 e2e scenarios + 47 unit tests; dotted keys, empty peers, and gitignored literal peers (audit W1-1 to W1-4) fixed in #407, and per-file plural grouping, `strip_root` on a file without one object root, and values in parse errors fixed in the review after it | pass |
| `key_parity` `--changed`, trace, fingerprints | e2e scenarios + golden fingerprints (729 tests); the unrelated-edit scenario now fails a full run, and a determinism test runs shuffled index orders and several thread counts (W1-11, W1-12, #407) | pass |
| `cross_file` `equals` shadow | e2e + property generator; 0 differences | pass |
| Corpus shadow replay | expanded-scope run 40912352842 vs `v0.20.1`: 106/110 replayed, 4 expected skips, 0 crashed, 0 differ | pass |
| Contract traits and bridge | adapter/analyzer bridge tests; `key_parity` uses `ContractAnalyzer` and reports its errors (W1-1, W1-3, fixed in #407); the bridge does not validate or confine finding paths (W2-6, latent until a plugin host exists) | **gap** |
| Relation limits | defined with defaults and caps; `RowsPerRun` and `DistinctValues` are not enforced and the join limits have no consumer (W2-3) | **gap** |
| Invariants 6-8: visibility, feature isolation, contract boundary | module visibility test; `check-shadow-feature-tree.sh` reads `alint-rules`' resolved features for each shipped or benchmarked build on every target, and five injected leaks fail it (`test-check-shadow-feature-tree.sh`, W2-2); text-scan import test. Three cache methods are public and undocumented as hidden (W2-5), and the import test misses non-`use` paths (W2-4) | **gap** |
| Registry injection | CLI (`check`, `test`, `rules list`/`show`/`categories`), LSP (session and hover), testkit (`Runner::with_registry`), DSL catalog; one test per surface registers a custom kind and observes it run or be described, and the built-in registry rejects it (W2-1) | pass |
| WIT draft | `witforge check wit/alint-plugin/0.1.0` (v2 wire lock), run by hand; no CI check (W2-7). The tag waits for witforge 0.1.0 and a CI regeneration check (ADR-0025 decision 4) | **blocked** |
| Instruction reference S1-S3 | deterministic + v0.18.2 reference gates green on `main`/PRs | pass |
| Relation scenario | `relation_coalescing.yml` 1k/10k recorded | pass |
| S4 workload | #399 changed the polyglot tree's content without a revision bump (W2-9) | **gap** |
| Go/no-go assessment of the port | verdict decided: GO on consistency, simplicity gated in v0.22 (ADR-0025 decision 3); the written assessment is W2-8 | **gap** |
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

1. W2 of [`pre-release-audit.md`](pre-release-audit.md) (W1 is done in
   #407): close the gaps behind the rows the audit disputes (W2: registry injection, the
   shadow feature-tree check, limits, the import-boundary test, the cache
   methods' visibility, the contract bridge's path checks, the WIT check,
   S4's workload revision), each with a gate.
2. Write the go/no-go assessment the release acceptance requires: whether the
   ported path became simpler and more consistent, with evidence.
3. Complete release preparation: version bump, changelog, generated artifacts,
   full release preflight and cross-platform checks.
4. Wait for witforge 0.1.0 and run the contract's regeneration check in
   default CI (ADR-0025 decision 4).
5. Tag only after those checks pass.

## Go/no-go

**HOLD.** Memory passes the v0.18.2 +10% gate and is below v0.20.1; S2
wall-clock is also below v0.20.1; the expanded-shadow corpus replay is
zero-diff. Those results stand, but they cover performance and the replay
only. The release acceptance also requires a correct `key_parity` and a
written judgment of the port.

The owner's verdict on the port (2026-10-30, ADR-0025 decision 3) is GO on
consistency: v0.21 ships once W1, W2, and the witforge check are done, and
Phase 3 starts only if v0.22 deletes the old `equals` code, keeps its
replacement within today's 190 non-comment lines with no calls into
`cross_file::eval`, and shows zero differences with the budgets held.
