# `ordered_block` — lines between marker pairs stay sorted (and optionally unique)

Status: **Implemented** — lands with the rule in v0.10 (this
commit; rule kind #3 of the case-study coverage push). Was a
design draft (2026-05-18). v0.10 demand #3 (8 sources,
ROADMAP-canonical; tied with `registry_paths_resolve` at the top
of the backlog). Open questions resolved on implementation:
literal markers only — regex deferred (Q1); `numeric` shipped,
`natural` deferred (Q2); detection-only, auto-fix is a follow-up
(Q3); no nesting, blanks ignored not grouped (Q4); one violation
per block (Q5). Unlike
`registry_paths_resolve` / `cross_file_value_equals` this is a
**per-file** rule (the `PerFileRule` fast path), not cross-file.

Demand evidence:
[`docs/development/launch-evidence.md`](../../development/launch-evidence.md)
("lines between marker pairs sorted unique under configurable
comparator", 7 sources: rust, airflow, tokio, cpython, arrow,
golang/go, protobuf `failure_lists`) and the per-repo tracker in
[`examples/README.md`](../../../examples/README.md#primitive-demand-tracker)
(`ordered_block` row: airflow, spark, flutter, golang-go,
protobuf, cpython, rust-lang/rust, tokio). Canonical scope:
[`../ROADMAP.md`](../ROADMAP.md#v010-case-study-coverage-push)
(#3).

## Problem

Projects keep hand-edited sorted regions delimited by marker
comments, and they drift: a contributor appends out of order or
duplicates an entry, the diff churns, parallel PRs conflict on
the same tail, and dedup silently fails. The shape recurs across
the demand sources:

- **`keep-sorted` / `keep_sorted` blocks** (rust, tokio, arrow):
  `// keep-sorted start` … `// keep-sorted end` around dependency
  lists, match arms, re-export lists. A whole ecosystem of
  one-off pre-commit scripts exists only to enforce this.
- **protobuf conformance `failure_lists`** (highest stakes):
  must stay sorted *and unique* so cross-implementation merges
  don't conflict and a failing case isn't silently listed twice.
- **`ACKS` / allowlists / module registries** (cpython, airflow,
  golang/go, flutter, spark): alphabetised contributor and
  allow/deny lists that rot the moment someone appends.

There is no generic "the lines between *these markers* must stay
sorted (optionally unique) under *this* comparator" check; every
project reinvents a bespoke script. `ordered_block` is that
primitive.

Precise, not heuristic (ROADMAP v0.10 cross-cutting decision):
literal line comparison under a named comparator, no guessing.

## Surface area

New per-file rule kind `ordered_block` in `alint-rules`.
`version: 1` unchanged; every v0.9.x config still parses.

```yaml
- id: keep-sorted
  kind: ordered_block
  paths: ["**/.gitignore", "CODEOWNERS", "**/*.bazel"]
  start: "# keep-sorted start"     # the marker line, matched on the trimmed line
  end: "# keep-sorted end"
  comparator: lexical              # lexical (default) | lexical-ci | numeric
  unique: false                    # also forbid duplicate entries in the block
  level: warning
```

`start` / `end` are matched against each line's trimmed content
(exact equality). Multiple independent blocks per file are
supported. A file with no `start` marker is silently fine — the
rule only governs explicitly-marked regions (same opt-in shape
as `markdown_paths_resolve`).

## Semantics

Per matching file (`PerFileRule` dispatch):

1. Scan lines. On a line whose trim equals `start`, open a block;
   collect subsequent lines until the line whose trim equals
   `end` (markers themselves are not part of the ordered set).
2. **Blank lines inside a block are ignored** (not part of the
   ordered sequence) — `keep-sorted`'s common behaviour and the
   least-surprising default. Every other line is an *entry*,
   compared by its **trimmed** content.
3. Each entry must be `>=` the previous entry under `comparator`
   (non-decreasing). With `unique: true` it must be strictly
   `>` (no equal/duplicate entries).
4. The **first** offending entry per block produces one
   violation, anchored at that line (`block starting at line S:
   "<entry>" is out of order` / `… is a duplicate`). One
   violation per problem per block — actionable, not noisy.
5. A `start` with no matching `end` before EOF is a violation
   (`unclosed ordered_block opened at line S`).

`comparator`: `lexical` (Rust `str` `Ord`, byte-wise UTF-8),
`lexical-ci` (ASCII-case-insensitive), `numeric` (parse the
leading integer of each entry; entries without one fall back to
`lexical`, so a mixed block degrades predictably rather than
panicking). Deliberately small for v0.10.

One existing config runs unchanged; the rule only adds new
shapes.

## False-positive surface

- **No markers ⇒ silent.** The rule never fires on a file
  lacking `start`; it governs opt-in regions only. No tree-wide
  "everything must be sorted" surprise.
- **Indentation / trailing whitespace.** Comparison is on the
  trimmed entry, so re-indentation doesn't spuriously reorder.
- **Blank lines.** Ignored (step 2) — a blank inside the block
  is not "the smallest entry".
- **Mixed numeric/non-numeric under `numeric`.** Non-numeric
  entries fall back to `lexical` rather than erroring; documented
  as a soft degrade (use `lexical` if the block is truly mixed).
- **Nested / repeated `start` before `end`.** v0.10: no nesting
  — a block is `start` → the next `end`; a second `start` inside
  an open block is treated as an ordinary entry. Documented
  limitation (Open question 4).
- **Marker as content.** Because markers match the *trimmed
  whole line* exactly, an entry that merely contains the marker
  text as a substring is unaffected.

## Implementation notes

- Module: `crates/alint-rules/src/ordered_block.rs`. Per-file:
  `impl Rule { eval_per_file }` + `as_per_file` +
  `impl PerFileRule { evaluate_file(ctx, path, bytes) }`,
  modelled on `markdown_paths_resolve`. `Scope::from_spec(spec)`
  for `paths`.
- No `FileIndex` / cross-file dispatch, no structured-query
  engine — pure line scan, O(L) per file. Comparator is a small
  `#[serde(rename_all = "kebab-case")]` enum.
- Non-UTF-8 files: skip (degenerate for a line-sorted text
  region), same as `markdown_paths_resolve`.
- No `include_str!` data; nothing leaves the crate.

## Tests

- Sorted block passes; unsorted block fails (one violation,
  correct line).
- `unique: true` with a duplicate entry fails; without it the
  duplicate passes (non-decreasing).
- Each `comparator`: `lexical`, `lexical-ci` (e.g. `Bravo` /
  `alpha` ordering), `numeric` (`9` before `10`), and the
  mixed-numeric fallback.
- Multiple independent blocks in one file (one bad, one good →
  exactly one violation).
- Unclosed `start` ⇒ violation; file without markers ⇒ silent;
  blank lines inside a block ignored.
- Lockstep with the codebase invariants (same checklist #1/#2
  followed): `coverage_audit_pass_fail` (per-file pass/fail
  scenarios), schema `$def` + dispatch `$ref` in both mirrored
  `config.json`, `all_kinds.yaml` entry, regenerated
  default-options snapshot, rule-count **72 → 73** across README
  ×2 / `docs/site/about` / `coverage_audit_readme_claims`,
  `docs/rules.md` section, CHANGELOG `[Unreleased]` Added (the
  third v0.10 item).
- **Bench-compare threshold:** add to the synthetic-tree
  fixture; O(L) per file, single pass — full-run S-class wall
  must not regress vs the pre-phase baseline (the `xtask
  bench-gate` gate, per `RELEASING.md`).

## Open questions

Resolve inline when implementation lands.

1. **Regex markers.** `start_regex` / `end_regex` for projects
   whose markers carry a payload (`# keep-sorted block=imports`).
   Leaning: ship literal-only in v0.10; add regex when a 2nd
   source needs the payload.
2. **`natural` comparator.** Natural sort (numbers within
   strings compared numerically, e.g. `x2` < `x10`). Deferred
   until a demand source specifically needs it beyond `numeric`.
3. **Auto-fix.** `ordered_block` is a strong fix candidate
   (re-sort the block in place). v0.10 is detection-only
   (consistent with #1/#2 and the tight cut); fix is a clean
   follow-up once the detector is proven.
4. **Nesting / grouping.** No nesting in v0.10. Blank-line
   *grouping* (sort within groups, keep groups ordered by first
   entry) is a real `keep-sorted` mode — deferred; v0.10 ignores
   blanks entirely.
5. **Reporting granularity.** One violation per block (first
   offence) vs per offending entry. v0.10: per block — matches
   `keep-sorted` UX and keeps output actionable; revisit if
   users want every offending line surfaced.

## v0.12: optional markers (markerless / sort-to-EOF) — SHIPPED

`start` and `end` were both required. The deep v0.12 case study found ~7 repos
whose whole-file sorted lists (dictionaries, allow-lists, fully-sorted
`CODEOWNERS`) have no natural marker pair to delimit — there is no line that
*isn't* a sortable entry, so the keep-sorted-block framing didn't fit.

Both markers are now `Option<String>`: the block spans an optional `start`
(default: the start of the file) to an optional `end` (default: EOF).

- omit `end` → sort from `start` to EOF;
- omit both → sort the whole file (markerless);
- omit `start`, keep `end` → sort from BOF until the `end` marker.

Implementation is a pure relaxation: with no `start`, the block is initialised
open at line 1; with no `end`, no marker line closes it. The `unclosed` check
fires only for the fully-delimited `start`+`end` contract (a block with an
absent `end` runs to EOF by design), so every existing marker-pair config
behaves identically. No new dispatch class; rule-kind count unchanged.

This is the surviving slice of the v0.12 "extract/normalize long tail" cluster.
Reproduce-first dissolved the rest: `split:` (redundant with a `regex` capture
group + the shipped `semver-minor` normalize), `occurrence: first` (workable
via the `(?s)\A.*?` first-match-anchor idiom), and inline `values:` / `except:`
(a one-line committed allow-file + `cross_file` `subset` already expresses it;
~2 repos). All documented as workaround idioms; revisit on recurrence.
