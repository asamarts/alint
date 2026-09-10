# Auto-fix implementation plan

Status: Planning (execution companion to the accepted design). This doc turns the
[`auto-fix.md`](auto-fix.md) framework (accepted) and [ADR-0017](../adr/0017-auto-fix-edit-model-and-applicability.md)
(accepted) into a phased, test-first build plan grounded in the current code. It does not
re-argue the design; it says, per phase, exactly what changes, where, how it is tested, which
gates must stay green, and what can go wrong. Detection-side companion:
[`rule-coverage-gaps.md`](rule-coverage-gaps.md).

Conventions: file references are `path:line` into the repo at the time of writing (line numbers
drift; treat them as a starting point, not a contract). Each phase follows the
[`TEMPLATE.md`](TEMPLATE.md) spirit and, when scheduled, graduates into its own
`docs/design/vX.Y/` record. This plan is em-dash-free by house rule. It has been through one
round of adversarial audit (three independent passes: code-accuracy, gaps/coherence,
test-coverage) plus a self-review; the revision note at the end records what that changed.

## Contents

- [1. Baseline reality: what already ships](#1-baseline-reality-what-already-ships)
- [2. The verification model every phase uses](#2-the-verification-model-every-phase-uses)
- [3. Downstream artifacts and their gates](#3-downstream-artifacts-and-their-gates)
- [4. Cross-cutting workstreams](#4-cross-cutting-workstreams)
- [5. Phase 0: the fix-engine foundation](#5-phase-0-the-fix-engine-foundation)
- [6. Phase 1: located content replacement and the fixpoint](#6-phase-1-located-content-replacement-and-the-fixpoint)
- [7. Phase 2: structured value edits (the flagship)](#7-phase-2-structured-value-edits-the-flagship)
- [8. Phase 3: metadata, VCS, and repo-scale cross-file](#8-phase-3-metadata-vcs-and-repo-scale-cross-file)
- [9. Phase 4: ordering, canonicalization, and headers](#9-phase-4-ordering-canonicalization-and-headers)
- [10. Deferred and special](#10-deferred-and-special)
- [11. Risk register](#11-risk-register)
- [12. Sequencing, milestones, and versioning](#12-sequencing-milestones-and-versioning)
- [13. Definition of done](#13-definition-of-done)

## 1. Baseline reality: what already ships

The single most important planning fact: **`alint fix` already exists and ships.** The engine,
the 12 whole-file ops, the CLI subcommand, LSP quick-fixes, the human / json / markdown fix
reports, and **two** end-to-end test layers with real `fix` cases are all in the tree today. This
plan is overwhelmingly *extension at known seams*, not greenfield. Build on the following; do not
rebuild it.

| Capability | Where | Notes |
|---|---|---|
| Fix edit model | `crates/alint-core/src/rule.rs:540-550` | `FixEdit` = `SetContent` / `CreateFile` / `DeleteFile` / `RenameFile` (4, whole-file only) |
| Fixer contract | `rule.rs:552-579` | `describe` / `apply` (writes) / `fix_edit` (non-writing, drives LSP); `FixContext` at `rule.rs:501-517` carries `dry_run`, `fix_size_limit`, `allow_out_of_root` |
| Rule to fixer | `rule.rs:390-392` (`Rule::fixer()`) | **16 rule kinds bind a fixer**, mapping to **12 distinct `*Fixer` structs** (shared, e.g. `FileRemoveFixer` serves four kinds); **all 12 override `fix_edit`** (so every bound kind has an LSP path). Fixers live in `crates/alint-rules/src/fixers/{creators,file_ops,hygiene,strip}.rs`, exported from `fixers/mod.rs` |
| Apply loop | `crates/alint-core/src/engine.rs:909-1067` (`Engine::fix`) | plain **serial** `for entry in &self.entries` (`:1006`), `evaluate` at `:1034`, `fixer().apply` at `:1047`, `FixOutcome`->`FixStatus` at `:1046-1053`. No fixpoint, no overlap detection, no re-walk. Signature `fix(&self, root, index, dry_run)` carries **no tier/threshold parameter** |
| Size / atomicity | `read_for_fix` `rule.rs:635-648`, `check_fix_size` `rule.rs:604-629`, `write_atomic` `crates/alint-rules/src/io.rs:104` | limit enforced inside each fixer's `apply`; `fix_edit` is **un-guarded by design** (`rule.rs:571-574`); engine never touches disk |
| Report | check-side `Report` `report.rs:6-35`; fix-side `FixReport` `report.rs:40-104`; `FixStatus` `report.rs:58-67` (`Applied`/`Skipped`/`Unfixable`); `has_unresolved` `report.rs:106-110` | **`applied`/`skipped`/`unfixable`/`has_unresolved` are `matches!`-based filters, NOT exhaustive** (so a new variant compiles clean and is silently uncounted); exit chain `report.rs:89-99` -> `main.rs:1012-1018` |
| CLI | `Cli` `crates/alint/src/cli.rs:32`, `Command::Fix` `cli.rs:208-224`, handler `cmd_fix` `main.rs:945-1020` | `--dry-run`/`--changed`/`--base` fix-local; `--baseline`/`--strict-baseline`/`--show-baselined`/`--only`/`--format` global; `--baseline` family **rejected** for non-check at `main.rs:179-186`; `--changed` already carves out cross-file + existence rules (they see the full tree) |
| Structured parse | `crates/alint-core/src/structured_format.rs` (`Format` `:42-51`, `Format::ALL` `:61-70`, `parse` `:72-155`) | returns a **detached, lossy** `serde_json::Value`; `BTreeMap`-backed (keys alphabetized; `preserve_order` off) |
| Structured-query rules | `crates/alint-rules/src/structured_path.rs` (`Op` `:100-116`, `evaluate_file` `:285-364`; shared builders `build_equals` `:586` / `build_matches` `:614` / `build_absent` `:558`) | `query` at `:314` is **non-located**; violations carry **no byte span** (never call `with_location`); `*_path_equals` = one violation per node, `*_path_absent` = one file-level violation for N nodes. **These builders do NOT read `spec.fix` today: a `fix:` block on a `*_path_*` kind is silently ignored, not rejected** (no catch-all here) |
| Located JSONPath | `serde_json_path` 0.7.2 exposes `query_located` -> `LocatedNodeList` -> `NormalizedPath` (`PathElement` = `Name`/`Index`) | present on the **current pin**, no upgrade needed |
| e2e Layer A (in-process) | `crates/alint-e2e/scenarios/**/*.yml` (recursive glob, `tests/scenarios.rs:45-47`; **17** fix scenarios incl. `fix/interactions/{mixed_fixable_and_unfixable,multiple_fixes_in_one_pass}.yml`), schema `crates/alint-testkit/src/scenario.rs`, runner `runner.rs:243-282` | `given.tree/config/git`, `when: [check, fix, fix_dry_run]`, `expect` + **`expect_tree`** (byte-exact). `Step` enum has no `--unsafe-fixes`; `ExpectStep` has no `suggested:` field (both are gaps this plan closes) |
| e2e Layer B (binary) | `crates/alint/tests/cli/*.toml` (trycmd); fix cases `fix-apply`, `fix-file-{append,prepend,remove,rename}`, `fix-trim` | `.in/` -> run -> `.out/` byte tree + `.stdout`; regen `TRYCMD=overwrite`; `[EXE]` placeholder gotcha in `help-*.stdout`. trycmd has **no per-OS skip** (unlike scenario `tags:`) so a unix-only case needs `#[cfg(unix)]` wrapping |
| proptest laws | `crates/alint-e2e/tests/invariants.rs` (**4** laws, `cases: 48`, `PROPTEST_CASES` scales) + generators `crates/alint-testkit/src/strategies.rs` | `check_never_panics`, `fix_dry_run_is_pure`, `fix_is_idempotent`, `fix_converges_when_fully_resolved`. `fixable_scenario_tree` (`strategies.rs`) is limited to `file_create/remove/prepend/append/rename` and emits `file_*` rules over ASCII blobs only |
| Kani | `crates/alint-core/src/pathsafe.rs:161` (`confine_steps_is_sound`, `#[kani::proof]` `#[kani::unwind(7)]`); CI `.github/workflows/kani.yml` (weekly) | **LIVE BUG (R-KANI): the workflow runs `-p alint-rules`, which has zero `#[kani::proof]`, so the job verifies nothing today** and is vacuously green |
| Perf gate | `crates/alint-bench/benches/det_check.rs` (gungraun / Callgrind, fixed-path byte-stable `Ir`, `check`-only); criterion `benches/fix_throughput.rs` exists | the deterministic gate is **advisory** (`DET_PERF_ADVISORY=1`, `ci.yml`), PR-only, and **I/O-blind** (measures `Ir`, not file reads) |
| LSP mapping | `crates/alint-lsp/src/lib.rs:771-828` (`fix_edit_to_workspace_edit`, exhaustive match); code actions `:446-520` | `SetContent` arm (`:780`) maps to a whole-document `TextEdit` via the `whole_document()` helper (`:763-765`); `Position.character` is **UTF-16** |
| Output formats | `crates/alint-output/` (8: agent/github/gitlab/human/json/junit/markdown/sarif) | fix renderers exist only for **human/json/markdown** (their `FixStatus` matches ARE exhaustive: `json.rs:219`, `human.rs:483`, `markdown.rs:95`); `cmd_fix` rejects other fix formats at `main.rs:963-972`; `SarifResult` (`sarif.rs:279-298`) has **no `fixes` field** yet |

**Implication.** The plan's weight is: (a) two new `FixEdit` variants + an `Applicability` type + a
threshold parameter on `Engine::fix`; (b) a rule-level `collect_edits` binding; (c) the batched
located-edit engine + fixpoint (built dormant first); (d) the per-format locate-and-serialize bridge
(Phase 2), which also has to *add* fix-parsing to the structured-query builders that ignore it today;
(e) the trust/provenance gate (genuinely new loader plumbing); (f) new CLI flags, LSP UTF-16 mapping,
and SARIF/agent `proposed_edit` emitters; and (g) **test-harness extensions** that do not exist yet
(drive `--unsafe-fixes` from the scenario runner, assert a `Suggested` status, generate structured
documents, and inject a write failure). Every code seam has an existing sibling to copy.

## 2. The verification model every phase uses

The user-facing requirement is **full unit and end-to-end integration coverage**. That is not one
test type; it is a fixed ladder, and every phase must climb all applicable rungs. Critically, "full
coverage" must be **mechanically enforced**, not asserted in prose: today nothing gates that a fixer
has a fix scenario, and the shipped 12 fixers already fall short of the matrix below (only `strip.rs`
carries a parity test; `file_ops.rs` has no idempotence/parity/silent test by name). Phase 0
therefore builds a new coverage gate (rung 8).

| # | Rung | What it proves | Where / how to add |
|---|---|---|---|
| 1 | **Unit (inline)** | one op does the right byte transform, and rejects incompatible input | `#[cfg(test)] mod tests` in the fixer file (`fixers/strip.rs:248-270` template): build a `Violation`, call `fix_edit`/`apply`, assert `FixEdit`/`FixOutcome`; **plus a `build_rejects` arm on every non-host rule** |
| 2 | **apply/fix_edit parity** | the disk path and the LSP path never diverge | copy `bom_fix_edit_binary_guard_mirrors_apply` (`fixers/strip.rs`); one per fixer with both paths |
| 3 | **e2e Layer A (scenario)** | `alint fix` over a real tree yields exact bytes | new `crates/alint-e2e/scenarios/fix/<name>.yml`; assert `applied/skipped/unfixable`/`suggested` + `expect_tree`; OS-gate with `tags:` (`unix-only` etc., honored at `scenarios.rs:29-43`) |
| 4 | **e2e Layer B (binary)** | the shipped binary mutates disk and prints the right summary + exit code | new `crates/alint/tests/cli/<name>.toml` + `.in/`/`.out/`/`.stdout`; regen `TRYCMD=overwrite`; restore `[EXE]`; wrap unix-only cases in `#[cfg(unix)]` |
| 5 | **proptest (laws)** | algebraic invariants hold over generated trees | extend `invariants.rs` + `strategies.rs` (see below; several laws need new harness capability) |
| 6 | **Kani (bounded proof)** | one combinatorial invariant is exhaustively correct | add beside `confine_steps_is_sound` (`pathsafe.rs`); **fix R-KANI first** or the proof does not run |
| 7 | **Perf** | no fix-path regression | a `fix --dry-run` cell in `det_check.rs` (advisory, and I/O-blind, so pair with a `fix_throughput.rs` wall-clock/syscall cell for the read-heavy collect path, R-DETGATE) |
| 8 | **Coverage gate (NEW)** | every op is actually tested | `coverage_audit_fix_coverage.rs` (Phase 0): enumerate every `FixSpec` op / `*Fixer` struct and assert each maps to a fix scenario with `applied:` **and** an idempotence scenario (`when: [fix, fix]`, second pass `applied: []`) |
| 9 | **Schema + doc drift** | generated surfaces stay in sync | `fix_report_schema.rs` round-trip, `in_crate_schema_matches_root`, `gen-schema --check`, `docs-export --check`, `coverage_audit_schema_drift.rs`, the `Format::ALL` parity gates (section 3) |
| 10 | **Dogfood + structural completeness** | alint's repo stays clean; every kind has pass/fail scenarios | the repo `.alint.yml`; `coverage_audit_pass_fail.rs` (`every_registered_rule_kind_has_pass_and_fail_scenarios`) |

**Per-op / per-flag coverage matrix** (the minimum bar; rung 8 enforces it): for each new op, ship
`{unit fire, unit silent, unit idempotence, `build_rejects` on non-host rules, parity, one Layer-A
scenario, one Layer-B trycmd}`; for each format in Phase 2 add `{comment/order golden,
re-parses-but-wrong-value-demoted, invalid-document-demoted, value-serialization matrix, CRLF golden}`;
for each new CLI flag add `{Layer-B help snapshot, one exercising scenario/trycmd}`; for each engine
invariant add `{a proptest law or a debug-assert}`.

### Proptest laws, and the harness changes they require

`invariants.rs` today needs new capability before most new laws are even runnable. The scenario
`Step` enum (`scenario.rs:186`) has only Check/Fix/FixDryRun/CheckChanged, and `run_step`
(`runner.rs:245`) calls `engine.fix(root, index, dry_run)` with no tier threshold, so **no test can
drive `--unsafe-fixes` today**. Add a `Step::Fix { unsafe_fixes: bool }` (or a variant), the new
threshold parameter on `Engine::fix`, and the runner wiring, in Phase 0/1. Then:

- **Tier monotonicity** (**Phase 1**, once an Unsafe op exists): default `fix` never applies an
  Unsafe/Suggestion edit; `fix --unsafe-fixes` is a superset. Vacuous in Phase 0 (no non-Safe op
  ships there), so it lands with `replace`.
- **Safe multiset descent** (**Phase 1**, active fixpoint): per-pass `|V|` lives inside `Engine::fix`
  and is not visible to the runner, so encode it as a `debug_assert!` in the fixpoint loop (DoD item
  6 permits) plus a proptest that a Safe-only multi-rule tree converges in `<= |V|` passes (needs a
  pass counter surfaced from the engine). Caveat: the strict "introduce none" contract is *violable*
  by the existing generator (`fixable_scenario_tree` makes `file_content_matches` append an SPDX line,
  which alongside a `file_content_forbidden: SPDX` rule introduces a violation and the law shrink-finds
  it), so the generator must draw non-conflicting Safe rule sets or the law must be qualified.
- **Order-independence modulo reproducibility** (**Phase 1**): shuffling independent rule order yields
  the same tree for disjoint edits; document that overlapping edits are reproducible-not-canonical
  (matches 5.8, do not over-claim confluence).
- **GetPut / PutGet lens laws** (**Phase 2**): these need a whole **new generator family**, not a
  "widening": a per-format `structured_doc_strategy()` producing valid comment-bearing documents plus
  a `jsonpath_into(doc)` strategy producing valid paths into them. `strategies.rs` today cannot emit
  structured documents at all; budget this as a real Phase 2 infra line.
- **Structured no-op preserves comments/order** (**Phase 2**): a Safe structured fix touches only the
  targeted span (the golden-file property, generalized).

## 3. Downstream artifacts and their gates

alint gates its own generated surfaces; a phase is not mergeable until these are updated **in the
same PR**. This is the pre-PR checklist for any change that adds an op or a status.

| Artifact | When | Gate that catches you |
|---|---|---|
| `crates/alint-core/src/config.rs` `FixSpec` variant + inner `*FixSpec` struct + `op_name()` arm | new op | compile + `expecting`-message test (`config.rs:865-871`) |
| The host builder(s) accept the op | new op | per-builder `fix.<op> is not compatible with <kind>` test. **Exception: the structured-query builders (`build_equals`/`build_absent`/`build_matches`) do NOT parse `fix:` today (silent ignore, no catch-all), so Phase 2 must ADD both honoring and rejection to those 3 shared helpers** |
| `crates/alint-rules/src/fixers/*.rs` new `pub struct *Fixer` | new op (drives the count) | `readme_auto_fix_ops_count_matches_fixers` (`coverage_audit_readme_claims.rs:309-325`, counts `*Fixer` structs); plus the NEW `coverage_audit_fix_coverage.rs` (rung 8) |
| `schemas/v1/config.json` `$defs/fix` (hand-written `oneOf` branch, `additionalProperties:false`, `applicability` inside the op object) then `xtask gen-schema` to sync `crates/alint-dsl/schemas/v1/config.json` | new op | `gen-schema --check` drift + `in_crate_schema_matches_root` |
| `schemas/v1/fix-report.json` (hand-written) + the `FixReport` serializer in `alint-output` | the `suggested` status (**Phase 0**, with W1) | `crates/alint-output/tests/fix_report_schema.rs` round-trip (NOT gen-schema) |
| `facts.json` (regenerate via xtask) | new op | `xtask/src/facts.rs:806` cross-check (not the decoy `crates/alint-core/src/facts.rs`) + README headline |
| `README.md` headline "N auto-fix ops" | new op | `readme_auto_fix_ops_count_matches_fixers` |
| `docs/rules.md` per-op reference | new op | `docs-export --check` |
| `docs/design/ARCHITECTURE.md` fix-ops table + execution step 9 (already stale: omits `file_footer`) | new op / engine change | `docs-export --check`; the fixpoint re-walk also amends the "walk once" invariant text at both `ARCHITECTURE.md:39` and `:353` (R-WALK) |
| **`SPAWNING_FIX_OPS` SSOT (new) + a spawn-parity gate** mirroring `coverage_audit_spawn_gate.rs` | a spawning op (Phase 3) | the new gate must assert every entry is refused from a non-top-level `extends:`, exactly as the kind-level gate does for `SPAWNING_RULE_KINDS` |
| **A `Format::ALL` value-serializer parity gate (new)** | Phase 2 | an exhaustive `match` over `Format::ALL` asserting each variant has a `set_value`/`remove_value` serializer or is on an explicit unsupported list (mirrors `format_all_is_complete` `structured_format.rs`, `extract.rs:299`, `json_schema_passes.rs:511`) |
| `ROADMAP.md` / `roadmap.json` | when a phase is scheduled | `gen-roadmap --check` (leave untouched until scheduling, to keep the gate green) |

## 4. Cross-cutting workstreams

Five threads span phases. They are called out here and scheduled inside the phases.

- **W1 - Tier and report plumbing.** The `Applicability` enum, `FixStatus::Suggested(edit)`, and the
  reporting/exit-code plumbing. Lands in **Phase 0**. Watch the split: the fix formatters'
  `FixStatus` matches ARE exhaustive (`json.rs:219`, `human.rs:483`, `markdown.rs:95`) and the engine
  mapping (`engine.rs:1046-1053`) is compiler-forced; but `FixReport::applied/skipped/unfixable`
  (`report.rs:70-86`), `has_unresolved` (`:106`), the testkit `assert_fix_status` (`runner.rs:402`),
  and `ExpectStep` (`scenario.rs:220`) are **`matches!`-based / field-based and NOT compiler-forced**,
  so adding `Suggested` compiles clean while silently (a) treating a Suggestion as resolved (wrong
  exit code) and (b) leaving the scenario harness unable to assert one. W1 must therefore add: a
  `FixReport::suggested()` counter + a summary line in all three fix formatters; the `has_unresolved`
  extension (error-level Suggested counts as unresolved); an `ExpectStep.suggested` field + a
  `Suggested` arm in `assert_fix_status`; and a unit proving an error-level Suggested trips
  `has_unfixable_errors` -> nonzero exit, plus a Layer-B trycmd asserting that exit code.
- **W2 - Trust and provenance (security).** Per ADR-0017 decision 3, split by first-subject:
  - **Content-fixer demotion + `trusted_extends:`** (**Phase 1**, with `replace`, the first
    content-injecting *new* op reachable via `extends:`). Thread the four-way source (top-level /
    local-path+nested / bundled / remote-URL) captured at the single classification site
    `loader.rs:143-158` onto `RuleSpec` / `RuleEntry` (neither has an origin field today). Content
    fixers from a **remote-URL** `extends:` demote to Suggestion (honored from the user's own tree and
    first-party bundled rulesets); `trusted_extends:` opts specific remotes back in; promotion is
    top-level-only. **This also retroactively demotes the three EXISTING inline-content ops**
    (`file_create`/`file_prepend`/`file_append`) when they arrive via a remote `extends:` (they
    auto-apply from any source today), which is an intentional behavior change for existing ops, not a
    no-op (see DoD item 3). The test matrix must pin all four provenance classes + `trusted_extends:`
    promotion + the negative (promotion from a non-top-level source refused).
  - **Spawning-fixer refusal** (**Phase 3**, with `git_untrack`, the first spawning fix op). Phase 0
    builds the mechanism as *scaffolding* only: a `SPAWNING_FIX_OPS` SSOT (empty until Phase 3) scanned
    in the `reject_command_rules_in` cluster (`loader.rs:163-167` / `nested.rs:205`). Nothing spawns
    until `git_untrack`, so the first real refusal + its test land in Phase 3 (with the spawn-parity
    gate of section 3). Do not schedule a spawning-refusal test before then (there is no subject).
- **W3 - Check-side fix output.** SARIF `result.fixes[]` (new `fixes` field on `SarifResult`
  `sarif.rs:279-298`, populated in `base_result` `:194-226`, with a schema-conformance test) and the
  `agent`/`json` `proposed_edit` field (distinct from the existing `agent` `fix_command`). Needs
  `Fixer::fix_edit`/`collect_edits` computed during `check`, gated to fix-carrying formats only.
  Lands in **Phase 2**; Phase 1 explicitly **defers** `replace`'s `proposed_edit`/SARIF surface (so a
  v0.17 `check --format sarif` does not surface `replace` fixes; revisit in Phase 2).
- **W4 - Baseline-aware fix.** `fix` rejects the `--baseline` family today (`main.rs:179-186`, which
  also covers `--strict-baseline` and `--show-baselined`). Make `fix` baseline-aware for all three
  (skip suppressed, surface as Suggestions, fix only new). Lands **by Phase 2**, before a Safe
  structured edit could auto-rewrite a grandfathered value. Extends
  [ADR-0006](../adr/0006-baseline-suppression.md) from `check` to `fix`.
- **W5 - Versioning and deprecation.** The tiers ship in **v0.17** (Phase 0) with a deprecation
  warning that `file_remove` will become Unsafe; `file_remove` flips Safe -> Unsafe about two minors
  later. The flip is not free: `file_remove` backs `file_absent`, `no_empty_files`, `no_submodules`,
  `no_symlinks`, and existing scenarios assert a **default** `fix` removes the file
  (`scenarios/fix/{file_remove,no_empty_files,no_submodules}.yml`, the trycmd `fix-file-remove`, and
  `fix/interactions/multiple_fixes_in_one_pass.yml`'s `no-bak` case). The flip PR must migrate every
  one of those (add `--unsafe-fixes` or `fix: { file_remove: { applicability: safe } }`); DoD item 3
  will not catch it because `file_remove` is an existing op whose *default* changes. W5 must also
  specify where the v0.17 deprecation warning is emitted (the fix-report path) and test it.

## 5. Phase 0: the fix-engine foundation

**Goal.** Add the substrate every later phase needs, ship **zero** new user-facing fixers, and remain
a **genuine no-op** (byte-identical result) for existing configs. Phase 0 does rework the whole-file
write path (in-memory config-order composition + one write per file, replacing per-fixer
disk-round-trips), but the resulting bytes are identical.

**Code changes.**

1. `FixEdit` gains `ReplaceRange { path, range: Range<usize>, content: Vec<u8> }` and
   `SetMode { path, mode: u32 }` (`rule.rs:540-550`). Compiler forces arms in
   `fix_edit_to_workspace_edit` (`lib.rs:771-828`); the `SetMode` arm returns **`None`** deliberately
   (chmod has no LSP `WorkspaceEdit`; it surfaces as a non-LSP Suggestion, per 5.7). `ReplaceRange`'s
   minimal-`TextEdit` mapping is **deferred to Phase 1** (no located edit is produced in Phase 0).
2. New `Applicability` enum (Safe/Unsafe/Suggestion/Never) beside `FixEdit` (W1). Classify the
   **existing 12 ops as Safe on introduction**, except `file_remove` (which stays Safe in v0.17 with a
   deprecation warning per W5; it flips to Unsafe about two minors later).
3. Rule-level `collect_edits(&[Violation], file, bytes, root) -> Vec<(FixEdit, Applicability)>` on the
   `Rule` trait next to `fixer()` (`rule.rs:390`), with a **default** that delegates to the existing
   per-violation `fixer().fix_edit()` so all 12 simple fixers are untouched.
4. Rework `Engine::fix` (`engine.rs:909-1067`) into two regimes: whole-file transforms compose in
   config order in memory (one write per file); located edits go through **collect -> tier-filter ->
   group-by-file -> total-order sort `(start, end, rule_index, violation_index)` -> skip-overlap +
   isolation groups -> re-parse AND localized-equivalence (PutGet) verify -> memoized per-(file,edit)
   demotion-to-Suggestion on verify failure -> `write_atomic`**. Note the two verify halves: re-parse
   is syntactic (rejects out-of-language output) and PutGet is semantic (rejects a splice that parses
   but wrote the wrong value/type/separator); a failing edit is demoted, not written, and the demotion
   is memoized so a stateless rule does not re-derive it every pass. **The batch, overlap-skip,
   isolation groups, verify, and fixpoint are built here but dormant** (only whole-file ops ship).
5. Add the applicability **threshold parameter** to `Engine::fix` (or a builder field) so the CLI tier
   selection (`--unsafe-fixes`) reaches the engine; thread `--diff`/`--fix-only` similarly.
6. Thread `WalkOptions` into the engine (new field + builder mirroring `with_fix_size_limit`
   `engine.rs:275-279`) OR hoist the fixpoint loop to `cmd_fix` which already holds the walk. Wired in
   Phase 0; the loop still runs once (the re-walk activates in Phase 1).
7. Relocate the `fix_size_limit` guard onto the read-only collect/read step (today it lives in each
   fixer's `apply` via `read_for_fix`; the generalized `collect_edits` path reads each file to locate
   spans and must re-apply the bound, since `fix_edit` is un-guarded, `rule.rs:571-574`). This is a
   safety-invariant move (constitution invariant 4) and needs its own test (see below).
8. W1 report plumbing (the full list in section 4): `FixStatus::Suggested(edit)`; `has_unresolved`
   and the `FixReport` counters extended; `ExpectStep.suggested` + `assert_fix_status` arm;
   `fix-report.json` `suggested` status + round-trip test.
9. W2 scaffolding: the `SPAWNING_FIX_OPS` SSOT (empty) scanned in the reject-at-load cluster. No
   spawning op exists yet, so there is nothing to refuse and nothing to test until Phase 3.
10. Build the **`coverage_audit_fix_coverage.rs` gate** (rung 8): enumerate ops/fixers and require a
    fix + idempotence scenario each. It will initially require back-filling scenarios for the shipped
    12 ops that lack them (a bounded, in-scope task).
11. New CLI flags on `Command::Fix` (`cli.rs:208-224`), threaded to `cmd_fix`: `--unsafe-fixes` (inert
    until Phase 1 ships the first Unsafe op, but wired now), `--diff` (unified diff of would-apply
    edits; pairs with `--dry-run`), `--fix-only` (filter `entries` to `rule.fixer().is_some()`,
    distinct from the global `--only <id>`). `fix --dry-run` remains the CI gate (no `fix --check`).

**Downstream artifacts.** ARCHITECTURE.md execution step 9 + the "walk once" invariant text at `:39`
and `:353` (now "once per fix pass"); `fix-report.json` + round-trip test for the new status; no new
op, so no `facts.json`/README count change.

**Test coverage.**
- Unit: `ReplaceRange` splice (insert = empty range, delete = empty content, replace); `SetMode`
  application (unix) + skip-with-`Skipped`-note (non-unix); tier filter; total-order sort determinism;
  overlap-skip picks the total-order winner; **isolation-group** exclusion; re-parse-fail -> Suggested;
  **PutGet-fail (parses but wrong value/type/separator) -> Suggested** (distinct from re-parse-fail).
- Parity: keep the existing per-fixer guards; add one proving the new `collect_edits` default matches
  `fix_edit` byte-for-byte for the 12 existing fixers (the "genuine no-op" proof).
- Size guard: a unit + Layer-A test that an over-limit file is skipped (Skipped) at the **collect**
  step for a located op, and that collect does not read it unbounded (invariant 4 on the new path).
- Suggested plumbing: the W1 unit (error-level Suggested -> nonzero exit) + a Layer-B trycmd asserting
  the exit code.
- e2e Layer A: re-run all **17** existing `fix/**/*.yml` unchanged (the no-op regression, including
  `interactions/multiple_fixes_in_one_pass.yml`); add the adversarial same-file pairs
  (`no_trailing_whitespace` + `final_newline`; `file_header` prepend + `max_consecutive_blank_lines`)
  asserting both fixes still apply (a naive `0..len` overlap-skip would drop one).
- e2e Layer B: `fix --diff` and `fix --fix-only` trycmd cases; regenerated `help-fix.stdout` (restore
  `[EXE]`).
- Kani: a bounded proof that the overlap detector yields a pairwise-disjoint applied set, OR that the
  splice primitive is byte-correct (sequel to `confine_steps_is_sound`). **Fix R-KANI in the same PR**
  (the `-p` arg + a proof-count assertion), since the harness verifies nothing today.
- Perf: a `fix --dry-run` cell in `det_check.rs` PLUS a `fix_throughput.rs` wall-clock/syscall cell for
  the collect read-path (the det gate is I/O-blind, R-DETGATE); a dry-run cell is single-pass and
  cannot exercise the fixpoint.

Note: tier-monotonicity and Safe-descent laws do **not** land here (vacuous without a non-Safe op and
a live fixpoint); they move to Phase 1.

**Acceptance gate.** The whole existing suite is byte-identical green (the no-op proof); the new
primitives/flags/gate/plumbing are covered; R-KANI is fixed and the Kani job reports N>0 proofs.
**Risk: low** for user-visible behavior; **medium** for scope (the engine rework + W1 plumbing + the
coverage-gate back-fill are the bulk of the work).

## 6. Phase 1: located content replacement and the fixpoint

**Goal.** Ship the first located-edit op, activate the fixpoint, and land the content-fixer trust
gate. This is where classes 2 and 4 become real, and the first Unsafe op appears.

**Code changes.**
1. New `replace` op (`FixSpec` variant + `ReplaceFixSpec { replacement }`): a Rust regex (host rule's
   `pattern:` on `file_content_forbidden`/`_matches`, or `matches:` on `*_path_matches`) plus a
   replacement template with capture substitution, emitting one `ReplaceRange` per match via
   `collect_edits`. Unsafe by default; Safe only when the replacement is provably a normalization.
2. **Activate** the fixpoint + index-invalidation re-walk (built dormant in Phase 0): a path-mutating
   edit forces a deterministic re-walk before the next pass; content-only passes re-check touched
   files; loop to a fixed point with a loud non-convergence cap. **`--changed` interaction:** the
   fixpoint confines writes to the `--changed` set plus files a fix in scope created; a required
   out-of-scope write is demoted to Suggestion (5.7). Note the existing `--changed` carve-out
   (cross-file + existence rules already see the full tree), so the re-walk spans two scopes.
3. LSP: map `ReplaceRange` to a minimal `TextEdit` with a precise `Range`, converting the byte offset
   to UTF-16 line/character from the `bytes` the fixer receives (R-UTF16).
4. W2 content-fixer trust (section 4): thread provenance onto `RuleSpec`/`RuleEntry`; demote remote-URL
   content fixers to Suggestion; add `trusted_extends:`; **also demote the existing inline-content
   `file_create`/`file_prepend`/`file_append` from remote-URL `extends:`** (a migration note + a
   deprecation entry). Promotion top-level-only.

**Downstream artifacts.** `config.json` `$defs/fix` (+`replace`), new `*Fixer` struct (count bump),
`facts.json`, README headline, `docs/rules.md`, ARCHITECTURE fix-ops table; `trusted_extends:` schema.

**Test coverage.**
- Unit: capture substitution; anchored vs global; multiline; no-match no-op; Unsafe-by-default; a
  provably-normalizing Safe case; `build_rejects` on non-host rules.
- e2e Layer A: `replace` removes a banned token and rewrites a captured span; a two-rule same-file
  overlap where the total order decides and the loser defers to the next pass; a cross-fixer
  non-convergence that hits the cap (assert the loud error); a `--changed` case where an out-of-scope
  write is demoted to Suggestion.
- e2e Layer B: a `fix --unsafe-fixes` trycmd (default `fix` leaves the Unsafe `replace` unapplied;
  `--unsafe-fixes` applies it) - this exercises the new threshold end to end.
- Trust matrix (W2): the four provenance classes each pin a tier (top-level / local-path / bundled =
  honored; remote-URL = Suggestion), `trusted_extends:` re-honors a named remote, promotion from a
  non-top-level source is refused, and an existing `file_prepend` from a remote `extends:` is demoted.
  Extend the `crates/alint-dsl` loader tests + a coverage-audit gate.
- proptest: tier-monotonicity; Safe-multiset descent (debug-assert + `<= |V|` convergence, with the
  non-conflicting-rule-set generator caveat); order-independence-modulo-reproducibility for disjoint
  `replace` edits; fixpoint termination under the cap for a non-idempotent Unsafe `replace`. Requires
  the `Step` + `Engine::fix` + `run_step` `--unsafe-fixes` harness change.
- Perf: a `fix --dry-run` cell over a multi-match `replace` tree, plus a `fix_throughput.rs` cell for
  the write path.

**Decision to record.** Phase 1 defers `replace`'s `proposed_edit`/SARIF surface to Phase 2 (W3), so
v0.17 `check --format sarif` does not carry `replace` fixes.

**Acceptance gate.** `replace` fires/silents/converges; the cap is loud; the four-class trust matrix +
`trusted_extends:` pass; tier/descent laws pass. **Risk: medium** (first real engine behavior change;
the fixpoint, the `--changed` confinement, and the trust plumbing are the load-bearing parts).

## 7. Phase 2: structured value edits (the flagship)

**Goal.** Make **24 of the 25** structured-query kinds fixable via format-preserving `set_value` /
`remove_value` (the 25th, `json_schema_passes`, is not a JSONPath rule; it maps to the deferred
JSON-Schema-guided fill of section 10). The biggest differentiator. Two hard parts: **locate** the
node's byte span (5.3) and **serialize** the value (5.4), driven by the rule-level `collect_edits`
binding that re-runs the query with `query_located` (present on the `serde_json_path` 0.7.2 pin).

**Phase 2 prelude (build first, all sub-phases depend on it).** The two ops (`set_value`,
`remove_value`); a `SpanResolver` trait (normalized path -> byte range) and a `ValueSerializer` trait
(value -> format-correct bytes), with a `Format::ALL` parity gate over both (section 3); the
`query_located` re-query in `collect_edits`; **and fix-parsing on the structured-query builders**:
`build_equals`/`build_absent`/`build_matches` (`structured_path.rs:558-641`) ignore `fix:` today, so
teach the 3 shared helpers to honor their one legal op (`set_value` on `*_path_equals`, `remove_value`
on `*_path_absent`) and emit the `not compatible` rejection for any other op. Only after the prelude do
the per-format resolvers/serializers land.

**Sub-phases, ordered strictly by dependency cost** (value lands early):

| Sub | Formats | Dep reality (verified) |
|---|---|---|
| 2a | HCL, XML, dotenv, INI | **zero new deps.** HCL: `hcl::edit` (re-export of `hcl-edit` 0.8.8, already prod). XML: `roxmltree` 0.20.0 `Node::range()` / `Attribute::range_value()` (`positions` on; **do not bump to 0.21**, R-ROXML). dotenv/INI: alint-owned (`dotenv.rs`, `ini.rs`), add per-value byte-offset tracking. |
| 2b | TOML | promote `toml_edit` 0.25.11 **dev -> prod** (present today only via `trycmd`; `toml` 1.1.2 does NOT pull it transitively, so this is a genuine new prod dep, R-DEP). `serde_spanned` (already prod) spans only typed `Spanned<T>` deserialization, not an arbitrary JSONPath location, so `toml_edit`'s CST is the realistic substrate. |
| 2c | properties | zero new deps; hand-roll a line/value-span locator over `java-properties` (span-less): separators `=`/`:`/space, backslash continuations, `\uXXXX`. |
| 2d | JSON | **new prod dep `jsonc-parser`** (dprint); records value ranges. The existing `strip_jsonc` (`structured_format.rs:227-312`) is a lossy rewrite, not a span source. |
| 2e | YAML | **new prod dep `saphyr-parser`**, scalar-span splice only (Unsafe until the re-parse postcondition passes); structural edits stay Suggestion. `serde_yaml_ng` is detached. Gate 2e on the open question (is scalar-only enough?) with corpus evidence. |

**Realism (R-CSTMAP).** The `NormalizedPath` is computed over the detached, key-alphabetized
`serde_json::Value` (`BTreeMap`, `preserve_order` off). For scalars in objects this maps back cleanly,
but repeated/sibling nodes are ambiguous: **XML sibling elements sharing a tag** and **TOML
array-of-tables** entries can lose identity between the lossy `Value` and the `roxmltree`/`toml_edit`
CST node. Each per-format resolver must prove NormalizedPath -> CST-node fidelity for these cases, with
an explicit XML-sibling and a TOML-array-of-tables test.

**Code changes.** `set_value` (host `*_path_equals`, reads `path:` + value `equals:`) and
`remove_value` (host `*_path_absent`, reads `path:`). Safe `set_value` is a **scalar replacing an
existing scalar**; object/array values, zero-match insertion, and nested creation are Suggestions (the
lens `get` is undefined on a zero-match path, 5.8). `remove_value` is Unsafe and deletes the node's
separator (trailing comma / whole `key = value` line / element) per format. W3 (SARIF/agent
`proposed_edit`, with a schema-conformance test) and W4 (baseline-aware fix, all three baseline flags)
land here. LSP: Suggestions are now offered as code actions (a human initiates them), multi-file fixes
become a multi-document `WorkspaceEdit`, and code actions carry a pinned document version
(`OptionalVersionedTextDocumentIdentifier`) rejected on drift (5.7).

**Downstream artifacts.** Two new ops through the full checklist; the `Format::ALL` serializer parity
gate; **three** new prod deps across the phase (`toml_edit` 2b, `jsonc-parser` 2d, `saphyr-parser` 2e)
into `Cargo.toml`/`Cargo.lock` + the license/supply-chain gates (a new prod dep of the published
`alint-rules` cannot be `publish=false`).

**Test coverage (per format, the defining bar).**
- Unit: scalar set fire/silent/idempotent; type-fidelity matrix (`"true"` vs `true`); `remove_value`
  separator surgery; **an edit that re-parses but writes the wrong value/type/separator is demoted to
  Suggestion** (the PutGet apply-time gate, distinct from the invalid-document case).
- Golden files: a comment/whitespace/key-order preservation golden per format (the anti-Repolinter
  proof) + a **CRLF golden** (the splice must not normalize EOLs); byte-exact except the target span.
- e2e Layer A: `set_value`/`remove_value` per format; a Suggestion case (object value, nested creation)
  surfaced but not written; a baseline case (grandfathered value skipped, surfaced as Suggestion).
- proptest: GetPut and PutGet per format; structured no-op preserves comments/order (needs the new
  `structured_doc_strategy()` + `jsonpath_into()` generators, section 2).
- W3: `check --format sarif` emits `result.fixes[]` for all tiers and the object validates against the
  SARIF fix schema; `agent`/`json --include-fixes` carry `proposed_edit`; the default check path
  computes no edits (perf-neutral). W4: `fix --baseline`/`--strict-baseline`/`--show-baselined` behave.

**Acceptance gate.** Each sub-phase is independently shippable and green before the next; the goldens
prove format preservation; Suggestions never write; the PutGet gate rejects wrong-value splices;
NormalizedPath->CST fidelity is proven per format. **Risk: medium-high** (the flagship; per-format
serialization + the CST-mapping fidelity + three new deps are the deep work).

## 8. Phase 3: metadata, VCS, and repo-scale cross-file

**Goal.** The permission, VCS-tree, and tree-shaped fix classes; the first spawning fixers.

**Code changes.**
- `chmod` (`SetMode`), host `executable_bit` (set/clear, Unsafe), `shebang_has_executable` (add +x,
  Safe), `executable_has_shebang` (Suggestion). Remove the "chmod auto-apply is deferred" rejection at
  `shebang_has_executable.rs:85-90`. Unix-gated (`#[cfg(unix)] PermissionsExt`); a non-unix `SetMode`
  never enters the apply set and reports `Skipped`.
- `git_untrack` (**the first spawning fix op**), host `file_absent` (and `no_committed_binaries` once
  that kind exists): `git rm --cached` plus an optional `.gitignore` line (`gitignore: bool`, default
  true). This is where W2's **spawning-fixer refusal goes live**: add `git_untrack` to
  `SPAWNING_FIX_OPS` and land the spawn-parity gate (section 3).
- `command`-backed fix op (design 5.6): a user-supplied fix command on the `command` rule; a spawning
  fixer, so top-level-only, in `SPAWNING_FIX_OPS`, gated as `git_untrack`. Distinct from the deferred
  regenerate-from-command (section 10).
- `sync_from` (whole-file copy from the host `cross_file` rule's `source:`; introduces no
  `content_from:`-style field; `cross_file` parses no `fix:` today, so this is new fix-block plumbing
  on that kind), and cross-file **create-and-register** via the multi-file transaction (all-or-nothing
  through verify, best-effort per-file writes with a loud partial-apply report).
- `dir_create` (host `dir_exists`), Safe.

**Test coverage.** Full per-op matrix (rung 8 enforces it). Layer-A: chmod (unix-tagged);
`git_untrack` incl. the negatives **run-twice on an already-untracked path** and **on a non-git tree**;
`sync_from` (multi-file `expect_tree`); `dir_create`. Layer-B chmod trycmd wrapped `#[cfg(unix)]` +
a non-unix Skipped-status expectation. Spawn gate: `git_untrack` and the `command`-fix are refused from
a non-top-level `extends:` (the spawn-parity gate). Multi-file transaction: a **fault-injection seam**
(a read-only-file or path-collision fixture, or a `write_atomic` hook) so "mid-batch verify failure
writes nothing" and "a real per-file write failure reports a loud partial apply" are actually testable
(the testkit has no such seam today; building it is part of this phase). **Risk: medium** (git
spawning, the honest multi-file semantics, and the new fault-injection harness).

## 9. Phase 4: ordering, canonicalization, and headers

**Goal.** The clean deterministic wins.

**Code changes and op surfaces.**

| Op | Host kind | Reads / new fields | Tier |
|---|---|---|---|
| `sort` | `ordered_block` | reuses `comparator`/`start`/`end`/`select`/`unique` | Safe |
| `dedup` | `ordered_block` **only** | the marked-block bounds | Safe |
| `indent_style` | `indent_style` | the rule's declared indent (width/kind) | Safe for pure leading indentation only; Unsafe otherwise |
| gitignore/gitattributes insert | (a new `insert_line` op or `file_append`-with-presence-guard; surface TBD) | the required line(s) | Safe, presence-guarded |
| `insert_header` | `file_header` | `text` (literal header) + `comment_style` (`line`/`block`/`auto`) | Safe, presence-guarded |

A `unique_by` collision has no Safe fix (ambiguous survivor) and routes to Suggestion (section 10).
`insert_header` is comment-style-aware (by extension), shebang- and xml-decl-aware, with a `.license`
sidecar for uncommentable types, and is **distinct from** the existing `file_prepend`-on-`file_header`
fix (verbatim bytes): a rule carries one or the other.

**Test coverage.** Full per-op matrix. Concrete negatives: `insert_header` when a header already exists
**in a different comment style** (presence-guard must not double-insert); `insert_header` vs the
existing `file_prepend`-on-`file_header` fix (both do not fire); `indent_style` on **mixed tab/space
leading whitespace** (the Safe/Unsafe boundary); `dedup` where the survivor is ambiguous (-> Suggestion,
not applied); a presence-guarded insert run twice (idempotent). **Risk: low-medium** (self-contained
transforms; `indent_style` needs care to stay Safe only for pure indentation).

## 10. Deferred and special

Not numbered phases; each needs an explicit opt-in and its own design record when demanded.

- **Reference/version pinning (class 7)** and **regenerate-from-command**: reach outside the tree
  (network or a spawned generator), so top-level-only, explicitly opted in, spawn-gated, or a future
  WASM plugin. Never the default binary; preserves telemetry-free-at-runtime. (The `command`-backed
  fix of Phase 3 is a *different* thing: a user-supplied command, not an inferred regenerator.)
- **JSON-Schema-guided fill** (`json_schema_passes`, the 25th structured-query kind): synthesizing a
  document to satisfy a schema is ambiguous; Suggestion at most, deferred.
- **Whole-file reprint (class 1)**: out of scope (alint is not a formatter).
- **`line_max_width` reflow; encoding transcode**: unsafe or ambiguous; Suggestion at most.
- **`unique_by` collision auto-resolution**: no Safe fix (ambiguous survivor); stays a Suggestion.

## 11. Risk register

| ID | Risk | Mitigation |
|---|---|---|
| R-KANI | **Live bug:** `kani.yml` runs `-p alint-rules`, which has zero `#[kani::proof]`; the only proof (`confine_steps_is_sound`) is in `alint-core`, so the weekly job verifies nothing and is vacuously green | fix the `-p` arg (do not "add a proof to alint-rules" instead: the new proof belongs in `alint-core` as a `confine_steps_is_sound` sequel) AND add a proof-count assertion (job fails if N=0 harnesses checked); land both in the Phase 0 Kani PR |
| R-DETGATE | the deterministic gate is advisory (`DET_PERF_ADVISORY=1`, PR-only) and **I/O-blind** (measures `Ir`, not file reads); the new collect step is read-heavy, and a `fix --dry-run` cell is single-pass | pair the det cell with a `fix_throughput.rs` wall-clock/syscall characterization of the collect read-path; state the perf rung is advisory, not blocking |
| R-UTF16 | LSP `Position.character` is a UTF-16 code unit; the current code dodges this by replacing whole documents; a `ReplaceRange` needs offset -> UTF-16 conversion | convert from the `bytes: &[u8]` the fixer receives; add multi-byte / emoji / CRLF unit tests before shipping minimal-diff LSP edits |
| R-EXE | regenerating `help-*.stdout` on Linux strips the trycmd `[EXE]` placeholder, reddening Windows CI | hand-restore `Usage: alint[EXE] <sub>` after every `TRYCMD=overwrite` |
| R-ROXML | bumping `roxmltree` past 0.20 reintroduces a deep-nesting stack overflow the pin guards against | keep the 0.20 pin; a dependabot bump's red CI can mask it, so verify the `xml_deeply_nested` guard |
| R-DEP | Phase 2 adds **three** prod deps (`toml_edit` 2b dev->prod, `jsonc-parser` 2d, `saphyr-parser` 2e) to the published `alint-rules` (supply-chain + new-crate publish gate) | run the supply-chain / license gates; a new prod dep of a published crate cannot be `publish=false`; stage each in its own reviewable commit |
| R-TWOOP | `FixSpec` is `#[serde(untagged)]` and the variant wrappers lack `deny_unknown_fields`, so a two-op `fix:` block silently deserializes as the first-declared matching variant and **drops the second op** at Rust load; only the JSON-Schema `oneOf` (editor/LSP) rejects it | for a file-mutating feature, editor-only rejection is insufficient: add an explicit load-time guard (reject >1 op key) + a test. Not optional |
| R-PROV | the content-fixer trust demotion needs per-source provenance that does not exist in the loader today | build it at the single classification site `loader.rs:143-158` and thread onto `RuleSpec`/`RuleEntry`; adversarial four-provenance-class `extends:` tests; treat as security-load-bearing |
| R-RETRO | W2's demotion retroactively changes the three existing inline-content ops (`file_create`/`file_prepend`/`file_append`) from auto-apply to Suggestion when they arrive via a remote `extends:` (a behavior change for existing ops, not a no-op) | announce it (a migration/deprecation note); `trusted_extends:` is the opt-back-in; amend DoD item 3 to exclude this case |
| R-CSTMAP | the `NormalizedPath` is over the detached key-alphabetized `Value`; repeated/sibling nodes (XML siblings sharing a tag, TOML array-of-tables) may not map cleanly back to the CST node | each per-format resolver proves NormalizedPath->CST-node fidelity with an XML-sibling and a TOML-array-of-tables test before that format's `set_value`/`remove_value` is Safe |
| R-FILEREMOVE | the `file_remove`->Unsafe flip reds every existing scenario that asserts a default `fix` removes a file, and DoD item 3 will not catch it (existing op, changed default) | the flip PR migrates the enumerated scenarios/trycmd (section 4, W5) and tests the v0.17 deprecation warning |
| R-WALK | the Phase 1 re-walk amends ARCHITECTURE's "walk once per invocation" invariant (stated at `:39` and `:353`) | update both ARCHITECTURE.md sites for the `fix` path; keep `check` unchanged; the re-walk is deterministic |
| R-SHARED-WORKTREE | parallel implementation agents in one checkout can contaminate each other's builds | any agent that mutates tracked source runs in an isolated worktree branched from the feature tip (not stale `origin/main`) |

## 12. Sequencing, milestones, and versioning

Aligns with `auto-fix.md` section 9 (which ties v0.17 to "introduces the tiers and a deprecation
warning"):

- **v0.17 = Phase 0.** The tiers, the primitives, the batched engine + verify machinery (dormant),
  the `--unsafe-fixes`/`--diff`/`--fix-only` flags, the report/exit plumbing, the coverage gate, and
  the R-KANI fix. Ships the deprecation warning that `file_remove` will become Unsafe. No new
  user-facing fixer (a foundation release), so it is a genuine no-op.
- **Next minor = Phase 1.** `replace` + the active fixpoint + `--changed` confinement + the
  content-fixer trust gate (`trusted_extends:`). This is the first behavior-changing minor.
- **Phase 2 (2a -> 2e)**, each format sub-phase its own shippable increment; the flagship value
  (HCL/XML/dotenv/INI) lands first with zero new deps. `file_remove` flips Safe -> Unsafe about two
  minors after v0.17 (its own migration PR, R-FILEREMOVE).
- **Phase 3, then Phase 4.** Metadata/VCS/cross-file (the first spawning fixers + the spawn gate),
  then the ordering/header wins.
- **Deferred** items stay demand-gated.

Each phase is one PR following the repo's phased-rollout convention (one commit per phase, or a small
series with a forward `Next: Phase N` pointer), keeps `ROADMAP.md` untouched until the phase is
actually scheduled, and lands its downstream-artifact updates in the same PR.

## 13. Definition of done

A phase is done when, and only when:

1. Every applicable rung of section 2 is green in CI (unit, `build_rejects`, parity, e2e Layer A + B,
   proptest, Kani where a proof was added, perf, the coverage gate, schema/doc-drift, dogfood).
2. The section 3 downstream artifacts are updated in the same PR and their gates pass.
3. The phase is a no-op for configs that do not use its new ops - **with one deliberate exception:
   W2 (Phase 1) demotes existing inline-content ops from a remote `extends:` to Suggestion
   (R-RETRO), and the `file_remove` flip changes an existing default (R-FILEREMOVE); those two
   intentional behavior changes are announced and migration-tested, not silent.**
4. `alint` dogfoods clean on its own repo.
5. The design doc's false-positive/safety surface (auto-fix.md section 7) has a corresponding test
   for each mitigation the phase touches (including isolation groups and the trust gate).
6. The formal contracts the phase relies on (auto-fix.md section 5.8) are encoded as proptest laws or
   debug-asserts (e.g. Safe-multiset descent as a fixpoint-loop `debug_assert!`), not just prose.
7. The new op appears in the `coverage_audit_fix_coverage.rs` gate with a fix + idempotence scenario
   (rung 8), so "full coverage" is enforced by a gate, not claimed in prose.

The bar is deliberately high: auto-fix mutates users' files, so "a fix is not done until a gate
asserts its invariant."

---

*Revision note: revised once after a round-1 adversarial audit (three independent passes -
code-accuracy, gaps/coherence, and test-coverage - plus a self-review). The audit confirmed the plan's
seams and dependency reality were accurate; the revision (a) corrected five factual imprecisions (the
`16 rules / 12 fixer structs` phrasing, the `17` recursive fix-scenario count, `xtask/src/facts.rs:806`,
`24 of 25` structured kinds, the LSP arm line); (b) completed the Safe acceptance test in the engine
(re-parse PLUS localized-equivalence/PutGet + memoized demotion) and restored isolation groups; (c)
re-sequenced the trust gate (content-demotion in Phase 1, spawning-refusal scaffolding in Phase 0 but
first live with `git_untrack` in Phase 3) and surfaced its retroactive demotion of existing content
ops (R-RETRO) and the `file_remove` flip migration (R-FILEREMOVE); (d) added the missing test harness
capabilities the laws require (drive `--unsafe-fixes`, assert `Suggested`, a structured-document
generator, a multi-file fault-injection seam) and a mechanical per-op coverage gate; (e) closed the
dropped interaction surfaces (`--changed` + fixpoint confinement, the LSP Suggestion/version-pinning
work, the `command`-fix op); (f) noted the structured-query builders ignore `fix:` today (Phase 2 must
add rejection); (g) fixed R-DEP to three prod deps, sharpened R-KANI (a live bug: the current proof
does not run) and R-TWOOP (a real load-time guard), and added R-CSTMAP, R-RETRO, and R-FILEREMOVE; and
(h) realigned the versioning so v0.17 = Phase 0. It also corrected a §5.5-vs-§7 contradiction in the
now-accepted `auto-fix.md` (remote-URL-only demotion) in the same PR.*
