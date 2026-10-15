# v0.18.3 changed-mode cells: tree-set overhead and the cost of correct `has_ancestor`

Status: resolved. The avoidable part is fixed for the next release; the remainder is the
intended cost of a correctness fix.

## Symptom

The v0.18.3 `bench-record` run (canonical bench host, quality gate PASS) failed the regression
gate on one cell, and every `changed`-mode cell moved up against v0.18.2:

| Cell | v0.18.2 `min_ms` | v0.18.3 `min_ms` | Delta | Gate |
|---|---|---|---|---|
| S2 1m changed | 5156 | 5980 | +16.0% | FAIL (limit +15%) |
| S4 10k changed | 104 | 117 | +12.4% | ok |
| S2 100k changed | 487 | 537 | +10.2% | ok |
| S4 1m changed | 9310 | 10206 | +9.6% | ok |
| S4 100k changed | 824 | 887 | +7.6% | ok |
| S1 1m / 100k changed | | | +4.9% / +4.5% | ok |

`full`-mode cells were unchanged.

## Cause

v0.18.3 made `--changed` judge changed files against the whole repository: a filtered index
now shares the full tree's path sets, so a `scope_filter.has_ancestor` marker outside the diff
counts. The engine built and shared BOTH the full index's file set and its file-and-directory
set on every `--changed` run, a pass over the whole index (two hash sets of every path at 1M
files), whether or not any rule performed a tree-wide lookup.

- **S2** (per-file content rules, no `has_ancestor`) paid that cost for nothing: about 820 ms
  at 1M files.
- **S1 and S4** use `has_ancestor` (S4 through the bundled `rust`, `node`, and `python`
  rulesets). For them, the full file set is required for the correct answer, and evaluating the
  changed files that really are in scope is more work than v0.18.2's (incorrect) behaviour of
  dropping them.

## Evidence

Same-host A/B, all three binaries built from source with one toolchain and profile (the
published release binaries are static musl builds and are not comparable with a local build),
`bench-scale --sizes 100k --scenarios S2,S4 --modes changed --runs 20`, two interleaved rounds
(median ms):

| Scenario | v0.18.2 | v0.18.3 | fixed |
|---|---|---|---|
| S2 100k changed | 425 / 431 | 449 / 464 | 422 / 427 |
| S4 100k changed | 646 / 652 | 695 / 706 | 694 / 694 |

The fix removes the S2 overhead entirely (back to v0.18.2); S4 keeps about +7%, the cost of
correct `has_ancestor` evaluation.

## Fix

- The filtered index shares only the full tree's FILE set (the file-and-directory set is no
  longer shared: its only reader, `markdown_paths_resolve`, is `requires_full_index` and always
  evaluates on the full index).
- The set is shared only when a rule with a `has_ancestor` predicate will actually be evaluated
  against the filtered index (`Engine::needs_tree_files`): it is not `--changed`-skipped, not
  `requires_full_index`, has no changed dependency, is not git-tracked, and, in `check`, is not
  a per-file rule (the per-file pass matches scope on the full index). A first version shared
  it whenever any rule had a `has_ancestor` predicate, which still made a repository that
  extends the bundled `rust` / `node` / `python` rulesets pay for it on a diff none of those
  rules covered; review caught that. Cachegrind on a 100k-file tree with such a rule out of the
  diff's scope: v0.18.2 2.110 G instructions, the first version 2.314 G, the final fix 2.121 G.

## Publication

The v0.18.3 `bench-record` PR is not merged (the gate failed); the next release's run, with the
fix, is the published `changed`-mode data point. The S4 changed-mode increase (+7 to +10%) is
expected to remain and is documented here.
