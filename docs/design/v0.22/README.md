# v0.22: completing the relation-layer port

Status: done (2026-11-07). All three gates are met; released as v0.22.0.
Decisions: [ADR-0025](../../adr/0025-v021-pre-release-decisions.md) decision 3 (the gates),
[ADR-0026](../../adr/0026-measuring-the-v022-simplicity-gate.md) (how gate 2 is counted),
[ADR-0030](../../adr/0030-v022-owner-decisions.md) (the owner decisions and the order of work).

v0.22 closes Phase 2 of [`general-linter.md`](../general-linter.md) (section 11.1). Phase 3
(`version: 2` selectors and report-only constraints) starts only if v0.22 meets all three gates;
otherwise the program's stop rule applies.

## 1. Scope

The three gates of ADR-0025 decision 3:

1. **One `equals` implementation.** The relation layer becomes the authoritative `cross_file`
   `equals` (including the `cross_file_value_equals` alias), and the old `equals` code is deleted.
2. **Simplicity bound.** Code that only the new `equals` executes is at most 134 non-comment
   lines, counted on both sides as ADR-0026 defines, and calls nothing in `cross_file::eval`.
3. **Zero differences, budgets held.** The e2e suite and the corpus replay show zero differences
   against v0.21, and the budgets of `general-linter.md` section 9.6 hold as ADR-0024 decision 6
   measures them.

Then, in this order (ADR-0030 decision 3):

4. `FixContext`, `Config` and `CompiledNestedSpec` become `#[non_exhaustive]`, with constructors
   (ADR-0030 decision 2).
5. A hidden-path finding's fingerprint leaves the path out, with one minor release of
   compatibility matching (ADR-0030 decision 1).
6. Two dry-run follow-ups from v0.21.2: a create through a dangling symlink whose target the
   create's own directory creation makes, and shared lookups for the landing checks of each
   predicted create (ADR-0030 decision 4).
7. A pre-v1.0 roadmap item for sealing the other public `alint-core` structs.

Out of scope: the npm package's per-platform binaries (the release after v0.22.0), plugin
loading, and Phase 3.

## 2. Where the code stands

`cross_file` reads values for every value relation (`equals`, `subset`, `superset`,
`set_equals`) and for `resolves` through two near-duplicate functions in `cross_file/eval.rs`:
`source_values_from` (69 lines) and `target_values` (74), each calling `read_rel` and
`extract_values_bounded` directly. `check_equals` (93) and `mismatch` (41) implement `equals` on
top of them: the 134 lines ADR-0026 counts.

The shadow (`cross_file/shadow.rs`, behind the test-only `shadow-ir` feature)
reimplements `equals` on the relation layer: `ReadCache`, `DecodeCache` and `ExtractAdapter`,
with the target-syntax filter for regex targets. It has matched the old path on the e2e suite,
its property generator and the corpus replay since v0.21.0. It still calls `mismatch`,
`add_source_locations`, `redact_extracted` and `read_rel` in `cross_file::eval`. It skips one
case: a path that is not a valid `RelPath`, because `RelPath` keeps segments as UTF-8 strings.

`equals` never takes a source glob (`source.files` requires a set relation or `registered`), so
the shadow's literal-source scope already covers every `equals` rule.

## 3. Design of the port

The shadow's value reading cannot simply replace `check_equals`'s, because ADR-0026 item 4 counts
a helper that only `equals` uses: the shadow's reader and its failure emitters would add about 150
lines to the 134. The port therefore moves reading itself onto the relation layer, for every
relation that reads values, and leaves `equals` with only its own logic:

- **One reader, on the relation layer** (`cross_file/values.rs`). `read_values(side, path,
  extract)` replaces both `source_values_from` and `target_values`: it reads through the run's
  `ReadCache`, decodes through `DecodeCache` with `ExtractAdapter`, applies the target-syntax
  filter to a regex target, and reports a missing file, a file over the cap, an unreadable file, a
  parse failure, non-string values and skipped non-literals with the messages each side gives
  today. `source_values` and `each_target` move there too. Every value relation and `resolves`
  call it, so it is shared code and does not count against gate 2.
- **Finding helpers** (`cross_file/findings.rs`): `violation`, `note`, `analysis_failure`,
  `add_source_locations` and `redact_extracted` move out of `cross_file::eval`, unchanged. Other
  relations use them, so they are shared.
- **`equals` alone** (`cross_file/equals.rs`): `check_equals` and `mismatch`, and nothing else.
  The module imports nothing from `cross_file::eval`, and a unit test counts its non-comment lines
  outside tests (at most 134) and refuses a reference to `eval`, so gate 2 is asserted on every
  test run, not measured once.
- **Lossless paths.** `RelPath` keeps a path that is not valid UTF-8 (raw bytes on Unix, as the OS
  gives them on Windows), so the reader never needs a fallback. Display stays lossy, as
  `Path::display` is today. As built, the walked-path constructor `RelPath::from_os_path` keeps
  such a name; selectors and the plugin contract keep `from_index_path`, because a plugin receives
  paths as strings, so `key_parity` is unchanged.
- **Caches for every value rule.** `CrossFileRule::uses_relations` returns true for the value
  relations and `resolves`, so the engine provides run-scoped caches whenever such a rule runs.
  Only paths a relation reads are retained (`ReadCache::mark_interest`), normally a few manifests
  per rule. The run-wide decode row budget is checked against the old per-extract limit
  (`MAX_EXTRACTED_VALUES`) before the switch, so no configuration that passed fails on a budget it
  never had. As built: a file the configuration names (`source.file`, a `targets` entry) is kept
  and decoded once per run; a file a glob matches is read with `ReadCache::read_unretained` and
  decoded without `DecodeCache`, so neither the kept bytes nor the run's row budget grow with the
  matches, and the 10,000-value limit applies to both through `LimitKind::RowsPerExtractor`.
  `alint fix` clears the caches after a rule's fix lands, because creates, removes and renames
  reach the disk at once and later rules have always seen them.
- **Deleted:** the old reader, `check_equals` in `cross_file::eval`, `shadow.rs`, the `shadow-ir`
  feature, its enablement in `alint-e2e` and the corpus replay, and CI's feature-tree check for it.
  `read_rel` stays only if a non-value relation still needs it. As built, `read_rel` is gone:
  `identical`, the one relation that does not read values, still reads its two files directly,
  through `confined_rel`.

The `sync_from` value fixer (`fixers/cross_file_ops.rs`) re-extracts through
`extract_values_bounded` when it applies an edit; it is a fixer, not the relation, and stays.

## 4. Evidence for gate 3

- The e2e suite, unchanged, on the port's pull requests.
- The corpus replay against the v0.21.2 binary (ADR-0024 decision 7), comparing JSON reports with
  proposed edits and baseline fingerprints: zero differences, with the committed expectations
  file unchanged.
- The deterministic gate against v0.18.2 on S1-S3 at 1k and 10k files (at most +3% instructions,
  no read/stat-family syscall growth beyond 32 calls per family at 10k).
- Before the tag: S1-S3 x {full, changed} wall-clock at 100k (30 runs) and 1M (three runs) within
  +3% of v0.18.2, and 1M peak RSS within +10%, on the bench host with caches dropped. This needs a
  quiet window on the bench host, which the maintainer schedules.

The port's pull requests come before the behavior changes (section 5), so this evidence is
taken on a tree whose only change is the port.

## 5. Phases

Each phase is one pull request; each ends with a pointer to the next.

| Phase | Pull request | Changes behavior |
|---|---|---|
| 0 | This plan and ADR-0030 | no |
| 1 | Move the finding helpers, `source_values` and `each_target` out of `cross_file::eval`, and `check_equals` and `mismatch` into `cross_file/equals.rs`. Pure moves; the shadow keeps comparing against the old reader. | no |
| 2 | The shared reader on the relation layer, lossless `RelPath`, caches for every value rule; delete the old reader, the shadow and `shadow-ir`; the gate-2 test; gate 3 evidence except the wall-clock run. | no (gate 3) |
| 3 | Seal `FixContext`, `Config` and `CompiledNestedSpec` with constructors. | API only |
| 4 | Path-free hidden-path fingerprints with v0.21 compatibility matching. | yes |
| 5 | The dangling-symlink dry-run prediction and shared landing lookups. | yes (dry run) |
| 6 | Roadmap item for the remaining structs, docs, CHANGELOG, and the release check. | no |

If phase 2 cannot meet gate 2 or gate 3, that is recorded here with the measurements, and the
stop rule decides whether Phase 3 starts; v0.22 can still ship phases 3 to 6.

## 6. Risks

- **The 134-line bound is tight.** `check_equals` and `mismatch` are 134 lines today, so the moved
  code has no slack. The likely savings are in `check_equals`'s two near-identical source-side
  findings (no value, several values). Anything the new reader needs goes into the shared reader,
  not into `equals.rs`.
- **Reader differences outside `equals`.** The shadow proved the relation-layer reader only for
  `equals`. The set relations and `resolves` also take a glob-union source (`source.files`), where a
  missing match reports differently (`discovered`). Phase 2 adds tests for those paths before the
  switch, and the corpus replay covers the set relations it contains. Outcome: the reader tests
  (`cross_file/values_tests.rs`) cover every relation, both sides, named and matched files, and
  each way a read or decode can fail; their complete findings are byte-identical between phase 1
  and phase 2, including three cases the shadow never compared.
- **Performance.** Reading through the caches adds a map lookup and an `Arc` per resource and
  retains the bytes of every file a relation reads. The deterministic and syscall gates catch
  instruction and I/O growth; peak RSS is checked at 1M before the tag.

## 7. Status log

- 2026-11-07: plan and ADR-0030 (phase 0).
- 2026-11-07: phase 1 (#465). Pure moves. `check_equals` grew from 93 to 98 lines because its
  `pub(super)` signature wraps.
- 2026-11-07: phase 2 (#466).
  - Gate 1: met. The relation layer is the only `cross_file` value reader; the old reader,
    `shadow.rs` and `shadow-ir` are deleted.
  - Gate 2: met. `check_equals` (89) and `mismatch` (41) are 130 lines as ADR-0026 counts them;
    `equals_stays_within_the_simplicity_gate` asserts the bound of 134 and that `equals` calls
    nothing in `cross_file::eval`. Merging the two source-side cardinality findings and dropping a
    check `each_target` already makes paid for phase 1's five lines.
  - Gate 3, deterministic part: met. The e2e suite passes unchanged. The corpus replay against
    v0.21.2 shows zero differences on 106 repositories (87 `equals` rules in 49 of them; the four
    skips are the committed expectations). Against v0.18.2, instructions change by -13.1% / -13.1%
    (S1, 1k / 10k), -11.5% / -12.4% (S2) and +0.1% / -1.0% (S3), within +3%; read/stat-family
    syscalls at 10k grow by at most 29 calls per family, within 32.
  - Gate 3, wall-clock and peak RSS at 100k and 1M: the release check in phase 6.
- 2026-11-07: phase 3 (#467). The three structs are `#[non_exhaustive]`. `FixContext::new` and
  `Config::new` take the required parts, setters the rest, and the workspace builds both through
  them; `CompiledNestedSpec` is built only by `CompiledNestedSpec::compile`, as before. A test
  sets every `Config` setter, since the mutation run found none of them covered.
- 2026-11-07: phase 4 (#468). A hidden-path fingerprint is `sha256(rule, "\0hidden-path",
  "sensitive")`. `check --baseline`, `fix --baseline`, the regeneration guard and the language
  server match the v0.21 form first, then the new one, and report the new one. The end-to-end
  test checks a baseline in the v0.21 form; one the released v0.21.2 binary wrote for six such
  findings was suppressed in full by `check` and `fix`. `alint fix` still tells hidden paths
  apart within a run, through the internal path-keyed identity.
- 2026-11-07: phase 5 (#469). The dry run follows a dangling symlink into directories the create
  makes. The OS differential test covers such a symlink pointing there directly, through another
  symlink and through `..`, and a unit test holds a loop of dangling symlinks to the link limit.
  One `WalkSkips` per dry run shares the ignore, override and symlink lookups across creates
  (predicting 20 creates in a 100-file repository falls from 6.84M to 0.62M instructions).
- 2026-11-07: phase 6. The roadmap item for sealing `Violation`, `Context`, `RuleSpec` and
  `WalkOptions` before v1.0, and the Phase 2 outcome in `general-linter.md`. The corpus replay of
  main after phase 5 (53090d4a7) against v0.21.2 shows zero differences on the same 106
  repositories. Only hidden-path fingerprints were allowed to differ (ADR-0030 decision 3); the
  one corpus configuration that chooses files from the environment (`facebook/react`) is among
  the four skips, so no replayed report has a hidden-path finding.

- 2026-11-07: the release check (section 4), met. On the bench host with caches dropped, in one
  session, rustc 1.97.0 builds of main (9f8876070), v0.18.2 and v0.21.2 ran in this order: main,
  v0.18.2 and v0.21.2 at 100k, then v0.18.2 and main at 1M, then peak RSS.
  - Wall-clock medians against v0.18.2, full / changed: at 100k (30 runs) S1 -1.62% / -1.25%, S2
    -11.75% / +1.29%, S3 -0.51% / -0.43%; at 1M (one warm-up and three runs) S1 -3.12% / -4.21%,
    S2 -10.64% / -0.09%, S3 +1.37% / +0.82%. All are within +3%.
  - Against v0.21.2 at 100k, every cell is within 1.3%: S1 +0.45% / +1.16%, S2 -0.71% / -0.26%,
    S3 -0.35% / -0.49%.
  - Peak RSS of `alint check` on a persistent 1M tree, from `wait4`, the highest of each
    version's runs: S1 159,772 KB against v0.18.2's 168,224 (-5.0%), S2 113,848 against 112,280
    (+1.4%), S3 244,040 against 252,824 (-3.5%). All are within +10%, and hyperfine's
    peak-memory figures for the timed 1M cells agree.
  - S3 is bimodal on this host. In every version a sixth to a third of its runs take 6-10%
    longer at 100k (main 5 and 7 of 30, v0.18.2 7 and 5, v0.21.2 4 and 8). At 1M, two of main's
    seven S3 runs fell in the slow mode (516.6 s in the timed full cell, 507.4 s in the memory
    pass) and one of v0.18.2's seven (497.5 s), so the mean of main's S3 full cell is 3.4% above
    v0.18.2's while its median is 1.37% above. The gate is on medians, and S3's instruction count
    is within 1% of v0.18.2's (phase 2). A process sampler, every five minutes, saw nothing
    running besides the harness.
  - The corpus replay of main's tip (9f8876070, which adds adotenv 1.3.0 after the phase 5
    replay) against v0.21.2 (run 42423454717): 106 of 110 repositories replayed, 49 of them with
  `cross_file` `equals`, the same four skipped, none crashed, zero differences.
  - Gate 3 is met in full, so all three gates are met and Phase 3 may start.

- 2026-11-08: the v0.22.0 bench record (#479), reviewed.
  - `bench-gate` passes. Mean S3 full at 1M is 490.4 s against v0.21.2's 475.2 s, and S4 is
    8.83 s against 8.42 s, but their spreads (13.8 s and 0.49 s) are the bimodal and one-slow-run
    cases. The gate's `min_ms` puts S3 at +0.25% and S4 at +1.3%.
  - The release run's `micro-det` job reported `fix_throughput/trim_trailing_whitespace/100` at
    +3.16%. That was harness noise, fixed in #478. At six iterations the same build of that
    bench ranged over 3.3%, and paths of different lengths moved it by up to 0.9%. At 51
    iterations, interleaved, v0.21.2 and v0.22.0 are 0.09% apart, and the full suite against
    v0.21.2 passes with #478's harness. That bench is -0.30%, and `/1000` +0.16%.
  - The record's same-session criterion A/B, which runs v0.21.2 first, showed
    `cross_file/for_each_dir/100` at +24% (and `/5000` at +10%), and
    `output_formats/10000_violations/gitlab` at +13%. Their instruction counts are within
    0.1%, and neither `for_each_dir.rs` nor `alint-output` changed. An interleaved A/B on the
    bench host (three ABBA rounds of rustc 1.97.0 builds) puts them at +0.4%, +2.7% with
    overlapping ranges, and +0.4%.
  - The CHANGELOG's dry-run figure was measured in one repository, where prediction was 45% of
    the run. Two more 100-file repositories (one a git repository) with rustc 1.97.0 release
    builds give 85% and 89% fewer prediction instructions at 20 creates, but 9% and 15% fewer in
    all, so the CHANGELOG now gives the ranges. A single create costs the same in each.

Next: v0.23, general-linter Phase 3 (`version: 2` selectors and report-only constraints), with a
design doc and the version-boundary ADR first; v0.23 also stops matching the v0.21 hidden-path
fingerprint (ADR-0030 decision 1). The npm package's move to per-platform binaries is its own
release after v0.22.0 (ADR-0030).
