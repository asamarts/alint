# v0.21 R8 release-readiness report

Status: **HOLD** (2026-10-29). The pre-tag audit in [`pre-release-audit.md`](pre-release-audit.md)
found that `key_parity` misses real differences (W1), that several "pass" rows below have no gate
behind them (W2), and that the go/no-go assessment of the port was never written (W2-8). W1 has
since been fixed (#407, merged 2026-10-30, and the review that followed it), and W2 is done (#411)
except W2-7, which waits on witforge 0.1.0. The performance and memory evidence below still
stands: the benchmark generator writes well-formed Rust text, the Rust import-gate literal
prefilter removes the remaining S2 slowdown, clean-workload 1M S2 RSS is 118,924 KB (within
v0.18.2 +10% and below v0.20.1), and S2 is faster than v0.20.1. The status stays HOLD until the
remaining release work below is done.

## Acceptance matrix

| Gate | Evidence | Status |
|---|---|---|
| `key_parity` across eight formats | 25 e2e scenarios + 47 unit tests; dotted keys, empty peers, and gitignored literal peers (audit W1-1 to W1-4) fixed in #407, and per-file plural grouping, `strip_root` on a file without one object root, and values in parse errors fixed in the review after it | pass |
| `key_parity` `--changed`, trace, fingerprints | e2e scenarios + golden fingerprints (729 tests); the unrelated-edit scenario now fails a full run, and a determinism test runs shuffled index orders and several thread counts (W1-11, W1-12, #407) | pass |
| `cross_file` `equals` shadow | e2e + property generator; 0 differences | pass |
| Corpus shadow replay | expanded-scope run 40912352842 vs `v0.20.1`: 106/110 replayed, 4 expected skips, 0 crashed, 0 differ | pass |
| Contract traits and bridge | adapter/analyzer bridge tests; `key_parity` uses `ContractAnalyzer` and reports its errors (W1-1, W1-3, fixed in #407); the bridge refuses an adapter row for any resource but the one it was given (path, origin, and index position), a finding or related location outside the analysis's input resources or not normalized, and an unknown failure kind, each with a test (W2-6) | pass |
| Relation limits | defined with defaults and caps; adapters enforce `RowsPerExtractor`, the decode cache takes each fresh decode's rows from the run's `RunBudget` (`RowsPerRun`) and keys entries on the limits, set conversion enforces `DistinctValues`, and a test keeps the join limits reserved until Phase 3 adds joins; each limit has a test (W2-3) | pass |
| Invariants 6-8: visibility, feature isolation, contract boundary | module visibility test, and a test that requires `#[doc(hidden)]` on every public function elsewhere in alint-core that names the relation layer or the contract (the three `FileIndex` cache methods, W2-5); `check-shadow-feature-tree.sh` reads `alint-rules`' resolved features for each shipped or benchmarked build on every target, and five injected leaks fail it (`test-check-shadow-feature-tree.sh`, W2-2); the `plugin_api` boundary test parses the module and checks every path, `use` tree, attribute, derive, and macro argument, with the escapes a line scan missed as negative samples (W2-4) | pass |
| Registry injection | CLI (`check`, `test`, `rules list`/`show`/`categories`), LSP (session and hover), testkit (`Runner::with_registry`), DSL catalog; one test per surface registers a custom kind and observes it run or be described, and the built-in registry rejects it (W2-1) | pass |
| WIT draft | `wit/alint-plugin/v0.1`, a draft until plugins load (ADR-0027). `ci/scripts/witforge-consumer.sh` checks it and regenerates the committed `alint-witforge-consumer` output with `--check`; its controls fail on each changed generated file, an extra or missing file, an unlocked member, and an unchecked version directory, and `--write` restores a changed file; the crate's tests convert every type the two sides share and account for every item on both (`tests/inventory.rs`); run by hand against witforge 0.1.0 built from its repository. The tag waits for witforge 0.1.0 and the CI job in [`witforge-adoption.md`](witforge-adoption.md) section 5 (W2-7, ADR-0025 decision 4) | **blocked** |
| Instruction reference S1-S3 | deterministic + v0.18.2 reference gates green on `main`/PRs | pass |
| Relation scenario | `relation_coalescing.yml` 1k/10k recorded | pass |
| S4 workload vs `v0.20.1` | revision 4: newline-terminated sources (and #399's whitespace change), with a generator test; same-session kbench run on the revision-4 tree, full and changed, 30 runs at 100k and three at 1M (W2-9) | pass (worst +2.12%, 1M changed) |
| Go/no-go assessment of the port | written below (W2-8): GO on consistency, simplicity gated in v0.22 (ADR-0025 decision 3), with gate 2 measured as ADR-0026 defines | pass |
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
  S1-S3 carry workload revision 2; S4 carries revision 4 (W2-9).
- S4 revision 4, kbench, caches dropped, one session (2026-10-30), in ABBA
  order: v0.21 at 100k, v0.20.1 at 100k, v0.20.1 at 1M, then v0.21 at 1M.
  v0.21 (built from #411's `crates/`, unchanged through its merge) against
  v0.20.1 (the tag), median wall-clock: 100k full 857.1 vs 871.5 ms (−1.65%), 100k
  changed 868.7 vs 875.5 ms (−0.78%), 1M full 8.920 vs 9.030 s (−1.22%), 1M
  changed 10.497 vs 10.279 s (+2.12%; the fastest runs differ by +0.79%, and
  v0.21's three runs span 10.29-10.84 s). Section 9.6's +3% wall-clock budget
  holds. ADR-0024 decision 6 keeps S4 out of the v0.18.2 comparison, so this run
  is S4's only check.
- Revision 4 removes the per-file findings that dominated S4's full runs
  (v0.20.1 at 1M: 15.55 s recorded under revision 3, 9.03 s here). Changed runs
  did not get faster (v0.20.1 at 100k: 884 ms recorded, 876 ms here; at 1M,
  9.12 s and 10.28 s, from different sessions). So on this workload a changed
  run is now no faster than a full one in either version: level at 100k, about
  14% slower at 1M. That cost comes from `--changed` itself, not from v0.21.
- Clean-workload 1M S2 peak RSS: v0.18.2 118,044 KB; v0.20.1 119,420 KB;
  current 118,924 KB (+0.75% vs v0.18.2, below v0.20.1).
- Clean-workload 1M S2 wall-clock: v0.20.1 5.861 s; current 5.357 s (−8.6%).
  S1 remains at parity (4.15 s).
- The Rust import-gate default pattern now prechecks for the literal `use`
  before running the per-line regex; generated source without imports skips
  the scan. Full S2 100k drops from 567 ms to 521 ms; 1M from 5.861 s to
  5.357 s.

## Assessment of the port (W2-8)

The stop rule (`general-linter.md` section 11.1) asks whether the ported path became simpler
and more consistent. The port is `cross_file` `equals` evaluated through the relation IR (the
R4 shadow), and `key_parity`, the first kind built on the IR (R6-R9).

**Consistency is shown.**

- *Same results.* The shadow finds zero differences on the 17 e2e scenarios that use
  `relation: equals`, on its property generator, and on 106 of 110 corpus configurations (49 of
  them with `equals`; four expected skips; run 40912352842).
- *One model.* The shadow and `key_parity` read through the run's one read cache and decode
  cache (with `key_parity` alone the 10k relation scenario makes 839 reads, and with a per-file
  rule added still 839), produce rows of one row, resource, and pointer model, and share the
  `Adapter` trait and the limits. `key_parity` also runs through the contract bridge, and the
  built-in adapters round-trip through it unchanged. (ADR-0025 called this "one extract adapter
  shared with `key_parity`"; ADR-0026 corrects that: the adapters differ, the caches, trait, and
  model are shared.)
- *What W1 showed.* Every `key_parity` defect W1 and the review after it found was in rule code
  layered over the IR (a `.join(".")` key rendering, plural grouping, `strip_root`, an analyzer
  error dropped by the rule), and the dotted-key fix was to use the IR's key identity, the
  segment list, throughout. The IR's own guarantees now each have a gate: enforced limits
  (W2-3), a bridge that refuses output outside its input (W2-6), the visibility and import
  boundaries (W2-4, W2-5), and feature isolation (W2-2).

**Simplicity is not shown, and cannot be in v0.21.**

- Production `equals` still runs the old code. The code only `equals` executes is
  `check_equals` (93 non-comment lines) and `mismatch` (41): 134 lines, measured as ADR-0026
  defines. (ADR-0025's 190 also counted the source extraction the set relations share.)
- The shadow adds 359 non-comment lines and calls back into the old code: `mismatch` and
  `add_source_locations` to build findings, `redact_extracted` to redact them, and
  `eval::read_rel` for a missing source's message. A shadow must reproduce the old output, so
  this is expected, but it is not evidence that the IR path is simpler.

**Verdict: GO on consistency** (ADR-0025 decision 3). v0.21 ships once the remaining release
work below is done. Phase 3 starts only if v0.22 meets all three gates; otherwise the stop rule
applies:

1. the IR path becomes the authoritative `equals`, and the old `equals` code is deleted;
2. the code only the new `equals` executes is at most 134 lines, counted as ADR-0026 defines,
   and calls nothing in `cross_file::eval`;
3. the e2e suite and the corpus replay show zero differences against v0.21, with the section
   9.6 budgets held as ADR-0024 decision 6 measures them.

## Remaining release work

1. W3 of [`pre-release-audit.md`](pre-release-audit.md): changelog, documentation, and release
   preparation (version bump, generated artifacts, full release preflight, cross-platform
   checks, a fresh corpus replay).
2. Wait until witforge 0.1.0 is published and pinned, and run the contract's check, generation,
   and regeneration `--check` in default CI (W2-7, ADR-0025 decision 4);
   [`witforge-adoption.md`](witforge-adoption.md) has the path.
3. Tag only after those checks pass.

## Go/no-go

**HOLD.** Memory passes the v0.18.2 +10% gate and is below v0.20.1; S2
wall-clock is also below v0.20.1; the expanded-shadow corpus replay is
zero-diff. Those results stand, but they cover performance and the replay
only. The release acceptance also requires a correct `key_parity` and a
written judgment of the port.

The owner's verdict on the port (2026-10-30, ADR-0025 decision 3) is GO on
consistency: v0.21 ships once W1, W2, and the witforge check are done, and
Phase 3 starts only if v0.22 deletes the old `equals` code, keeps its
replacement within today's 134 `equals`-only lines (ADR-0026, which replaces
ADR-0025's 190) with no calls into `cross_file::eval`, and shows zero
differences with the budgets held. The assessment above records the evidence.
