# Auto-fix implementation plan

Status: Planning (execution companion to the accepted design). This doc turns the
[`auto-fix.md`](auto-fix.md) framework (accepted) and [ADR-0017](../adr/0017-auto-fix-edit-model-and-applicability.md)
(accepted) into a phased, test-first build plan grounded in the current code. It does not
re-argue the design; it says, per phase, exactly what changes, where, how it is tested, which
gates must stay green, and what can go wrong. It has been through three rounds of adversarial audit
(three independent passes each: code-accuracy, gaps/coherence, test-coverage) plus self-review; the
revision note at the end records what changed. Detection-side companion:
[`rule-coverage-gaps.md`](rule-coverage-gaps.md).

Conventions: file references are `path:line` into the repo at the time of writing (line numbers
drift by a line or two; treat them as a starting point, not a contract). Each phase follows the
[`TEMPLATE.md`](TEMPLATE.md) spirit and, when scheduled, graduates into its own
`docs/design/vX.Y/` record. This plan is em-dash-free by house rule.

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
| Rule to fixer | `rule.rs:390-392` (`Rule::fixer()`) | **16 rule kinds bind a fixer**, mapping to **12 distinct `*Fixer` structs** (shared, e.g. `FileRemoveFixer` serves 4 kinds: file_absent / no_empty_files / no_submodules / no_symlinks); **all 12 override `fix_edit`**. Fixers live in `crates/alint-rules/src/fixers/{creators,file_ops,hygiene,strip}.rs` |
| Apply loop | `crates/alint-core/src/engine.rs:909-1067` (`Engine::fix`) | plain **serial** `for entry in &self.entries` (`:1006`), `evaluate` `:1034`, `fixer().apply` `:1047`, `FixOutcome`->`FixStatus` `:1046-1053`. No fixpoint, no overlap detection, no re-walk. Signature `fix(&self, root, index, dry_run)` carries **no tier/threshold parameter** |
| Size / atomicity | `read_for_fix` `rule.rs:635-648`, `check_fix_size` `rule.rs:604-629`, `write_atomic` `crates/alint-rules/src/io.rs:104` | limit enforced inside each fixer's `apply`; `fix_edit` **un-guarded by design** (`rule.rs:571-574`); engine never touches disk |
| Report | `FixReport` `report.rs:40-104`; `FixStatus` `report.rs:58-67` (`Applied`/`Skipped`/`Unfixable`); `has_unresolved` `report.rs:106-110` | **`applied`/`skipped`/`unfixable` (report.rs:70-86) and `has_unresolved` are `matches!`-based, NOT exhaustive** (a new variant compiles clean and is silently uncounted); exit chain `report.rs:89-99` -> `main.rs:1012-1018` |
| CLI | `Cli` `crates/alint/src/cli.rs:32`, `Command::Fix` `cli.rs:208-224`, handler `cmd_fix` `main.rs:945-1020` | `--dry-run`/`--changed`/`--base` fix-local; `--baseline`/`--strict-baseline`/`--show-baselined`/`--only`/`--format` global; `--baseline` family **rejected** for non-check at `main.rs:179-186`; `--changed` already carves out cross-file + existence rules (they see the full tree) |
| Structured parse | `crates/alint-core/src/structured_format.rs` (`Format` `:42-51`, `Format::ALL` `:61-70`, `parse` `:72-155`) | returns a **detached, lossy** `serde_json::Value`; `BTreeMap`-backed (keys alphabetized; `preserve_order` off) - so comments, key order, and whitespace are gone before any rule runs |
| Structured-query rules | `crates/alint-rules/src/structured_path.rs` (`Op` `:100-116`; the 3 shared builders `build_absent` `:558` / `build_equals` `:586` / `build_matches` `:614`; thin per-format entry `json_path_equals_build` `:462`) | `query` `:314` is **non-located**; violations carry **no byte span** (never `with_location`); `*_path_equals` = one violation per node, `*_path_absent` = one file-level violation for N nodes. **All three builders IGNORE `spec.fix` today (silent, no catch-all)** because `fix` is a known `RuleSpec` field |
| Located JSONPath | `serde_json_path` 0.7.2 exposes `query_located` -> `LocatedNodeList` -> `NormalizedPath` (`PathElement` = `Name`/`Index`) | present on the **current pin**, no upgrade needed |
| e2e Layer A (in-process) | `crates/alint-e2e/scenarios/**/*.yml` (recursive glob, `tests/scenarios.rs:45-47`; **17** fix scenarios incl. `fix/interactions/{mixed_fixable_and_unfixable,multiple_fixes_in_one_pass}.yml`), schema `crates/alint-testkit/src/scenario.rs`, runner `runner.rs` (`run_step`'s no-threshold `engine.fix` call at `:245`, `assert_fix_status` `:402`) | `given.tree/config/git`, `when: [check, fix, fix_dry_run]` (`Step` = 4 unit variants, no `--unsafe-fixes`), `expect` + **`expect_tree`** (byte-exact). `ExpectStep` has no `suggested:` field (a gap this plan closes). **14** of 17 use `when: [check, fix, check]` (a 15th, `multiple_fixes_in_one_pass`, uses `[fix, check]`); **zero** use `[fix, fix]` |
| e2e Layer B (binary) | `crates/alint/tests/cli/*.toml` (trycmd); fix cases `fix-apply`, `fix-file-{append,prepend,remove,rename}`, `fix-trim` | `.in/` -> run -> `.out/` byte tree + `.stdout`; regen `TRYCMD=overwrite`; `[EXE]` placeholder gotcha in `help-*.stdout`. trycmd has **no per-OS skip** so a unix-only case needs `#[cfg(unix)]` wrapping |
| proptest laws | `crates/alint-e2e/tests/invariants.rs` (**4** laws, `cases: 48`) + generators `crates/alint-testkit/src/strategies.rs` | `check_never_panics`, `fix_dry_run_is_pure`, `fix_is_idempotent`, `fix_converges_when_fully_resolved`. `fixable_scenario_tree` (entry `strategies.rs:81`; rule catalogue `fixable_rule_yaml` `:242-249`) emits only `file_create/remove/rename/append` fixes over ASCII blobs (`file_prepend` and `file_content_forbidden` are NOT in it) |
| Kani | `crates/alint-core/src/pathsafe.rs:159` (`confine_steps_is_sound`, `#[kani::proof]` `#[kani::unwind(7)]`); CI `.github/workflows/kani.yml` | **LIVE BUG (R-KANI): the workflow runs `-p alint-rules`, which has zero `#[kani::proof]`; the only proof is in `alint-core`, so the weekly job verifies nothing** |
| Perf gate | `crates/alint-bench/benches/det_check.rs` (gungraun / Callgrind, fixed-path byte-stable `Ir`, `check`-only); criterion `benches/fix_throughput.rs` exists | the deterministic gate is **advisory** (`DET_PERF_ADVISORY=1`, `ci.yml:313`), PR-only, and **I/O-blind** (measures `Ir`, not file reads) |
| LSP mapping | `crates/alint-lsp/src/lib.rs:771-828` (`fix_edit_to_workspace_edit`, exhaustive match, no wildcard); code actions `:446-520` | `SetContent` arm opens at `:773` and maps to a whole-document `TextEdit` via `whole_document()` (`:763-765`); `Position.character` is **UTF-16** |
| Output formats | `crates/alint-output/` (8: agent/github/gitlab/human/json/junit/markdown/sarif) | fix renderers exist only for **human/json/markdown**, whose `FixStatus` matches ARE exhaustive (`json.rs:219`, `human.rs:483`, `markdown.rs:95`); `cmd_fix` rejects other fix formats at `main.rs:964-973`; `SarifResult` (`sarif.rs:279-297`) has **no `fixes` field** yet |

**Implication.** The plan's weight is: (a) two new `FixEdit` variants + an `Applicability` type + a
threshold parameter on `Engine::fix`; (b) a rule-level `collect_edits` binding **returning a
`CollectedEdit` that carries the edit's own verification obligation** (see 5, item 3); (c) the
batched located-edit engine + fixpoint (built dormant first); (d) the per-format
locate-and-serialize bridge (Phase 2), which also has to *add* fix-parsing to the structured-query
builders that ignore it today; (e) the trust/provenance gate (genuinely new loader plumbing); (f)
new CLI flags, LSP UTF-16 mapping, and SARIF/agent `proposed_edit` emitters; and (g) **test-harness
extensions** that do not exist yet (a `fix_unsafe` scenario step, an assertable `Suggested` status,
per-format structured-document generators, and a write-fault-injection hook). Every code seam has
an existing sibling to copy.

## 2. The verification model every phase uses

The user-facing requirement is **full unit and end-to-end integration coverage**, and it must be
**mechanically enforced**, not asserted in prose: today nothing gates that a fixer has a fix
scenario, and the shipped 12 fixers already fall short of the matrix below (only `strip.rs` carries
a parity test; `file_ops.rs` has no idempotence/parity/silent test by name). Phase 0 builds a new
coverage gate (rung 8).

| # | Rung | What it proves | Where / how to add |
|---|---|---|---|
| 1 | **Unit (inline)** | one op does the right byte transform, and rejects incompatible input | `#[cfg(test)] mod tests` in the fixer file (`fixers/strip.rs:248-270`); **plus a `build_rejects` arm on every non-host rule** |
| 2 | **apply/fix_edit parity** | the disk path and the LSP path never diverge | copy `bom_fix_edit_binary_guard_mirrors_apply` (`fixers/strip.rs:346`); one per fixer with both paths |
| 3 | **e2e Layer A (scenario)** | `alint fix` over a real tree yields exact bytes | new `scenarios/fix/<name>.yml`; assert `applied/skipped/unfixable`/`suggested` + `expect_tree`; OS-gate with `tags:` (honored at `scenarios.rs:29-43`) |
| 4 | **e2e Layer B (binary)** | the shipped binary mutates disk, prints the right summary + exit code | new `tests/cli/<name>.toml` + `.in/`/`.out/`/`.stdout`; regen `TRYCMD=overwrite`; restore `[EXE]`; wrap unix-only in `#[cfg(unix)]` |
| 5 | **proptest (laws)** | algebraic invariants hold over generated trees | extend `invariants.rs` + `strategies.rs` (see below; several laws need new harness capability) |
| 6 | **Kani (bounded proof)** | one combinatorial invariant is exhaustively correct | add beside `confine_steps_is_sound` (`pathsafe.rs`); **fix R-KANI first** or the proof does not run |
| 7 | **Perf (advisory)** | surfaces (advisory, non-blocking) a fix-path regression signal | a `fix --dry-run` cell in `det_check.rs` (advisory, and I/O-blind, so pair with a `fix_throughput.rs` wall-clock/syscall cell for the read-heavy collect path, R-DETGATE) |
| 8 | **Coverage gate (NEW)** | every op is actually tested | `coverage_audit_fix_coverage.rs` (Phase 0): see the mechanism below |
| 9 | **Schema + doc drift** | generated surfaces stay in sync | `fix_report_schema.rs` (schema-validation), `in_crate_schema_matches_root`, `gen-schema --check`, `docs-export --check`, `coverage_audit_schema_drift.rs`, the `Format::ALL` parity gates (section 3) |
| 10 | **Dogfood + structural completeness** | alint's repo stays clean; every kind has pass/fail scenarios | the repo `.alint.yml`; `coverage_audit_pass_fail.rs` (`every_registered_rule_kind_has_pass_and_fail_scenarios:148`) |

**Rung 8 mechanism (must be concrete, or it is theatre).** `FixSpec` (`config.rs:391-428`) is
`#[serde(untagged)]` with **no `ALL` const** and `op_name()` is a match, not an iterator, so nothing
can enumerate ops today. Phase 0 adds `FixSpec::ALL: &[&str]` (the op-name axis - 12 names; not the
`*Fixer`-struct axis, which the README count already covers) plus a `fix_spec_op_covers_all` parity
gate mirroring `format_all_is_complete`. The coverage gate then builds `covered_ops` by **parsing
each `fix/*.yml` scenario's inline `given.config` and, for every `applied:` / `suggested:` id across
both `fix` and `fix_unsafe` steps, resolving that id to its rule and reading the rule's `fix:` op
key**. It resolves ops from **inline** rules only, requires at least one inline scenario per op, and
skips an id it cannot resolve inline (a fixer supplied by an extended ruleset), so an
`extends:`-sourced scenario never red-herrings the gate. It asserts `covered_ops` is a superset of
`FixSpec::ALL` and that each op also has a **convergence proof**: a scenario whose second fix pass is
a no-op, accepted in either shape - `when: [fix, fix]` with the second `applied: []` **and no residual
`skipped:`** (else "applied nothing because it skipped" would masquerade as converged), or
`when: [.., fix, check]` ending in `violations: []` (which 15 of the 17 scenarios already satisfy).
All 12 shipped ops already have such a scenario, so the Phase-0 back-fill is expected to be **empty**;
the gate exists to prevent future drift. User-supplied `command`-fix (Phase 3) is **exempt** from the
convergence requirement (idempotence is the user's command's business, not the harness's).

**Per-op / per-flag coverage matrix** (rung 8 enforces the scenario+convergence part): for each new
op, ship `{unit fire, unit silent, unit idempotence, `build_rejects` on non-host rules, parity, one
Layer-A scenario, one Layer-B trycmd}`; for each format in Phase 2 add `{comment/order golden, CRLF
golden, re-parses-but-wrong-value-demoted, invalid-document-demoted, value-serialization matrix}`;
for each new CLI flag add `{Layer-B help snapshot, one exercising scenario/trycmd}`; for each engine
invariant add `{a proptest law or a debug-assert}`.

### Proptest laws, and the harness changes they require

The scenario `Step` enum (`scenario.rs:188-198`) has only Check/Fix/FixDryRun/CheckChanged as
**unit** variants, and `run_step` (`runner.rs:245`) calls `engine.fix(root, index, dry_run)` with no
threshold, so **no test can drive `--unsafe-fixes` today**. Add a new **unit** variant `FixUnsafe`
(YAML `fix_unsafe`) - NOT a struct variant on `Fix`, which would break the bare-string deserialize of
all 17 existing scenarios - plus the threshold parameter on `Engine::fix` and a new `run_step` arm
(the match is exhaustive, no wildcard, so the arm is compiler-forced). Then:

- **Tier monotonicity** (**Phase 1**, once an Unsafe op exists): default `fix` never applies an
  Unsafe/Suggestion edit; `fix_unsafe` is a superset. Vacuous in Phase 0.
- **Safe convergence, stated safely** (**Phase 1**, active fixpoint): the strict "`|V|` strictly
  decreases each pass" law is **not** assertable over the general generator - `file_content_matches`
  appends an `SPDX` line regardless of the drawn pattern (`strategies.rs:304-308`), so on a non-SPDX
  draw a Safe fix applies without resolving its own violation. **Even "`|V|` is non-increasing per
  pass" is too strong for the production loop**: a single fix can resolve one violation while
  introducing others that a later pass clears (the run still converges), so a per-pass non-increasing
  `debug_assert!` would fire in debug builds during the test suite. The production-loop `debug_assert!`s
  therefore assert only the **sound** properties: the fixpoint **terminates under the cap**, and after
  a pass that applies no edit a full re-check finds **no newly-applicable** edit. Strict (monotone)
  descent is asserted only over a **curated proptest pool** where it holds by construction, built by
  **file-disjointness**: N files, each subject to exactly one presence-guarded Safe fixer that
  resolves only its own violation and touches only its own file (A -> final-newline, B -> strip-BOM, C
  empty -> remove, D missing -> create), so `|V|` drops by the number of files fixed per pass and no
  two rules interact. (Seeking globally-non-interacting rules on ONE file is the landmine - even
  `file_collapse_blank_lines` + `final_newline` interact at EOF.) Both the descent and termination
  proptests run over this curated pool, not `fixable_scenario_tree`.
- **Order-independence modulo reproducibility** (**Phase 1**): shuffling independent rule order
  yields the same tree for disjoint edits; overlapping edits are reproducible-not-canonical (5.8, do
  not over-claim confluence).
- **GetPut / PutGet lens laws** (**Phase 2**): these need **new per-format generators**, not a
  "widening" - a `structured_doc_strategy()` per format (8 comment-bearing document generators) plus
  a `jsonpath_into(doc)` path generator, each gated behind its own sub-phase. `strategies.rs` emits
  no structured documents today.

## 3. Downstream artifacts and their gates

A phase is not mergeable until these are updated **in the same PR**. Pre-PR checklist for any change
that adds an op or a status.

| Artifact | When | Gate that catches you |
|---|---|---|
| `crates/alint-core/src/config.rs` `FixSpec` variant + inner `*FixSpec` struct + `op_name()` arm + `FixSpec::ALL` entry (Phase 0 adds `ALL`) | new op | compile + `expecting`-message test (`config.rs:865-871`) + the new `fix_spec_op_covers_all` parity gate |
| The host builder(s) accept the op | new op | per-builder `fix.<op> is not compatible with <kind>` test. **`build_equals`/`build_absent`/`build_matches` AND `file_content_forbidden` (`file_content_forbidden.rs:101-122`) do NOT parse `fix:` today (silent ignore, no catch-all); `file_content_matches` DOES (a `file_append` arm at `:117`). So Phase 1 adds honoring+rejection to `file_content_forbidden` and extends `file_content_matches`, both for `replace`; the Phase-2 prelude adds it to `build_equals`+`set_value`, `build_absent`+`remove_value`, `build_matches`+`replace`** |
| `crates/alint-rules/src/fixers/*.rs` new `pub struct *Fixer` | new op | `readme_auto_fix_ops_count_matches_fixers` (`coverage_audit_readme_claims.rs:310`, counts `*Fixer` structs by text scan) + the NEW `coverage_audit_fix_coverage.rs` (rung 8) |
| `schemas/v1/config.json` `$defs/fix` (hand-written `oneOf` branch, `additionalProperties:false`, `applicability` inside the op object) then `xtask gen-schema` to sync `crates/alint-dsl/schemas/v1/config.json` | new op | `gen-schema --check` + `in_crate_schema_matches_root` (`alint-dsl/src/tests.rs:1079`) |
| `schemas/v1/fix-report.json` (hand-written) + the `FixReport` serializer in `alint-output` | the `suggested` status (**Phase 0**) | `crates/alint-output/tests/fix_report_schema.rs` (renders + **validates against the schema**; NOT gen-schema, and not a serialize/deserialize round-trip) |
| `facts.json` (regenerate via xtask) | new op | `xtask/src/facts.rs:806` cross-check (not the decoy `crates/alint-core/src/facts.rs`) + README headline |
| `README.md` headline "N auto-fix ops" | new op | `readme_auto_fix_ops_count_matches_fixers` |
| `docs/rules.md` per-op reference | new op | `docs-export --check` |
| `docs/design/ARCHITECTURE.md` fix-ops table + execution step 9 (already stale: omits `file_footer`) | new op / the engine rework | `docs-export --check`. (The "walk once per invocation" invariant text at `:39` and `:353` is amended by the Phase-1 re-walk, NOT here - see R-WALK) |
| **`SPAWNING_FIX_OPS` SSOT (new) + TWO gates** | a spawning op (Phase 3) | (a) a **parity** gate mirroring `coverage_audit_spawn_gate.rs` (the set of fix ops that actually spawn == `SPAWNING_FIX_OPS`); (b) an **`extends:`-refusal canary** mirroring `crates/alint/tests/spawn_gate.rs` (each spawning op refused from a non-top-level source). These are two different tests in two different files - do not conflate them |
| **A `Format::ALL` value-serializer parity gate (new)** | Phase 2 | an exhaustive `match` over `Format::ALL` asserting each variant has a `set_value`/`remove_value` serializer or is on an explicit unsupported list (mirrors `format_all_is_complete`, `extract.rs:298`, `json_schema_passes.rs:510`) |
| `ROADMAP.md` / `roadmap.json` | when a phase is scheduled | `gen-roadmap --check` (leave untouched until scheduling) |

## 4. Cross-cutting workstreams

Five threads span phases; scheduled inside the phases.

- **W1 - Tier and report plumbing.** The `Applicability` enum, `FixStatus::Suggested(edit)`, and the
  reporting/exit-code plumbing. Lands in **Phase 0**. The fix formatters' `FixStatus` matches ARE
  exhaustive (`json.rs:219`, `human.rs:483`, `markdown.rs:95`) and the engine mapping
  (`engine.rs:1046-1053`) is compiler-forced; but `FixReport::applied/skipped/unfixable`
  (`report.rs:70-86`), `has_unresolved` (`:106`), the testkit `assert_step` (fn at `runner.rs:299`,
  its `Fix` arm `:321-342`, where the `suggested` assertion goes - NOT the generic
  `assert_fix_status`), and `ExpectStep`
  (`scenario.rs:220`) are **`matches!`/field-based and NOT compiler-forced**, so adding `Suggested`
  silently (a) treats a Suggestion as resolved (wrong exit) and (b) leaves the harness unable to
  assert one. W1 adds: a `FixReport::suggested()` counter + a summary line in all three fix
  formatters (**omitted when the count is zero**, so existing fix `.stdout` snapshots stay
  byte-identical); the `has_unresolved` extension; an `ExpectStep.suggested` field + a `Suggested`
  arm in `assert_step`; and a unit proving an error-level Suggested trips `has_unfixable_errors` ->
  nonzero exit. (The end-to-end Layer-B exit-code trycmd lands in **Phase 1**, the first phase that
  can actually emit a Suggestion.)
- **W2 - Trust and provenance (security).** Per ADR-0017 decision 3, split by first-subject:
  - **Content-fixer demotion + `trusted_extends:`** (**Phase 1**, with `replace`, the first
    content-injecting *new* op reachable via `extends:`). Thread the four-way source (top-level /
    local-path+nested / bundled / remote-URL) captured at the single classification site
    `loader.rs:143-158` onto `RuleSpec` / `RuleEntry` (neither has an origin field today). Content
    fixers from a **remote-URL** `extends:` demote to Suggestion (honored from the user's own tree
    and first-party bundled rulesets); `trusted_extends:` opts specific remotes back in; promotion is
    top-level-only. **This also retroactively demotes the three EXISTING inline-content ops**
    (`file_create`/`file_prepend`/`file_append`) from a remote `extends:` (R-RETRO). The test matrix
    pins all four provenance classes + `trusted_extends:` promotion + the negative.
  - **Spawning-fixer refusal** (**Phase 3**, with `git_untrack`/`command`-fix, the first spawning fix
    ops). Phase 0 builds only the mechanism as *scaffolding*: a `SPAWNING_FIX_OPS` SSOT (empty until
    Phase 3) scanned in the `reject_command_rules_in` cluster (`loader.rs:163-167` / `nested.rs:205`).
    Nothing spawns until Phase 3, so the first real refusal + its two gates (section 3) land there.
- **W3 - Check-side fix output.** SARIF `result.fixes[]` (new `fixes` field on `SarifResult`
  `sarif.rs:279-297`, populated in `base_result` `:194-226`, with a schema-conformance test) and the
  `agent`/`json` `proposed_edit` field (distinct from the existing `agent` `fix_command`). Needs a
  fix computed during `check`, gated to fix-carrying formats only. Lands in **Phase 2**; Phase 1
  explicitly **defers** `replace`'s `proposed_edit`/SARIF surface.
- **W4 - Baseline-aware fix.** `fix` rejects the `--baseline` family today (`main.rs:179-186`, all
  three flags). Make `fix` baseline-aware for all three (skip suppressed, surface as Suggestions, fix
  only new). Lands **by Phase 2**. Extends [ADR-0006](../adr/0006-baseline-suppression.md).
- **W5 - Versioning and deprecation.** Tiers ship in **v0.17** (Phase 0) with a deprecation warning
  that `file_remove` will become Unsafe; `file_remove` flips Safe -> Unsafe about two minors later.
  The flip is not free: `file_remove` backs `file_absent`/`no_empty_files`/`no_submodules`/
  `no_symlinks`, and existing scenarios assert a **default** `fix` removes the file
  (`scenarios/fix/{file_remove,no_empty_files,no_submodules}.yml`, `fix-file-remove` trycmd, and
  `fix/interactions/multiple_fixes_in_one_pass.yml`'s `no-bak` case). The flip PR migrates every one
  (add `fix_unsafe` or `fix: { file_remove: { applicability: safe } }`); DoD item 3 will not catch it
  (existing op, changed default). W5 also specifies where the v0.17 deprecation warning fires (the
  fix-report path) and tests it.

## 5. Phase 0: the fix-engine foundation

**Goal.** Add the substrate every later phase needs, ship **zero** new user-facing fixers, and remain
a **genuine no-op** (byte-identical result) for existing configs. Phase 0 reworks the whole-file
write path (in-memory config-order composition + one write per file) but the resulting bytes are
identical.

**Code changes.**

1. `FixEdit` gains `ReplaceRange { path, range: Range<usize>, content: Vec<u8> }` and
   `SetMode { path, mode: u32 }` (`rule.rs:540-550`). Compiler forces arms in
   `fix_edit_to_workspace_edit` (`lib.rs:771-828`); the `SetMode` arm returns **`None`** deliberately
   (chmod has no LSP `WorkspaceEdit`). `ReplaceRange`'s minimal-`TextEdit` mapping is deferred to
   Phase 1.
2. New `Applicability` enum (Safe/Unsafe/Suggestion/Never). Classify the **existing 12 ops as Safe**,
   except `file_remove` (Safe in v0.17 with a deprecation warning per W5).
3. **The collect contract carries its own verification obligation (R-VERIFY)** (this is the load-bearing type
   the design's translation-validation rests on). Rule-level
   `collect_edits(&[Violation], file, bytes, root) -> Vec<CollectedEdit>` where
   `CollectedEdit { edit: FixEdit, applicability: Applicability, verify: EditVerifier, isolation_group: Option<GroupId> }`
   and `EditVerifier` is an **executable enum** the engine can run without knowing the op:
   `None` (whole-file normalizers - no semantic check) | `Structured { format: Format, query: String, expect: ExpectedValue }`
   with `ExpectedValue = Scalar(serde_json::Value) | Absent`. `query` is the **owned JSONPath source
   string** (the rule's `path_src`), NOT a borrowed `serde_json_path::NormalizedPath`: a
   `NormalizedPath` borrows the parsed `Value` that drops at `collect_edits` return (a dangling
   borrow), and it is insufficient for `Absent` anyway (after a batched multi-node removal array
   indices shift, so only re-running the query and asserting zero matches is correct). This **refines
   ADR-0017 decision 1 / auto-fix.md 5.2.1**, whose `collect_edits -> Vec<(FixEdit, Applicability)>`
   cannot carry the verifier the Safe acceptance test needs. `isolation_group` is assigned by the
   collecting rule (per rule-kind/op) for edits that must not co-apply even when byte-disjoint; its
   `GroupId` type and grouping policy are pinned when the first isolation-needing op ships, and the
   Phase-0 exclusion test uses a fixture rule that tags two disjoint edits into one group. A default
   `collect_edits` delegates to the 12 existing fixers' `fix_edit` with `verify: None` and
   `isolation_group: None`, so they are untouched. Without this contract, the Phase-0 verify machinery
   and the Phase-2 PutGet gate have nothing to build against.
4. Rework `Engine::fix` (`engine.rs:909-1067`) into two regimes: whole-file transforms compose in
   config order in memory (one write per file); located edits go through **collect -> tier-filter ->
   group-by-file -> total-order sort `(start, end, rule_index, violation_index)` -> skip-overlap +
   isolation groups -> verify -> memoized per-(file,edit) demotion-to-Suggestion on verify failure ->
   `write_atomic`**. The `verify` step runs each `CollectedEdit`'s `EditVerifier`: for `Structured`
   it re-parses the post-edit bytes (syntactic) AND re-runs the JSONPath source `query`, asserting
   `.at_most_one() == Scalar` or, for `Absent`, `.is_empty()` (the PutGet localized-equivalence
   check); for `None` it is a no-op. It runs only on
   would-be-applied edits (a Suggestion is never applied, so never verified). `rule_index` spans the
   flattened root+nested-config rule list in deterministic discovery order (5.2.2 of the design), so
   `nested_configs` stays deterministic. **Batch, overlap-skip, isolation groups, verify, and
   fixpoint are built here but dormant** (only whole-file ops ship, `verify: None`).
5. Add the applicability **threshold parameter** to `Engine::fix` (or a builder field) so the CLI
   tier selection reaches the engine; thread `--diff`/`--fix-only` similarly.
6. Thread `WalkOptions` into the engine (new field + builder mirroring `with_fix_size_limit`
   `engine.rs:275-279`) OR hoist the fixpoint loop to `cmd_fix`. Wired in Phase 0; the loop runs once
   (the re-walk activates in Phase 1).
7. Relocate the `fix_size_limit` guard onto the read-only collect step (`fix_edit`/`collect_edits`
   is un-guarded today, `rule.rs:571-574`). Safety invariant 4; needs its own test.
8. **The two-op-block guard (R-TWOOP).** Add a load-time guard that rejects a `fix:` block with more
   than one op key, plus a test. The `#[serde(untagged)]` `FixSpec` dispatch picks the first matching
   variant and silently drops sibling op-keys (the inner structs' `deny_unknown_fields` does not
   apply across the fix-block map), so a file-mutating feature must not inherit this latent bug.
9. W1 report plumbing (section 4): `FixStatus::Suggested(edit)`; the counters/`has_unresolved`
   extended; `ExpectStep.suggested` + the `assert_step` arm; `fix-report.json` `suggested` status +
   its schema-validation test; the zero-count summary-line omission.
10. W2 scaffolding: the `SPAWNING_FIX_OPS` SSOT (empty) scanned at load. No spawning op exists yet, so
    nothing to refuse and nothing to test until Phase 3.
11. Build the **`coverage_audit_fix_coverage.rs` gate** (rung 8) with the `FixSpec::ALL` substrate and
    the parse-config -> resolve-`applied:`-id -> op-key mechanism of section 2; back-fill a
    convergence scenario for any of the 12 ops that lacks one.
12. New CLI flags on `Command::Fix` (`cli.rs:208-224`): `--unsafe-fixes` (inert until Phase 1, wired
    now), `--diff`, `--fix-only`. `fix --dry-run` remains the CI gate.

**Downstream artifacts.** ARCHITECTURE.md execution step 9 + the fix-ops table (the engine rework
legitimately changes step 9); `FixSpec::ALL` + its parity gate; `fix-report.json` + validation test.
No new op, so no `facts.json`/README count change. (The `:39/:353` "walk once" invariant text is
amended in **Phase 1**, not here.)

**Test coverage.**
- Unit: `ReplaceRange` splice (insert/delete/replace); `SetMode` (unix) + Skipped (non-unix); tier
  filter; total-order sort determinism (incl. a nested-config ordering case); overlap-skip winner;
  **isolation-group** exclusion; `EditVerifier::Structured` re-parse-fail -> Suggested AND
  parses-but-wrong-`Scalar` -> Suggested (distinct); the two-op-block rejection (R-TWOOP); the
  size-guard skip at the collect step for a located edit (invariant 4 on the new path).
- Parity: keep the existing per-fixer guards; add one proving the `collect_edits` default matches
  `fix_edit` byte-for-byte for the 12 existing fixers, AND that the engine-generated `FixStatus`
  (Applied/Skipped) and its reason string match today's fixer-generated ones - the rework moves
  status generation into the engine and Layer-B `.stdout` snapshots assert those strings, so the
  no-op proof is not just edit bytes.
- Suggested plumbing: the W1 unit (error-level Suggested -> nonzero exit). (The Layer-B exit-code
  trycmd is in Phase 1.)
- e2e Layer A: re-run all **17** existing `fix/**/*.yml` unchanged (the no-op regression); add the
  adversarial same-file pairs (`no_trailing_whitespace` + `final_newline`; `file_header` prepend +
  `max_consecutive_blank_lines`) asserting both fixes still apply.
- e2e Layer B: `fix --diff` and `fix --fix-only` trycmd cases; regenerate `help-fix.stdout` (restore
  `[EXE]`, R-EXE).
- Kani: a bounded proof (overlap detector yields a pairwise-disjoint applied set, or the splice is
  byte-correct). **Fix R-KANI in the same PR** (the `-p` arg + a proof-count assertion).
- Perf: a `fix --dry-run` cell in `det_check.rs` PLUS a `fix_throughput.rs` wall-clock/syscall cell
  for the collect read-path (the det gate is I/O-blind and advisory, R-DETGATE).

Note: tier-monotonicity and the convergence law land in Phase 1 (vacuous here).

**Acceptance gate.** The whole existing suite is byte-identical green; the new primitives / flags /
gate / plumbing / two-op-guard are covered; R-KANI is fixed and the Kani job reports N>0 proofs.
**Risk: low** behaviorally, **medium** in scope (the engine rework + the `CollectedEdit` contract +
W1 plumbing + the coverage-gate build + the convergence-scenario back-fill are the bulk).

## 6. Phase 1: located content replacement and the fixpoint

**Goal.** Ship the first located-edit op, activate the fixpoint, and land the content-fixer trust
gate. First Unsafe op.

**Code changes.**
1. New `replace` op (`FixSpec` variant + `ReplaceFixSpec { replacement }`): a Rust regex from the
   host rule's `pattern:`, wired to **`file_content_forbidden` and `file_content_matches` only**
   (matching auto-fix.md's Phase 1), emitting one `ReplaceRange` per match via `collect_edits`
   (`verify: None` - regex replace is not a structured op; correctness is byte-locality + the golden,
   not PutGet). The two hosts are **asymmetric**: `file_content_matches` already parses `fix:` (extend
   its existing arm for `replace`), but `file_content_forbidden` does NOT (`file_content_forbidden.rs`
   reads no `spec.fix`), so it needs the full honoring+rejection scaffold added - the same work the
   Phase-2 prelude does for the structured builders. Unsafe by default; Safe only when the replacement
   is provably a normalization. (`replace` on `*_path_matches` is **deferred to Phase 2**, because it
   needs `build_matches` fix-parsing that the Phase-2 prelude adds.)
2. **Activate** the fixpoint + index-invalidation re-walk (built dormant in Phase 0): a path-mutating
   edit forces a deterministic re-walk; content-only passes re-check touched files; loop with a loud
   non-convergence cap. **Apply-once-per-(file, rule, violation-identity)** (Ruff's model): a fix
   already attempted for a violation this run is never re-attempted. This is load-bearing, not an
   optimization: without it a fixer whose fix does not resolve its own violation re-fires every pass -
   e.g. a `file_content_matches` + `file_append` rule whose appended content does not literally
   contain the rule's `pattern:` (an SPDX/boilerplate header) re-appends until the cap: N duplicated
   copies, a **file-corruption regression for an existing valid config** (today's single pass appends
   once). Apply-once restores "append once" and makes "terminate when no NEW fix is applicable"
   provable. **`--changed` confinement:** the fixpoint confines writes to the `--changed`
   set plus files a fix in scope created; a required out-of-scope write is demoted to Suggestion
   (5.7). This **amends ARCHITECTURE.md's "walk once per invocation" invariant (`:39`, `:353`) for
   the fix path only** (`check` still walks once) - update both sites here (R-WALK).
3. LSP: map `ReplaceRange` to a minimal `TextEdit`, converting the byte offset to UTF-16
   line/character from the `bytes` the fixer receives (R-UTF16).
4. W2 content-fixer trust (section 4; R-PROV): provenance onto `RuleSpec`/`RuleEntry`; remote-URL content
   fixers -> Suggestion; `trusted_extends:`; **also demote the existing
   `file_create`/`file_prepend`/`file_append` from a remote `extends:`** (R-RETRO, a migration note).

**Downstream artifacts.** `config.json` `$defs/fix` (+`replace`) + `FixSpec::ALL`; new `*Fixer`
struct; `facts.json`; README; `docs/rules.md`; ARCHITECTURE fix-ops table + the `:39/:353` invariant
amendment; `trusted_extends:` schema.

**Test coverage.**
- Unit: capture substitution; anchored vs global; multiline; no-match no-op; Unsafe-by-default; a
  provably-normalizing Safe case; `build_rejects` on non-host rules.
- LSP: byte-offset -> UTF-16 conversion unit tests (multi-byte, emoji, CRLF) per R-UTF16.
- e2e Layer A: `replace` removes a banned token and rewrites a captured span; a two-rule same-file
  overlap where the total order decides and the loser defers; a cross-fixer non-convergence hitting
  the cap; a `--changed` case where an out-of-scope write is demoted to Suggestion; **an apply-once
  case** - a `file_content_matches` + `file_append` (SPDX header) under the fixpoint applies once
  (second pass `applied: []`, exactly one appended copy), proving the fix is not re-attempted.
- e2e Layer B: a `fix --unsafe-fixes` trycmd (default `fix` leaves the Unsafe `replace` unapplied;
  `--unsafe-fixes` applies it); the W1 error-level-Suggested-> nonzero-exit exit-code case.
- Trust matrix (W2): the four provenance classes each pin a tier; `trusted_extends:` re-honors a
  named remote; promotion from a non-top-level source is refused; an existing `file_prepend` from a
  remote `extends:` is demoted. Extend the `alint-dsl` loader tests + a coverage-audit gate.
- proptest: tier-monotonicity; the termination + curated-pool strict-descent laws (over the
  file-disjoint pool, with the sound `debug_assert!`s, section 2); order-independence for disjoint
  `replace` edits. Uses the new `fix_unsafe` Step.

**Acceptance gate.** `replace` fires/silents/converges; the cap is loud; the four-class trust matrix
+ `trusted_extends:` pass. **Risk: medium.**

## 7. Phase 2: structured value edits (the flagship)

**Goal.** Make the **16** `*_path_equals`/`*_path_absent` kinds format-preservingly fixable via
`set_value`/`remove_value`. Together with the Phase-1 `replace` op extended to the 8 `*_path_matches`
kinds (Unsafe, Never-by-default, user-supplied template), **24 of the 25** structured-query kinds
become fixable across the arc; `json_schema_passes` (the 25th) is the deferred JSON-Schema-guided
fill (section 10). Two hard parts: **locate** the node's byte span (5.3) and **serialize** the value
(5.4), driven by `collect_edits` re-running the query with `query_located`.

**Phase 2 prelude (build first; all sub-phases depend on it).** The ops `set_value` / `remove_value`;
a `SpanResolver` trait (normalized path -> byte range) and a `ValueSerializer` trait (value ->
format-correct bytes), with a `Format::ALL` parity gate over both (section 3); the `query_located`
re-query in `collect_edits`; each edit populates its `EditVerifier::Structured { format, query, expect }`
(`query` = the rule's owned JSONPath source, section 5); **and fix-parsing on the structured-query builders**: teach the 3 shared helpers each to
honor its one legal op and reject the rest - `build_equals` (`:586`) -> `set_value`, `build_absent`
(`:558`) -> `remove_value`, `build_matches` (`:614`) -> `replace` (the `*_path_matches` extension of
the Phase-1 op). Only after the prelude do the per-format resolvers/serializers land.

**Sub-phases, ordered by dependency cost** (value lands early). Note **2a is the heaviest sub-phase**
(four formats, two of them hand-rolled), so sequence its formats internally 2a-lib (HCL, XML;
library-backed spans) then 2a-handrolled (dotenv, INI; new per-value byte-offset tracking in
alint-owned parsers):

| Sub | Formats | Dep reality (verified) |
|---|---|---|
| 2a | HCL, XML, dotenv, INI | **zero new deps.** HCL: `hcl::edit` (re-export of `hcl-edit` 0.8.8, prod). XML: `roxmltree` 0.20.0 `Node::range()` / `Attribute::range_value()` (**do not bump to 0.21**, R-ROXML). dotenv/INI: alint-owned (`dotenv.rs`, `ini.rs`), add per-value byte-offset tracking (the hand-rolled, heavier half). |
| 2b | TOML | add `toml_edit` 0.25.11 as a **new direct prod dep** (present today only transitively via the `trycmd` dev-tool - NOT a declared dev-dep to "promote"; `toml` 1.1.2 does not pull it either, its deps are `toml_parser`/`toml_writer`; R-DEP). `serde_spanned` (already prod) spans only typed `Spanned<T>` deserialization, not an arbitrary JSONPath location. |
| 2c | properties | zero new deps; hand-roll a line/value-span locator over the span-less `java-properties`. |
| 2d | JSON | **new prod dep `jsonc-parser`** (dprint). `strip_jsonc` (`structured_format.rs:227-312`) is a lossy rewrite, not a span source. |
| 2e | YAML | **new prod dep `saphyr-parser`**, scalar-span splice only (structural stays Suggestion). Gate 2e on the open question (is scalar-only enough?) with corpus evidence. |

**Realism (R-CSTMAP).** The `NormalizedPath` is over the detached, key-alphabetized `Value`
(`BTreeMap`). Repeated/sibling nodes (XML siblings sharing a tag, TOML array-of-tables) may not map
cleanly back to the CST node. Each per-format resolver proves NormalizedPath -> CST-node fidelity
with an XML-sibling and a TOML-array-of-tables test before that format's ops are Safe.

**Verify semantics per op (the design's translation validation, made buildable via the
`EditVerifier` of section 5).** `set_value` (host `*_path_equals`, reads `path:` + value `equals:`):
Safe only for a **scalar replacing an existing scalar**, `expect: Scalar(value)` - the engine
re-parses and re-runs the JSONPath source, asserting `.at_most_one() == value`. Object/array values,
zero-match insertion, and nested creation are Suggestions (the lens `get` is undefined on a zero-match
path, 5.8). `remove_value` (host `*_path_absent`, reads `path:`, Unsafe): `expect: Absent` - the
engine re-parses and asserts the JSONPath now matches **zero** nodes. The comparison **normalizes into
the format's value domain**: dotenv / properties / INI parse every value as a string, so a typed
`equals: 8080` is compared in its string form there (else a correct Safe fix would be spuriously
demoted); the typed formats (JSON/YAML/TOML/HCL/XML) compare in-type. Both PutGet checks run on the
**detached parsed `Value`**, which
**cannot see comments, key order, or a wrong separator** - so a wrong-separator or trivia-clobbering
splice is caught NOT by PutGet but by the **comment/order golden files + a byte-locality assertion
(output == input outside the edited `range`)**; the plan assigns that role explicitly rather than
folding it into "PutGet."

**LSP (5.7):** Suggestions are offered as code actions (a human initiates), multi-file fixes become a
multi-document `WorkspaceEdit`, and each action carries a pinned document version
(`OptionalVersionedTextDocumentIdentifier`) rejected on drift. W3 (SARIF/agent `proposed_edit`) and
W4 (baseline-aware fix, all three baseline flags) land here.

**Downstream artifacts.** Two new ops through the full checklist; the `Format::ALL` serializer parity
gate; **three** new prod deps across the phase (`toml_edit`, `jsonc-parser`, `saphyr-parser`) + the
license/supply-chain gates. Binding an op to the 24 structured kinds flips `fixer().is_some()` to
true for them, so the check report's `fixable` flag begins appearing in SARIF/agent/json output even
before any fix runs - add a test that check *violations* are byte-identical before/after binding and
confirm the flag flip is intended. Adding *rejection* of an incompatible op to those builders turns a
today-silently-ignored `fix:` into a new load error - no in-repo config triggers it (verified), but
note the migration for external configs.

**Test coverage (per format, the defining bar).**
- Unit: scalar set fire/silent/idempotent; type-fidelity matrix; `remove_value` separator surgery;
  an edit that re-parses but violates its `EditVerifier` (wrong `Scalar`, or `path` still present for
  a removal) -> demoted to Suggestion.
- Golden files: a comment/whitespace/key-order golden per format + a **CRLF golden** (the splice must
  not normalize EOLs) + the byte-locality assertion (the wrong-separator net).
- e2e Layer A: `set_value`/`remove_value` per format; a Suggestion case (object value, nested
  creation) surfaced but not written; a baseline case (grandfathered value skipped).
- proptest: GetPut and PutGet per format (needs the per-format `structured_doc_strategy()` +
  `jsonpath_into()` generators, section 2); structured no-op preserves comments/order.
- LSP: a code-action-offered-for-Suggestion test, a multi-file `WorkspaceEdit` test, and a
  stale-version-rejected test.
- W3: `check --format sarif` emits `result.fixes[]` for all tiers and validates against the SARIF fix
  schema; `agent`/`json --include-fixes` carry `proposed_edit`; the default check path computes no
  edits. W4: `fix --baseline`/`--strict-baseline`/`--show-baselined` behave.

**Acceptance gate.** Each sub-phase independently shippable and green; goldens + byte-locality prove
format preservation; the `EditVerifier` rejects wrong-value/wrong-removal splices; NormalizedPath->CST
fidelity proven per format. **Risk: medium-high.**

## 8. Phase 3: metadata, VCS, and repo-scale cross-file

**Goal.** The permission, VCS-tree, and tree-shaped fix classes; the first spawning fixers.

**Code changes.**
- `chmod` (`SetMode`), host `executable_bit` (set/clear, Unsafe), `shebang_has_executable` (add +x,
  Safe), `executable_has_shebang` (Suggestion). Remove the "chmod deferred" rejection at
  `shebang_has_executable.rs:85-90`. Unix-gated; a non-unix `SetMode` reports `Skipped`.
- `git_untrack` (**the first spawning fix op**), host `file_absent` (and `no_committed_binaries` once
  that kind exists): `git rm --cached` plus an optional `.gitignore` line (`gitignore: bool`). W2's
  spawning gate goes live: add `git_untrack` to `SPAWNING_FIX_OPS` and land the parity gate + the
  `extends:`-refusal canary (section 3, two separate tests; R-SPAWNGATE).
- `command`-backed fix op (design 5.6): a user-supplied fix command on the `command` rule; a spawning
  fixer, top-level-only, in `SPAWNING_FIX_OPS`. This **lifts the existing `command.rs:285-292`
  rejection** ("command rules do not support fix: blocks") + its test at `command.rs:490-495`
  (analogous to the chmod / `indent_style.rs:175` rejections lifted elsewhere in this arc). Distinct
  from the deferred regenerate-from-command (section 10). Its own fire/silent test; **exempt from the
  rung-8 convergence requirement** (a user's command's idempotence is not the harness's to guarantee).
- `sync_from` (**Unsafe**; whole-file copy from the host `cross_file` rule's `source:`; no
  `content_from:`-style field; `cross_file` parses no `fix:` today, so new fix-block plumbing),
  cross-file **create-and-register** and **cross-file value propagation** (propagate one canonical
  value to N files via per-file `set_value` over the `cross_file`/`unique_by` family), both using the
  multi-file transaction (all-or-nothing through verify, best-effort per-file writes with a loud
  partial-apply report).
- `dir_create` (host `dir_exists`, Safe); **relocate** via `RenameFile` for a lockfile-not-at-root
  whose target is unambiguous (Safe; Suggestion where ambiguous).

**Test coverage.** Full per-op matrix (rung 8). Layer-A: chmod (unix-tagged); `git_untrack` incl. the
negatives run-twice-on-already-untracked and on-a-non-git-tree; `sync_from`; value propagation
(multi-file `expect_tree`); `dir_create`; relocate (the unambiguous-Safe apply AND the
ambiguous-target -> Suggestion case). Layer-B chmod trycmd wrapped `#[cfg(unix)]` + a
non-unix Skipped-status expectation. Spawn gate: the parity gate + the `extends:`-refusal canary for
`git_untrack` and the `command`-fix. **Multi-file transaction:** a decided fault-injection seam - an
**injectable writer on the engine's located-edit write step** (the engine, not the fixer, owns the
write now - itself a real rework of the 12 ops' write path, not dormant scaffolding), exposed via an
`Engine` builder hook gated behind a `test-hooks` cargo feature (testkit enables it as a dev-dep
feature; `pub(crate)` cannot reach a separate crate, so the feature gate, or `#[doc(hidden)]`, keeps
it out of the stable API) - so "mid-batch verify failure writes nothing" and "a real per-file write
failure reports a loud partial apply" are testable. **Risk: medium.**

## 9. Phase 4: ordering, canonicalization, and headers

**Goal.** The clean deterministic wins.

**Code changes and op surfaces.**

| Op | Host kind | Reads / new fields | Tier |
|---|---|---|---|
| `sort` | `ordered_block` | reuses `comparator`/`start`/`end`/`select`/`unique` | Safe |
| `dedup` | `ordered_block` **only** | the marked-block bounds | Safe |
| `indent_style` | `indent_style` (a shipped kind, currently fix-rejected at `indent_style.rs:175`) | the rule's declared indent | Safe for pure leading indentation only; Unsafe otherwise |
| gitignore/gitattributes insert | a new `insert_line` op (surface TBD: host kind + fields) | the required line(s) | Safe, presence-guarded |
| `insert_header` | `file_header` | `text` (literal) + `comment_style` (`line`/`block`/`auto`) | Safe, presence-guarded |

A `unique_by` collision has no Safe fix (ambiguous survivor) and routes to Suggestion (section 10).
`insert_header` is comment-style-aware, shebang/xml-decl-aware, `.license`-sidecar-aware, and
**distinct from** the existing `file_prepend`-on-`file_header` fix (verbatim bytes): a rule carries
one or the other.

**Test coverage.** Full per-op matrix. Negatives: `insert_header` when a header exists **in a
different comment style** (no double-insert); `insert_header` vs the existing `file_prepend` fix (both
do not fire); `indent_style` on **mixed tab/space leading whitespace** (the Safe/Unsafe boundary);
`dedup` with an ambiguous survivor (-> Suggestion). **Risk: low-medium.**

## 10. Deferred and special

Not numbered phases; each needs an explicit opt-in and its own design record.

- **Reference/version pinning (class 7)** and **regenerate-from-command**: reach outside the tree, so
  top-level-only, spawn-gated, or a future WASM plugin. (The `command`-backed fix of Phase 3 is a
  *user-supplied* command, a different thing from an inferred regenerator.)
- **JSON-Schema-guided fill** (`json_schema_passes`, the 25th structured kind): synthesizing a
  document to satisfy a schema is ambiguous; Suggestion at most.
- **Whole-file reprint (class 1)**: out of scope (not a formatter). **`line_max_width` reflow;
  encoding transcode**: Suggestion at most. **`unique_by` collision auto-resolution**: no Safe fix.

## 11. Risk register

| ID | Risk | Mitigation |
|---|---|---|
| R-KANI | **Live bug:** `kani.yml` runs `-p alint-rules`, which has zero `#[kani::proof]`; the only proof is in `alint-core`, so the weekly job verifies nothing | fix the `-p` arg (the new proof belongs in `alint-core`) AND add a proof-count assertion (fail if N=0); Phase 0 |
| R-VERIFY | the Safe acceptance test needs per-edit format/path/expected-value context that a bare `Vec<(FixEdit, Applicability)>` cannot carry | the `CollectedEdit { edit, applicability, verify: EditVerifier, isolation_group }` contract of section 5; built (dormant) in Phase 0 so Phase 2 has something to populate |
| R-DEP | **Three** new direct prod deps in Phase 2 (`toml_edit` - only transitive via `trycmd` today, not a dev-dep to "promote" - plus `jsonc-parser`, `saphyr-parser`) to the published `alint-rules` | license/supply-chain gates; a prod dep of a published crate cannot be `publish=false`; one commit each |
| R-TWOOP | `FixSpec` `#[serde(untagged)]` dispatch picks the first matching variant and silently drops sibling op-keys (the inner structs' `deny_unknown_fields` does not apply across the fix-block map) | a load-time guard rejecting a >1-op-key `fix:` block + a test, scheduled in Phase 0 (item 8) |
| R-CSTMAP | the `NormalizedPath` is over the alphabetized detached `Value`; XML siblings / TOML array-of-tables may not map back to the CST node | per-format NormalizedPath->CST fidelity tests (XML-sibling, TOML-array-of-tables) before that format is Safe |
| R-RETRO | W2 retroactively flips existing `file_create`/`file_prepend`/`file_append` from auto-apply to Suggestion from a remote `extends:` (not a no-op) | announce it; `trusted_extends:` opts back in; DoD item 3 exception |
| R-FILEREMOVE | the `file_remove`->Unsafe flip reds every existing scenario that asserts a default `fix` removes a file; the no-op DoD misses it | the flip PR migrates the enumerated scenarios + tests the v0.17 deprecation warning (W5) |
| R-DETGATE | the deterministic perf gate is advisory and I/O-blind; the collect step is read-heavy and a dry-run cell is single-pass | pair with a `fix_throughput.rs` wall-clock/syscall cell; the perf rung (7) is advisory, not blocking - not a must-be-green DoD gate |
| R-UTF16 | LSP `Position.character` is UTF-16; a `ReplaceRange` needs offset -> UTF-16 conversion | convert from the fixer's bytes; multi-byte / emoji / CRLF tests in Phase 1 |
| R-PROV | the content-fixer trust demotion needs per-source provenance that does not exist in the loader today | build it at the single classification site (Phase 1 W2); adversarial four-provenance-class tests |
| R-SPAWNGATE | the spawn gate is TWO tests (parity + `extends:`-refusal) in two files; conflating them ships only half | Phase 3: a `SPAWNING_FIX_OPS` parity gate (mirrors `coverage_audit_spawn_gate.rs`) AND an `extends:`-refusal canary (mirrors `crates/alint/tests/spawn_gate.rs`) |
| R-ROXML / R-EXE | a `roxmltree` bump past 0.20 reintroduces a stack overflow; regenerated help snapshots strip the trycmd `[EXE]` placeholder | keep the 0.20 pin (verify `xml_deeply_nested`); hand-restore `alint[EXE]` after every `TRYCMD=overwrite` |
| R-WALK | the Phase 1 re-walk amends ARCHITECTURE's "walk once per invocation" invariant (`:39`, `:353`) | Phase 1 updates both sites for the fix path only; `check` stays "walk once" |
| R-SHARED-WORKTREE | parallel implementation agents in one checkout can contaminate each other's builds (a process risk, not a plan-content risk) | any agent that mutates tracked source runs in an isolated worktree branched from the feature tip |

## 12. Sequencing, milestones, and versioning

**v0.17 ships the entire arc (Phases 0 through 4) as one release**, cut only once every phase's
Definition of Done (section 13) is green. This supersedes the earlier "one phase per minor" mapping
and matches `auto-fix.md` section 9: the tiers, the primitives, and every fixer through
ordering/headers ship together, so v0.17 is `alint`'s first *fixing* release rather than a dormant
no-op.

The work is still built and reviewed **phase by phase** - each phase is one PR (or a small series
with a forward `Next: Phase N` pointer) that lands its downstream-artifact updates in the same PR and
merges toward a long-lived v0.17 integration line, never released on its own:

- **Phase 0 (foundation).** The tiers, the primitives, the `CollectedEdit`/verify machinery, the
  fixpoint driver (dormant: no op collects located edits yet), the flags, report/exit plumbing, the
  two-op guard, the coverage gate, and the R-KANI proof-count fix. Ships the `file_remove`
  deprecation warning. A genuine no-op (byte-identical) for existing configs.
- **Phase 1.** `replace` + the active fixpoint + `--changed` confinement + the content-fixer trust
  gate (W2 demotes existing remote-`extends:` content ops to Suggestion, R-RETRO). First Unsafe op.
- **Phase 2** (prelude, then 2a-lib -> 2a-handrolled -> 2b -> 2c -> 2d -> 2e): the flagship
  structured-value edits, each sub-phase its own PR.
- **Phase 3, then Phase 4.**
- **Deferred** items (section 10) stay demand-gated and out of v0.17.

**The one thing v0.17 does NOT do is flip the `file_remove` default.** Reclassifying `file_remove`
from Safe to Unsafe changes an existing default (R-FILEREMOVE, DoD item 3), and the safety contract
(`auto-fix.md` 5.6, 7) requires the deprecation warning to ship at least one full minor before the
flip. So v0.17 ships the warning and **v0.18** flips the default in its own migration PR. This is the
deliberate exception to "the whole arc is v0.17," and it exists so users get one release of warning
before a default changes under them.

`ROADMAP.md` / `roadmap.json` carry the public `## v0.17: Auto-fix` entry; this section is the
source of truth for the internal phase order behind it.

## 13. Definition of done

A phase is done when, and only when:

1. Every applicable **blocking** rung of section 2 is green in CI (unit, `build_rejects`, parity, e2e
   Layer A + B, proptest, Kani where a proof was added, the coverage gate, schema/doc-drift,
   dogfood). The **perf rung (7) is advisory**: its signal is recorded, not required green.
2. The section 3 downstream artifacts are updated in the same PR and their gates pass.
3. The phase is a no-op for configs that do not use its new ops - **with two deliberate exceptions,
   both announced and migration-tested, not silent:** W2 (Phase 1) demotes existing remote-`extends:`
   content ops to Suggestion (R-RETRO), and the `file_remove` flip changes an existing default
   (R-FILEREMOVE).
4. `alint` dogfoods clean on its own repo.
5. The design's false-positive/safety surface (auto-fix.md 7) has a test for each mitigation the
   phase touches (including isolation groups and the trust gate).
6. The formal contracts the phase relies on (auto-fix.md 5.8) are encoded as proptest laws or
   debug-asserts (fixpoint termination-under-cap as a `debug_assert!`; strict/monotone descent as a
   curated-pool proptest, NOT a production per-pass assert - section 2), not prose.
7. Every op in `FixSpec::ALL` is in `coverage_audit_fix_coverage.rs` (rung 8) with a fix +
   convergence scenario - Phase 0 back-fills any existing op that lacks one (expected: none today) -
   or is the explicitly-exempt user-supplied `command`-fix, so "full coverage" is gated, not claimed.

The bar is deliberately high: auto-fix mutates users' files, so "a fix is not done until a gate
asserts its invariant."

---

*Revision note: revised three times after adversarial audit rounds (three independent passes each -
code-accuracy, gaps/coherence, test-coverage - plus self-review). Round 1 completed the Safe
acceptance test, added the coverage gate and the harness-capability list, re-sequenced the trust
gate, added R-RETRO/R-FILEREMOVE/R-CSTMAP, and fixed an auto-fix.md 5.5-vs-7 contradiction. Round 2
fixed issues the round-1 additions themselves introduced: (a) the verify machinery
could not be built against a bare `Vec<(FixEdit, Applicability)>` return, so the collect contract now
returns a `CollectedEdit` carrying an executable `EditVerifier` (which also gives isolation groups a
data home) - R-VERIFY; (b) `replace` was bound to `*_path_matches` in Phase 1 but that builder's
fix-parsing was deferred to Phase 2 (an unbuildable order), so Phase 1 now targets only the content
kinds and the `*_path_matches` extension moves to the Phase 2 prelude; (c) the R-TWOOP guard was
mandatory but scheduled in no phase and its mechanism was mis-described (untagged dispatch drops
sibling keys; the inner structs are guarded) - now scheduled in Phase 0 with the correct mechanism;
(d) the coverage gate had no enumeration substrate (added `FixSpec::ALL`) and no op->scenario
mechanism (now specified), and its idempotence criterion matched zero existing scenarios (now accepts
the convergence shape 15 scenarios use); (e) the `Step` change is a unit variant `fix_unsafe`, not a
struct variant that would break all 17 scenarios; (f) the Safe-descent law's counterexample was
unreproducible (the cited rule pairing cannot occur in the generator) - restated as a non-increasing
law + a curated-pool strict-descent law with a `debug_assert!`; (g) PutGet was over-generalized -
`remove_value` gets an `Absent` post-condition, and wrong-separator/trivia clobbering is reassigned
to golden files + a byte-locality check, since it is invisible to the lossy `Value`; (h) the perf
rung was both "advisory" and a must-be-green DoD gate (now advisory-only); (i) the "walk once"
ARCHITECTURE amendment was double-booked into Phase 0 (now Phase 1 only, fix-path scoped); (j)
cross-file value propagation and relocate (auto-fix.md 2.2 / Phase 3) were dropped (now in Phase 3);
(k) the spawn gate is two tests in two files, not one (R-SPAWNGATE); (l) "24 of 25 via
set_value/remove_value" over-claimed (those two ops cover 16; the 8 `*_path_matches` are `replace`);
plus the fault-injection seam decided (an injectable engine writer), LSP tests scheduled into Phases
1-2, `nested_configs` `rule_index` ordering pinned, `sync_from` tier stated (Unsafe), the
`fixer().is_some()` check-output flag-flip noted, `structured_doc_strategy` scoped per-format, and
factual fixes (`fixable_scenario_tree` = create/remove/rename/append, `fix_report_schema.rs` is
schema-validation not round-trip, the LSP `SetContent` arm at :773). Round 3 confirmed convergence
(all three passes reported the round-2 additions accurate and buildable; no critical or architectural
findings) and fixed their local residue: (a) the `EditVerifier` stored a borrowed `NormalizedPath`
that dangles past the parse and is insufficient for `Absent`, now an owned JSONPath source string
re-run to assert `.at_most_one()`/`.is_empty()` (refining ADR-0017 decision 1 / auto-fix.md 5.2.1);
(b) the active fixpoint would re-apply a non-self-resolving fixer (`file_content_matches`+`file_append`
of a header not containing its own pattern) until the cap - a file-corruption regression for an
existing config - now an apply-once-per-(file,rule,violation) rule; (c) the per-pass `debug_assert!`
cannot require non-increasing `|V|` (a pass can transiently increase it and still converge), so strict
descent is a curated file-disjoint-pool proptest only and the production asserts are termination +
no-newly-applicable; (d) `file_content_forbidden` does NOT parse `fix:` today (unlike
`file_content_matches`), so Phase 1 adds the scaffold there too; (e) the section-3 table still
scheduled `build_matches`+`replace` in Phase 1 (a stale cell, now Phase 2); (f) the coverage gate must
scan `fix_unsafe` steps and resolve ops inline-only (the Phase-0 back-fill is in fact empty). Plus the
cross-format verify-domain normalization (string-only dotenv/INI), the `command.rs` fix-rejection
lifted in Phase 3, the injectable-writer `test-hooks` feature gate, the 14-not-15 `[check,fix,check]`
count, four risk-ID back-links, the `toml_edit` reframe (a new direct dep, not a dev->prod promotion),
and the relocate ambiguous-Suggestion test. The code-accuracy pass found no baseline-fact errors in
any round. The Kani CI live bug (R-KANI) was fixed and verified in a separate PR (#244) - the proof
now runs against alint-core, pinned by harness name.*
