# Benchmark scenario consolidation (14 -> 5)

Status: **PROPOSAL** (awaiting decisions D1-D5). Date: 2026-09-28.

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
  (`xtask/src/bench/tools.rs`). No `--format`, no `fix`. S5's and `sfix_trim`'s
  `fix:` blocks are inert under the harness. **Auto-fix is unbenchmarked at the
  macro layer.**
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
`command_idempotent`. Regular tree + overlays (`manifest.sha256`, `.gff_target`,
24-node cyclic `graph/` subtree). Absorbs: S7, S11, S13, S14.

### S4 -- Workspace bundle (realistic; release anchor)
Axis D. `extends:` the bundled rulesets (oss-baseline + rust + node + python +
monorepo + cargo-workspace) + `nested_configs: true` + git overlay
(`git_no_denied_paths`, `file_exists` `git_tracked_only`), on a **polyglot + git**
tree. The realistic mixed workload; the release anchor. Absorbs: S3, S8, S9.
(Requires the harness to support one scenario that needs BOTH a git repo AND the
polyglot tree -- see D3.)

### S5(fix) -- Auto-fix (dedicated; NEW axis)
The fix engine over a tree of fixable violations, run as **`alint fix --dry-run`**
(see D2). One scenario exercising **all 26 fix ops** across every regime:
- whole-file normalizers: `file_trim_trailing_whitespace`, `file_append_final_newline`, `file_normalize_line_endings`, `file_collapse_blank_lines`, `file_strip_bidi`, `file_strip_zero_width`, `file_strip_bom`
- located: `replace`, `sort`, `indent_style`, `insert_line`, `insert_header`
- structured: `set_value`, `remove_value`
- path / metadata: `file_rename` (filename_case), `chmod` (executable_bit), `file_create`, `file_remove`, `dir_create`, `relocate`
- cross-file: `sync_from`, `create_and_register`
- compose: several fix rules over the same `**/*.rs` so the compose buffer + fixpoint are exercised
- spawn (`command`, `git_untrack`): see D5.

Closes the auto-fix macro gap (was 2/26 ops, micro-only).

## Coverage proof (every current kind family survives)

Every distinct kind in the 14 maps to at least one new scenario (path -> S1,
content / per-file structured -> S2, cross-file / graph / single-shot -> S3,
bundle / git / polyglot -> S4, all fix ops -> S5fix). The soft
`coverage_audit_bench_listing.rs` "uncovered kinds" list is NOT expected to grow for
any kind covered before (it may shift which scenario covers it). Granularity is
intentionally coarser: a regression now localizes to an AXIS rather than a
sub-scenario, which is the point of the consolidation.

## The 8 lockstep constraints and how each is handled

1. **`scenarios/*.yml`**: replace the 14 with the 5 files (naming per D1).
2. **`Scenario` enum (`xtask/src/bench/mod.rs`) + its 3 unit tests**: rewrite the
   arms (`parse`/`label`/`description`/`config_yaml`/`all`/`requires_polyglot_tree`/
   `requires_git_repo`/`setup_overlay`/`teardown_overlay`) for the 5. S4 needs
   `requires_git_repo && requires_polyglot_tree` (harness change, D3). S5fix needs a
   new "mode = fix" path (D2). Update `every_scenario_yaml_loads...`,
   `all_covers_every_parsed_label`, `overlay_hooks_are_idempotent`.
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
7. **`bench-record.yml` scenario-derivation glob (currently BROKEN by
   `sfix_trim.yml`; `bench-record.yml:311`)**: fix the glob to `s[0-9]*_*.yml` so it
   never picks up a `sfix`-named file, and pass the fix scenario explicitly. This is
   a live bug the consolidation must fix regardless.
8. **Docs enumerating S1-S14** (`docs/benchmarks/macro/README.md`,
   `METHODOLOGY.md`, `RUNNING.md`, `README.md`, `RELEASING.md`, `bench-record.yml`
   comments, `deterministic-perf-gating.md`): update to the 5, and fix the
   already-stale "S5 = the only --fix bench" claim.

## Open decisions (recommendations)

- **D1 -- Numbering & history transition.** The trajectory system is append-only; a
  REDUCTION is a re-baseline. (a) **Reuse `s1`..`s4` + `sfix_all`** with new
  (broader) meanings -- low numbers, "5 scenarios" reads cleanly, but the "S1" label
  spans two definitions across versions in HISTORY. (b) **Fresh ids `s20`..`s23` +
  `sfix_all`** -- a clean append, HISTORY shows S1-S14 ending and the new block
  starting, but "5 scenarios numbered s20+" is odd. **Recommend (a)** with a
  documented "re-baselined at vX.Y" note: each new sN is a strict SUPERSET of the
  old sN's axis, so the row stays semantically continuous.
- **D2 -- `fix` vs `fix --dry-run` for S5fix.** Real `fix` mutates the tree, so
  hyperfine's many iterations each need a fresh materialization (harness change +
  noise). `fix --dry-run` exercises collect + compute + compose + verify (the
  engine-heavy work) and is idempotent across iterations -- matching the micro
  `fix_throughput`. **Recommend `--dry-run`** (add a `Mode::Fix` that runs
  `fix --dry-run`); the write-back cost is I/O-bound and less interesting for an
  engine benchmark, and is separately covered by the `apply` correctness corpus.
- **D3 -- S4 tree type.** Combining S3+S8+S9 wants extends + nested + polyglot +
  git. **Recommend** the polyglot tree WITH a git overlay (a new
  `requires_git_repo && requires_polyglot_tree` combination in `run.rs`), so one
  scenario carries the full realism. Alternative: keep S4 on the regular+git tree
  and drop polyglot realism (loses S9's scope_filter-at-scale shape).
- **D4 -- Historical comparability.** Keep the 14 old result dirs as-is
  (render-history shows them retiring). The consolidation release publishes only the
  5. Cross-version comparison of an old axis uses the old sN row up to the
  re-baseline and the new sN row after. **Recommend** documenting this cutover in
  HISTORY.md + `bench-coverage.md`.
- **D5 -- Spawn fix ops in S5fix.** `command` + `git_untrack` SPAWN (subprocess per
  violation) -- including them makes the fix bench noisy + slow and measures
  `git` / the command, not alint. **Recommend EXCLUDING** them from the perf
  scenario (they are correctness-gated elsewhere); note it. Alternative: a tiny
  fixed count with a `true` command (like old S13) to measure spawn overhead.

## Implementation sequence (post-approval)

1. Fix the `bench-record.yml` glob (constraint 7) -- standalone, lands first.
2. Author the 5 new scenario YAMLs; delete the 14 old (det_check repoints).
3. Rewrite the `Scenario` enum + add `Mode::Fix` + the git+polyglot combo; update
   the 3 enum unit tests.
4. Repoint `det_check.rs`; update `facts.rs` count + regen `facts.json`.
5. Update `render-history.py` (retire old ids, add new, repoint cell_keys / anchor);
   update the trajectory assert.
6. Update the docs + the stale claims.
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
