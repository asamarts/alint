# Benchmark scenario consolidation (14 -> 5)

Status: **APPROVED** (D1-D5 settled 2026-09-28) + adversarially audit-verified
(3-agent pass: zero kinds dropped, all constraints + the glob bug confirmed).
Ready to implement. Date: 2026-09-28.

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
The fix engine over a tree of fixable violations, run as **`alint fix --dry-run`**
(D2). One scenario exercising **all 24 NON-SPAWNING fix ops** -- the full 26 in
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
3. **`det_check.rs` `include_str!` of s1/s2/s6/s7/s12/sfix_trim**: repoint the
   deterministic Ir gate's cells to the new files (S1 layout, S2 content, S3
   relational, and the new fix scenario for the `fix_grp` cell). Keeps the Callgrind
   gate aligned with the new axes.
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
   the **"112 cells" (14x4x2)** count -> **40 (5x4x2)** (`bench-record.yml:271`,
   `CHANGELOG.md:1334`). Fix the already-stale **"S5 = the only --fix bench"** claim,
   which lives in 3 concrete places: `render-history.py:199`, generated
   `docs/benchmarks/HISTORY.md:201`, and `docs/benchmarks/macro/README.md:32`.

## Decisions (APPROVED 2026-09-28)

asamarts approved all five recommendations. They are settled; implementation follows
them.

- **D1 -- Numbering & history transition: APPROVED (a) -- reuse `s1`..`s4` +
  `sfix_all`** with new (broader) meanings. The trajectory system is append-only, so
  a REDUCTION is a re-baseline; each new sN is a strict SUPERSET of the old sN's
  axis, so the HISTORY row stays semantically continuous. Add a documented
  "re-baselined at vX.Y" note (constraint 5 keeps the old ids known to
  render-history). (Rejected: fresh `s20`+ ids -- clean append but odd for "5
  scenarios".)
- **D2 -- Fix scenario mode: APPROVED `fix --dry-run`** (add a `Mode::Fix` that runs
  it). It exercises collect + compute + compose + verify (the engine-heavy work) and
  is idempotent across hyperfine iterations -- no per-iteration re-materialization,
  matching the micro `fix_throughput`. The write-back cost is I/O-bound and less
  interesting for an engine benchmark, and is covered by the `apply` correctness
  corpus. (Rejected: real `fix` -- mutates the tree, needs re-materialization +
  adds noise.)
- **D3 -- S4 tree type: APPROVED polyglot + git combined** (a new
  `requires_git_repo && requires_polyglot_tree` capability in `run.rs`), so one
  scenario carries the full realism. (Rejected: regular+git only -- loses S9's
  scope_filter-at-scale shape.)
- **D4 -- Historical comparability: APPROVED -- keep the 14 old result dirs as-is**
  (render-history shows them retiring); the consolidation release publishes only the
  5. Cross-version comparison of an old axis reads the old sN row up to the
  re-baseline and the new sN row after. Document the cutover in HISTORY.md +
  `bench-coverage.md`.
- **D5 -- Spawn fix ops in S5fix: APPROVED -- EXCLUDE `command` + `git_untrack`**
  (they spawn a subprocess per violation, making the fix bench noisy/slow and
  measuring git / the command, not alint; correctness-gated elsewhere). So S5fix
  exercises the **24 non-spawning ops** (see the S5(fix) list). (Rejected: a tiny
  fixed-count `true` command to measure spawn overhead.)

## Implementation sequence (approved; ready to build)

1. Fix the `bench-record.yml` glob (constraint 7) -- standalone, lands first.
2. Author the 5 new scenario YAMLs; delete the 14 old (det_check repoints).
3. Rewrite the `Scenario` enum AND its use-sites -- `tools.rs` (the exhaustive
   match + the competitive-tool gating + `debug_assert`s) and `run.rs:31`
   (quick-mode default); add `Mode::Fix` + the git+polyglot combo; update the 3 enum
   unit tests. Re-confirm the ls-lint/grep/repolinter gating still targets the
   intended (now-broadened) scenario.
4. Repoint `det_check.rs`; update the `facts.rs` `14` literal + regen `facts.json`.
5. Update `render-history.py` (retire old ids, add new, repoint cell_keys / anchor);
   update the trajectory assert.
6. Update the docs + strings (constraint 8) -- incl. `pull_request_template.md`, the
   "112 cells" -> "40 cells" count, and the "S5 = only --fix bench" claim in its 3
   places -- and fix the pre-existing stale SOURCE comments (`mod.rs:223` calls S5
   "fix-pass throughput" though the harness can't fix; `fix_throughput.rs` docstring
   claims 3 ops, only 2 are wired).
7. `cargo test --workspace` + a `xtask bench-scale --scenarios <new> --sizes 1k
   --modes full,changed,fix --json-only` smoke on kbench; verify `bench gate` skips
   the (baseline-less) new cells cleanly.

## Risks / rollback

- **Historical trajectory continuity** (D1/D4): the main risk. Mitigated by keeping
  old ids known to render-history and documenting the re-baseline.
- **First-release gating gap**: brand-new scenario ids have no baseline mate, so
  `bench gate` skips them on the consolidation release (expected; noted in
  `bench-coverage.md`'s Phase-7 prerequisite).
- **Rollback**: the 14 YAMLs + enum are recoverable from git; the change is
  additive-then-subtractive in one PR, revertable wholesale.
