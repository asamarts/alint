# v0.22: completing the relation-layer port

Status: planned (2026-11-07).
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
  gives them on Windows), so the reader never needs a fallback and `key_parity` gains the same
  coverage. Display stays lossy, as `Path::display` is today.
- **Caches for every value rule.** `CrossFileRule::uses_relations` returns true for the value
  relations and `resolves`, so the engine provides run-scoped caches whenever such a rule runs.
  Only paths a relation reads are retained (`ReadCache::mark_interest`), normally a few manifests
  per rule. The run-wide decode row budget is checked against the old per-extract limit
  (`MAX_EXTRACTED_VALUES`) before the switch, so no configuration that passed fails on a budget it
  never had.
- **Deleted:** the old reader, `check_equals` in `cross_file::eval`, `shadow.rs`, the `shadow-ir`
  feature, its enablement in `alint-e2e` and the corpus replay, and CI's feature-tree check for it.
  `read_rel` stays only if a non-value relation still needs it.

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
  switch, and the corpus replay covers the set relations it contains.
- **Performance.** Reading through the caches adds a map lookup and an `Arc` per resource and
  retains the bytes of every file a relation reads. The deterministic and syscall gates catch
  instruction and I/O growth; peak RSS is checked at 1M before the tag.

## 7. Status log

- 2026-11-07: plan and ADR-0030 (phase 0).

Next: phase 1.
