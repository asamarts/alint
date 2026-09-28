# Benchmark scenario consolidation (14 -> 5)

Status: **APPROVED + audit-verified across 2 adversarial rounds (5 agents).** Design
sound (zero kinds dropped); all constraints + the live glob bug confirmed; round-2
implementation-readiness gaps (Mode::Fix wiring, mode x scenario gating, the missing
git+polyglot generator, the `sfix_all` fixture, the release-pipeline modes, the
`cell_keys`/`schema_version` impact) folded into the sequence. **D1/D4 RESOLVED:** the
5 new scenarios become the DEFAULT/MAIN series; the existing 14 move to a LEGACY page
AFTER the new set is built, run, and verified. **Phase A (build) COMPLETE +
independently gate-verified** (commits 43561695 / 831879a3 / 3d9570e3: 5 YAMLs,
`Mode::Fix`, git+polyglot generator, the 24-op `sfix_all` fixture; fmt / workspace
clippy / `cargo test --workspace` / release build / `gen-facts --check` all green +
a local `bench-scale SFIX` fix smoke). **Phase B (kbench run: current + past
versions) + Phase C (legacy migration) remain.** Date: 2026-09-28.

## Goal

Reduce the `bench-scale` macro scenario set from **14** (`s1`..`s14`) to **5**:
four scenarios that still cover every performance axis of the current set, plus one
**dedicated auto-fix scenario**. The fix engine has **zero** macro coverage today
(every scenario runs `alint check`; only 2 of 26 fix ops are exercised, in the
criterion micro layer). Net: fewer, sharper scenarios and a real fix benchmark.

Non-goals: touching the criterion micro-benches (`crates/alint-bench/benches/*.rs`)
or the legacy `bench-release` / `bench_config.yml` smoke. The "scenarios" here are
the `bench-scale` YAMLs only.

## Current state (audit summary)

- **Three benchmark systems**; only `bench-scale` has "scenarios": 14 `sN_*.yml`
  plus `sfix_trim.yml` (the latter is not in the `Scenario` enum -- only
  `det_check.rs` `include_str!`s it).
- **Every scenario runs `alint check <tree>` / `... --changed`**
  (`xtask/src/bench/tools.rs`; the `Mode` enum is only `Full`/`Changed`). No
  `--format`, no `fix`. S5's `fix:` blocks are inert under the bench-scale harness;
  `sfix_trim`'s `fix:` is inert in bench-scale too but ACTIVE in `det_check.rs` (its
  sole purpose -- `alint fix --dry-run`, the deterministic Ir fix gate). **Auto-fix
  is unbenchmarked at the macro (wall-clock scenario) layer.**
- Uniform tree sizes (1k/10k/100k/1m); tree TYPE varies: regular / git (S8) /
  polyglot (S9, S10).

## The 4 performance axes the 14 scenarios cover

| Axis | Engine hot path | Current scenarios |
|------|-----------------|-------------------|
| **A. Layout / path** | walker + GlobSet + path-only rules (little/no content read) | S1, S10, S2-existence, S4-partial |
| **B. Per-file content** | per-file dispatch fan-out; content read + per-line kinds; per-file structured | S5, S6, S2-content, S4-content, S12 |
| **C. Cross-file / graph** | whole-index build, path-index, relational, graph, single-shot spawn | S7, S11, S13, S14, S3-crossfile |
| **D. Workspace realism** | `extends:` chains, `nested_configs`, polyglot, git overlay (the release anchor) | S3, S8, S9 |

Auto-fix (collect + compute + compose + verify) is a **fifth axis with no macro
coverage today**.

## Proposed 5 scenarios

### S1 -- Layout & path (walk-bound)
Axis A. The cheapest path; the ls-lint / grep competitive anchor (old S1's role).
Kinds: `filename_case`, `filename_regex`, `file_exists`, `file_absent`,
`dir_absent`, `file_max_size`, `no_empty_files`, `no_symlinks`; a subset wrapped in
`scope_filter:` (folds in S10's v0.9.8 shape). Regular tree.
Absorbs: S1, S10, S2's existence/size rules, S4's absence rules.

### S2 -- Per-file content
Axis B. Per-file dispatch fan-out + content read. Kinds: the 13 content kinds
(`final_newline`, `no_trailing_whitespace`, `no_bidi_controls`,
`no_zero_width_chars`, `no_bom`, `line_endings`, `indent_style`, `line_max_width`,
`max_consecutive_blank_lines`, `file_max_lines`, `file_min_lines`, `file_is_text`,
`file_is_ascii`) + `file_content_forbidden` + per-file structured (`ordered_block`,
`import_gate`, `xml_path_matches`, `xml_path_equals`). Regular tree + a `.csproj`
overlay (for xml). Absorbs: S5, S6, S2's content rules, S4's content rules, S12.

### S3 -- Cross-file, relational & graph
Axis C. Whole-index + path-index + graph build + single-shot spawn. Kinds:
`pair`, `unique_by`, `for_each_dir`, `for_each_file`, `every_matching_has`,
`dir_only_contains`, `registry_paths_resolve`, `cross_file_value_equals`,
`pair_hash`, `cross_file`, `for_each_match`, `file_graph` x3 (no_dangling
from_content + derive_target + acyclic), `generated_file_fresh`,
`command_idempotent`. Regular tree + overlays: `manifest.sha256` (for `pair_hash`;
NOT `registry_paths_resolve`, which reads `Cargo.toml`), `.gff_target` (for
`generated_file_fresh`), and `.v012_editions` + a 24-node cyclic `graph/` subtree
(for `cross_file`/`set_equals` + `file_graph`). Absorbs: S7, S11, S13, S14.

### S4 -- Workspace bundle (realistic; release anchor)
Axis D. `extends:` the bundled rulesets (oss-baseline + rust + node + python +
monorepo + cargo-workspace) + `nested_configs: true` + git overlay
(`git_no_denied_paths`, `file_exists` `git_tracked_only`), on a **polyglot + git**
tree. The realistic mixed workload; the release anchor. Absorbs: S3, S8, S9.
(Requires the harness to support one scenario that needs BOTH a git repo AND the
polyglot tree -- see D3.)

### S5(fix) -- Auto-fix (dedicated; NEW axis)
The fix engine over a tree of fixable violations, run as
**`alint fix --unsafe-fixes --dry-run`** (D2: `--unsafe-fixes` so every op's
compute+compose path runs rather than the Unsafe ops downgrading to compute-only
suggestions; `--dry-run` so nothing writes and the tree stays byte-stable across
hyperfine iterations). One scenario exercising **all 24 NON-SPAWNING fix ops** -- the full 26 in
`FixSpec::ALL_OP_NAMES` minus `command` + `git_untrack`, which spawn a subprocess
and are excluded per D5 (they would measure git / the command, not alint):
- whole-file normalizers: `file_trim_trailing_whitespace`, `file_append_final_newline`, `file_normalize_line_endings`, `file_collapse_blank_lines`, `file_strip_bidi`, `file_strip_zero_width`, `file_strip_bom`
- inline content (ruleset-authored bytes): `file_create` (file_exists), `file_prepend` (file_header), `file_append` (file_content_matches / file_footer)
- located: `replace`, `sort`, `indent_style`, `insert_line`, `insert_header`
- structured: `set_value`, `remove_value`
- path / metadata: `file_rename` (filename_case), `chmod` (executable_bit), `file_remove`, `dir_create`, `relocate`
- cross-file: `sync_from`, `create_and_register`
- compose: several fix rules over the same `**/*.rs` so the compose buffer + fixpoint are exercised

Closes the auto-fix macro gap (was 2/26 ops, micro-only). Each op's host rule kind
is fixed and known, so the scenario is authorable; the full 26-op -> host map is in
the implementation notes. `command` + `git_untrack` (the 2 spawn ops) are
deliberately absent (D5).

## Coverage proof (every current kind family survives)

Every distinct kind in the 14 maps to at least one new scenario (path -> S1,
content / per-file structured -> S2, cross-file / graph / single-shot -> S3,
bundle / git / polyglot -> S4, all fix ops -> S5fix). The soft
`coverage_audit_bench_listing.rs` "uncovered kinds" list is NOT expected to grow for
any kind covered before (it may shift which scenario covers it). Granularity is
intentionally coarser: a regression now localizes to an AXIS rather than a
sub-scenario, which is the point of the consolidation.

**Verified (3-agent adversarial audit, 2026-09-28): ZERO kinds dropped** -- all 44
distinct kinds in the 14 old scenarios + the 6 bundled rulesets map to at least one
new scenario. CAVEAT: three kinds -- `toml_path_matches`, `file_min_size`,
`no_merge_conflict_markers` -- survive ONLY through S4's `extends:` (they appear in
no explicit S1/S2/S3 kind list), so S4 must keep the full 6-ruleset extends-set
(oss-baseline + rust + node + python + monorepo + cargo-workspace) or those three
drop.

("Absorbs" tracks where each old scenario's CHECK kinds move; a split old scenario
spans two new ones -- e.g. old S14's `ordered_block` -> S2, its graph/cross-file ->
S3 -- and old S5's fix ASPECT moves to S5fix while its check kinds move to S2.)

## The 8 lockstep constraints and how each is handled

1. **`scenarios/*.yml`**: replace the 14 with the 5 files (naming per D1).
2. **`Scenario` enum (`xtask/src/bench/mod.rs`) + ALL its use-sites + its 3 unit
   tests**: rewrite the arms (`parse`/`label`/`description`/`config_yaml`/`all`/
   `requires_polyglot_tree`/`requires_git_repo`/`setup_overlay`/`teardown_overlay`)
   for the 5 (the 14 YAMLs are also `include_str!`'d at `mod.rs:44-57`). S4 needs
   `requires_git_repo && requires_polyglot_tree` (harness change, D3). S5fix needs a
   new `Mode::Fix` path (D2). Update the 3 tests `every_scenario_yaml_loads...`,
   `all_covers_every_parsed_label`, `overlay_hooks_are_idempotent`.
   **HARD compile + SEMANTIC coupling the enum rewrite hits (audit-added):
   `xtask/src/bench/tools.rs`** exhaustively matches `Scenario::S1..S14` (won't
   compile with the deleted variants) AND hardcodes the competitive-tool gating --
   `(LsLint, S1)`, `(GrepPipeline, S1|S2)`, `(Repolinter, S2)` + `debug_assert_eq!`
   -- so under D1's broadened S1/S2 the ls-lint/grep/repolinter comparisons silently
   re-gate; re-confirm each still targets the intended scenario. `run.rs:31`
   (`args.scenarios = vec![Scenario::S1]`, the quick-mode default) is also a
   use-site to update.
3. **`det_check.rs` -- a BROADER rewrite than a repoint (G-note):** it
   `include_str!`s s1/s2/s6/s7/s12/sfix_trim (`:38-64`). Repoint the deterministic Ir
   gate's `check` cells to the new files (s1 layout, s2 content, s3 relational) --
   which means rewriting BOTH the `const S6/S7/S12 = include_str!` AND the
   `#[bench::s6_*/s7_*/s12_*]` attribute lines (`:163-171`), not just the paths --
   and repoint the `fix_grp` cell from `sfix_trim.yml` to `sfix_all` (then delete
   `sfix_trim.yml`). Keeps the Callgrind gate aligned with the new axes.
4. **`facts.rs` `14` literal + `facts.json` `bench_scenario_rule_counts`**: change
   the expected count; regenerate `facts.json` (`xtask gen-facts`, drift-gated). The
   count is over `s<N>_*.yml` (excludes `sfix`-named), so a `sfix_all.yml` scenario
   is excluded from the numbered count (like `sfix_trim` today).
5. **`render-history.py` `SCENARIOS` / `FIRST_VERSION` / `cell_keys` + recurrence
   guard**: the guard fires if a scenario id in ANY committed `results.json` is not
   in `SCENARIOS`. Historical dirs contain S1-S14, so `SCENARIOS` must **retain all
   14 historical ids** (marked retired) AND add the new ids. `FIRST_VERSION` gets
   the new ids at the consolidation release. `cell_keys` / the HISTORY headline
   repoint to the new anchors (constraint 6).
6. **`s3_1m_full` trajectory assert (`coverage_audit_benchmarks_trajectory.rs` +
   `render-history.py`)**: repoint the "newest publish has this cell" anchor to the
   new workspace scenario at `1m`/`full`.
7. **`bench-record.yml` scenario-derivation glob -- a LIVE BUG (reproduced).** The
   `ls xtask/src/bench/scenarios/s*.yml` glob (`bench-record.yml:310`; the `sed`
   extraction is `:311`) now also matches `sfix_trim.yml`, whose name has no digit
   after `s`, so `sed` passes the raw path through and `sort -n` puts it FIRST.
   Reproduced verbatim, the derived list is
   `Sxtask/src/bench/scenarios/sfix_trim.yml,S1,S2,...,S14` -- that first bogus token
   reaches `Scenario::parse`, which `bail!`s "unknown scenario", so the next
   tag-push bench-record run FAILS. Fix the glob to `s[0-9]*_*.yml` (excludes any
   `sfix`-named file) and pass the fix scenario explicitly. Must be fixed regardless
   of the consolidation. (`facts.rs`'s stricter `^s(\d+)_` regex is already immune,
   which is why its `14` count is correct.)
8. **Docs + strings enumerating S1-S14** (audit-expanded list): `docs/benchmarks/
   METHODOLOGY.md`, `docs/benchmarks/RUNNING.md`, `docs/benchmarks/README.md`,
   `RELEASING.md`, `docs/design/deterministic-perf-gating.md`, `bench-record.yml`
   (comments + the runtime commit/PR strings at ~:446/:461),
   **`.github/pull_request_template.md:25`** (hardcodes `--scenarios S1,S6,S7`, which
   would `bail!` post-consolidation), the per-run READMEs under
   `docs/benchmarks/investigations/` + `docs/benchmarks/macro/results/`, and code
   comments (`xtask/src/facts.rs:322`, `xtask/src/bench/mod.rs:194,569`). Also update
   the **"112 cells" (14x4x2)** count -> **36** (4 check scenarios x 4 sizes x 2
   modes + `sfix_all` x 4 sizes x 1 fix mode; NOT 40 -- the fix scenario runs one
   mode, per D2/D5/G2/G4) (`bench-record.yml:271`, `CHANGELOG.md:1334`). Fix the already-stale **"S5 = the only --fix bench"** claim,
   which lives in 3 concrete places: `render-history.py:199`, generated
   `docs/benchmarks/HISTORY.md:201`, and `docs/benchmarks/macro/README.md:32`.

## Decisions (APPROVED 2026-09-28)

asamarts approved all five recommendations. They are settled; implementation follows
them.

- **D1 -- Numbering, main series & legacy migration: RESOLVED (asamarts; supersedes
  the earlier "reuse ids in one series").** Keep the `s1`..`s4` + `sfix_all` ids, but
  make the 5 NEW scenarios the **DEFAULT/MAIN** benchmark series and **move the
  existing 14-scenario benchmarks + their published results to a LEGACY / historical
  path + page.** This resolves the two round-2 blockers that killed the original
  same-series id-reuse plan: (G6) new-S3 is the *cross-file* axis while old-S3 was the
  *workspace* axis, so reusing the id in ONE series would splice incomparable numbers
  into one HISTORY row (even forward-only); and (E) `render-history` keys on
  `(version, scenario, size, mode)` last-write-wins, so backfilling new scenarios onto
  past versions would collide with the old ids. With old + new in SEPARATE paths
  nothing collides or mixes -- old-S3 (legacy) and new-S3 (main) are unambiguous, and
  the new-vs-old gate mis-pairing (E's Q5) cannot arise (the main series has no
  old-axis rows). The new main series is backfilled across past versions (see
  **Past-version backfill**); the legacy series is frozen. **Sequence: BUILD the new
  set -> RUN/benchmark it -> verify it's good -> THEN move the old to legacy** (never
  tear down the proven-good old set before the new one is validated).
- **D2 -- Fix scenario mode: APPROVED `fix --dry-run`; built as
  `fix --unsafe-fixes --dry-run`** (add a `Mode::Fix` that runs it -- `--unsafe-fixes`
  forces every op through compose so the fix bench measures all 24, not just the Safe
  subset; refinement applied during the Phase A build). It exercises collect + compute + compose + verify (the engine-heavy work) and
  is idempotent across hyperfine iterations -- no per-iteration re-materialization,
  matching the micro `fix_throughput`. The write-back cost is I/O-bound and less
  interesting for an engine benchmark, and is covered by the `apply` correctness
  corpus. (Rejected: real `fix` -- mutates the tree, needs re-materialization +
  adds noise.)
- **D3 -- S4 tree type: APPROVED polyglot + git combined** (a new
  `requires_git_repo && requires_polyglot_tree` capability in `run.rs`), so one
  scenario carries the full realism. (Rejected: regular+git only -- loses S9's
  scope_filter-at-scale shape.)
- **D4 -- Historical comparability: RESOLVED via the legacy page (see D1).** The old
  S1-S14 series + its results move to a frozen LEGACY path + HISTORY page (kept for
  reference, never re-run). The new 5-scenario MAIN series carries the go-forward
  history AND is backfilled across past versions (ragged; see Past-version backfill),
  so it has its own cross-version trajectory from day one. No old/new mixing to
  reconcile -- the two series are separate paths. Document the split + a legacy
  pointer in the new HISTORY + `bench-coverage.md`.
- **D5 -- Spawn fix ops in S5fix: APPROVED -- EXCLUDE `command` + `git_untrack`**
  (they spawn a subprocess per violation, making the fix bench noisy/slow and
  measuring git / the command, not alint; correctness-gated elsewhere). So S5fix
  exercises the **24 non-spawning ops** (see the S5(fix) list). (Rejected: a tiny
  fixed-count `true` command to measure spawn overhead.)

## Past-version backfill (the new MAIN series across releases)

The harness has **no version selector** -- `Tool::Alint` always builds + runs the
CURRENT checkout (`run.rs:39` -> `build_release_binary`; the version is derived from
the workspace `Cargo.toml`, not chosen). `--docker` is NOT a past-version mechanism
(it builds alint from the mounted current source; it only pins competitor + toolchain
versions). So backfilling = for each past tag: checkout, graft the new bench
subsystem, build that tag's alint, run the new scenarios, write into the MAIN series.
This mirrors the v0.10-v0.13 manual-backfill precedent (`bench-host-migration.md`
step 3), which grafted only `run.rs`; here the graft is larger (the whole new
`xtask/src/bench/` tree + the 5 YAMLs + `Mode::Fix` + the git-polyglot generator),
and `--scenarios` must be passed explicitly (the old ref's `s*.yml` glob emits the
old ids). The backfill writes to the new series' **own fresh path**, so it never
collides with the still-present old results (which move to legacy only in Phase C).

**The matrix is RAGGED (feature availability -- verified against real tags):**
- **`sfix_all`: consolidation release onward ONLY.** Its 24 ops are the unreleased
  v0.17 arc (`insert_header`/`create_and_register` absent even at v0.16.1). No
  pre-release fix history is possible.
- **`s3_relational`: v0.12.0+** (`file_graph`/`cross_file`/`for_each_match` land at
  v0.12; absent at v0.10/v0.11).
- **`s2_content`: v0.10.0+** (`xml_path_*`/`ordered_block`/`import_gate` are v0.10).
- **`s1_layout`: the whole kbench series** (v0.10.0+; the kinds are ancient).
- **`s4_workspace`: runs from ~v0.10 but is a CROSS-VERSION CONFOUND** -- `extends:`
  pulls the bundled rulesets shipped IN that binary, whose rule set evolved per
  release, so it measures a moving target. Flag it as such (or pin a fixed inline
  ruleset for comparability -- a follow-up).
- An old tag whose alint lacks a declared kind/op `bail!`s at load; skip that (tag,
  scenario) cell -- it renders `n/a` via each id's `FIRST_VERSION` floor.

**Universe:** the canonical kbench series starts at **v0.10.0** (pre-v0.10 is on the
retired 3900X arch, not comparable); 11 published dirs (v0.10.0-.2, v0.11.0, v0.12.0,
v0.13.0, v0.14.0-.2, v0.15.0, v0.16.0). Pins: host `kbench`, rustc **1.97.0**,
`ALINT_BENCH_DROP_CACHES=1`, `TMPDIR=/bench`. `bench gate` skips baseline-less cells
(`gate.rs:161`), so a fresh backfilled series does not false-fail.

## Legacy migration (Phase C -- only after the new set is proven)

Make the 5 new scenarios the default; move the existing 14-scenario benchmarks +
results to a frozen legacy home. Because the new MAIN series already wrote to its own
fresh path in Phase B, Phase C is a relocation of the OLD data + a page/pointer swap,
not a data move of the new (and never a backfill into a dir holding old data --
`results.json` is overwritten, not merged, `output.rs:22`).
- **Results:** relocate the old `docs/benchmarks/macro/results/<arch>/v*/` (S1-S14
  data) to a legacy path (e.g. `.../results/legacy/<arch>/`).
- **HISTORY page:** render the old series ONCE into a frozen legacy HISTORY page (a
  static snapshot or `render-history` against the legacy path); the new MAIN HISTORY
  (5 scenarios, backfilled) becomes the default page. Cross-link the two.
- **render-history recurrence guard:** with legacy + main in SEPARATE paths, each
  render only sees its own ids, so the guard (`measured` subset of `SCENARIOS`) is
  satisfied without carrying the old ids in the main `SCENARIOS`. Add `FIRST_VERSION`
  floors for the 5 new ids (per the ragged matrix).
- **Site + `trajectory.json` (G5):** repoint `cell_keys` to the new main anchors
  (e.g. `s4_workspace_1m_full`), bump `schema_version` 1->2 (the module's own rule),
  update the `== 1` pin in `coverage_audit_benchmarks_trajectory.rs:91` + the
  `s3_1m_full` anchor -> the new main cell, and point `benchmarks.astro` at the new
  series with a legacy link (a coordinated alint.org site-repo edit).
- **Scenario YAMLs:** only NOW delete the old 14 from the active path (they remain in
  git history + the legacy results).

## Implementation sequence (approved; ready to build)

Phased per D1: **BUILD -> RUN/verify -> MIGRATE old to legacy (last).**

### Phase A -- build the new harness (on `main`, one PR) -- DONE (43561695/831879a3/3d9570e3)
1. **Glob bugfix (constraint 7), standalone first:** `bench-record.yml:310` glob ->
   `s[0-9]*_*.yml`; inject the fix scenario id into `--scenarios` (step 9).
2. **Author the 5 scenario YAMLs** (`s1_layout`, `s2_content`, `s3_relational`,
   `s4_workspace`, `sfix_all`). KEEP the old 14 for now (they move to legacy in
   Phase C, AFTER the new set is proven -- do NOT delete them yet).
3. **`Mode::Fix` (D2) -- the mechanics round-2 flagged (G1/G2), or fix mode silently
   benchmarks `check`:**
   - `tools.rs` `invocation()` is an if/else (`:174`), NOT a match -- add a
     `Mode::Fix => format!("{bin} fix {root} --unsafe-fixes --dry-run")` arm.
   - `Tool::supports()` (`:92`) is `(Alint,_,_) => true` -- NARROW so only (check
     scenarios x `Full|Changed`) and (`sfix_all` x `Fix`) run; else S1-S4 run in fix
     mode and `sfix_all` in check mode = junk rows.
   - add the `Fix` variant to the `Mode` enum + the `--modes` CLI parse.
4. **`Scenario` enum + ALL use-sites:** rewrite the arms in `mod.rs` (incl. the
   `include_str!` block `:44-57`) + the 3 unit tests; rewrite `tools.rs`'s exhaustive
   `Scenario::S1..S14` match + the competitive-tool gating (`(LsLint,S1)`,
   `(GrepPipeline,S1|S2)`, `(Repolinter,S2)` + `debug_assert`s), re-confirming each
   still targets the intended broadened scenario; fix `run.rs:31` quick-mode default.
5. **git+polyglot tree (D3) -- the MISSING generator (G8):** `run.rs` tree-selection
   is polyglot-XOR-git and there is no git-polyglot generator. Add
   `generate_git_nested_polyglot_monorepo` (or a `git: bool` on the polyglot
   generator) in `crates/alint-bench/src/tree.rs`, and rework `run.rs:143-152` so S4
   (`requires_git_repo && requires_polyglot_tree`) gets a git-initialised polyglot
   tree for ALL modes (not only `changed`).
6. **`sfix_all` fixture (G7) -- the crux:** the base synthetic tree has ~no fixable
   violations, so `fix --unsafe-fixes --dry-run` would measure "scan, find nothing".
Author a
   fixture/overlay (extend `det_check`'s `materialize_fixable` idea) that plants a
   violation for EACH of the 24 non-spawn ops.
7. **`det_check.rs` (constraint 3 -- broader than a repoint):** rewrite the `check`
   bench group -- the `const S6/S7/S12 = include_str!` AND the `#[bench::s6_*/...]`
   attribute lines -- to the new files (s1/s2/s3); repoint the `fix_grp` cell from
   `sfix_trim.yml` to `sfix_all`; delete `sfix_trim.yml`.
8. **`facts.rs`:** change the `14` literal (-> 4 numbered; `sfix_all` is excluded like
   `sfix_trim`) + regen `facts.json` (`xtask gen-facts`).
9. **Release pipeline (G3):** `bench-record.yml:317` `--modes full,changed` ->
   `full,changed,fix`; ensure `--scenarios` includes `sfix_all` (the numeric glob
   won't add it).
10. **Docs/strings (constraint 8)** incl. `pull_request_template.md`, the cell count
    **36 (4x4x2 + 1x4x1), NOT 40** (G4), the "S5=only-fix" claim's 3 places,
    `bench-coverage.md:157`'s runnable `--scenarios S1..S13`, `RELEASING.md:141/311`,
    and the stale source comments (`mod.rs:223`, `fix_throughput.rs` docstring).
11. Gate: `cargo test --workspace` + `cargo build --release -p alint -p xtask` green;
    `xtask gen-facts --check` clean.

### Phase B -- run + verify on kbench (current + past)
12. Smoke on kbench (`ALINT_BENCH_DROP_CACHES=1`, `TMPDIR=/bench`, rustc 1.97.0):
    `xtask bench-scale --scenarios s1,s2,s3,s4,sfix_all --sizes 1k --modes
    full,changed,fix --json-only`. Confirm each scenario/mode makes sane rows +
    `bench gate` skips the baseline-less new cells.
13. Full current-version run (1k/10k/100k/1m) into the MAIN series path.
14. **Past-version backfill** into the MAIN series per the ragged floors (see the
    Past-version backfill section).

### Phase C -- migrate old to legacy (only AFTER Phase B verifies good)
15. Per the **Legacy migration** section: move the old S1-S14 results to the legacy
    path, freeze/render the legacy HISTORY page, repoint `render-history` + the site
    (`benchmarks.astro`, `cell_keys`, `trajectory.json`, `schema_version` 1->2 -- G5)
    to the new MAIN series, and repoint the `s3_1m_full` anchor
    (`coverage_audit_benchmarks_trajectory.rs`) to the new main workspace cell.

## Risks / rollback

- **Old/new axis mixing + backfill collision (was the top risk): RESOLVED** by the
  legacy-page split (D1) -- old S1-S14 and new s1-s4 live in SEPARATE paths, so
  `render-history` never mixes or collides them, and the cutover gate cannot mis-pair
  new-vs-old axis (E's Q5). No id-reuse-in-one-series hazard remains.
- **First-run gating:** the new MAIN series has no baseline mate on its first run, so
  `bench gate` skips its cells (`gate.rs:161`, expected); after the past-version
  backfill the new scenarios gain their own (all-new-axis) cross-version mates.
- **`s4_workspace` cross-version confound:** its `extends:` measures each binary's
  own evolving bundled rulesets -- a moving target across versions (see Past-version
  backfill); flagged, and pinning a fixed inline ruleset is a follow-up.
- **Rollback:** the old 14 YAMLs + enum are in git history; Phase A is one PR
  (revertable wholesale) and Phase C's legacy move is a reversible relocation. The
  old set is never torn down until Phase B proves the new set good.
