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
| Rule to fixer | `rule.rs:390-392` (`Rule::fixer()`); 16 impls, 12 override `fix_edit` | fixers live in `crates/alint-rules/src/fixers/{creators,file_ops,hygiene,strip}.rs`, exported from `fixers/mod.rs` |
| Apply loop | `crates/alint-core/src/engine.rs:909-1067` (`Engine::fix`) | plain **serial** `for entry in &self.entries` (`:1006`), `evaluate` at `:1034`, `fixer().apply` at `:1041-1052`. No fixpoint, no overlap detection, no re-walk |
| Size / atomicity | `read_for_fix` `rule.rs:635-648`, `check_fix_size` `rule.rs:604-629`, `write_atomic` `crates/alint-rules/src/io.rs:104` | limit enforced inside each fixer's `apply`, not the engine; engine never touches disk |
| Report | check-side `Report` `report.rs:6-35`; fix-side `FixReport` `report.rs:40-104`; `FixStatus` `report.rs:58-67` (`Applied`/`Skipped`/`Unfixable`); `has_unresolved` `report.rs:106-110` | exit chain `report.rs:89-99` -> `main.rs:1012-1018` |
| CLI | `Cli` `crates/alint/src/cli.rs:32`, `Command::Fix` `cli.rs:208-224`, handler `cmd_fix` `main.rs:945-1020` | `--dry-run`/`--changed`/`--base` are fix-local; `--baseline`/`--only`/`--format` are global; `--baseline` is **rejected** for non-check at `main.rs:179-186` |
| Structured parse | `crates/alint-core/src/structured_format.rs` (`Format` `:42-51`, `Format::ALL` `:61-70`, `parse` `:72-155`) | returns a **detached, lossy** `serde_json::Value`; `BTreeMap`-backed (keys alphabetized; `preserve_order` off) |
| Structured-query rules | `crates/alint-rules/src/structured_path.rs` (`Op` `:100-116`, `evaluate_file` `:285-364`) | `query` at `:314` is **non-located**; violations carry **no byte span** (never call `with_location`); `*_path_equals` = one violation per node, `*_path_absent` = one file-level violation for N nodes |
| Located JSONPath | `serde_json_path` 0.7.2 exposes `query_located` -> `LocatedNodeList` -> `NormalizedPath` | present on the **current pin**, no upgrade needed |
| e2e Layer A (in-process) | `crates/alint-e2e/scenarios/fix/*.yml` (15 today), driver `tests/scenarios.rs:45-66`, schema `crates/alint-testkit/src/scenario.rs`, runner `runner.rs:243-282` | `given.tree/config/git`, `when: [check, fix, fix_dry_run]`, `expect` + **`expect_tree`** (byte-exact) |
| e2e Layer B (binary) | `crates/alint/tests/cli/*.toml` (trycmd); fix cases `fix-apply`, `fix-file-{append,prepend,remove,rename}`, `fix-trim` | `.in/` -> run -> `.out/` byte tree + `.stdout`; regen `TRYCMD=overwrite`; `[EXE]` placeholder gotcha in `help-*.stdout` |
| proptest laws | `crates/alint-e2e/tests/invariants.rs` (4 laws) + strategies `crates/alint-testkit/src/strategies.rs` | `fix_dry_run_is_pure`, `fix_is_idempotent`, `fix_converges_when_fully_resolved`, `check_never_panics` |
| Kani | `crates/alint-core/src/pathsafe.rs:161` (`confine_steps_is_sound`, `#[kani::proof]` `#[kani::unwind(7)]`); CI `.github/workflows/kani.yml` (weekly) | see risk R-KANI: the workflow runs `-p alint-rules` but the proof is in `alint-core` |
| Perf gate | `crates/alint-bench/benches/det_check.rs` (gungraun / Callgrind, fixed-path byte-stable `Ir`); criterion `benches/fix_throughput.rs` exists | the deterministic gate covers `check` only today |
| LSP mapping | `crates/alint-lsp/src/lib.rs:771-828` (`fix_edit_to_workspace_edit`); code actions `:446-520` | `SetContent` maps to a whole-document `TextEdit` (`:763-765`); `Position.character` is **UTF-16** |
| Output formats | `crates/alint-output/` (8: agent/github/gitlab/human/json/junit/markdown/sarif) | fix renderers exist only for **human/json/markdown**; `cmd_fix` rejects other fix formats at `main.rs:963-972`; SARIF `result.fixes[]` does **not** exist yet |

**Implication.** The plan's weight is: (a) two new `FixEdit` variants + an `Applicability` type; (b)
a rule-level `collect_edits` binding; (c) the batched located-edit engine + fixpoint (built dormant
first); (d) the per-format locate-and-serialize bridge (Phase 2); (e) the trust/provenance gate
(genuinely new loader plumbing); (f) new CLI flags, LSP UTF-16 mapping, and SARIF/agent
`proposed_edit` emitters. Every one has an existing sibling to copy.

## 2. The verification model every phase uses

The user-facing requirement is **full unit and end-to-end integration coverage**. That is not one
test type; it is a fixed ladder, and every phase must climb all applicable rungs. Each rung already
exists with a copyable pattern.

| Rung | What it proves | Where / how to add | Runs in |
|---|---|---|---|
| **Unit (inline)** | one fixer/op does the right byte transform, and rejects incompatible input | `#[cfg(test)] mod tests` co-located in the fixer file (`fixers/strip.rs:248-270` template): build a `Violation`, call `fix_edit`/`apply`, assert `FixEdit`/`FixOutcome`; plus a `build_rejects_fix_block` arm per non-host rule | every `cargo test` |
| **apply/fix_edit parity** | the disk path and the LSP path never diverge | copy `bom_fix_edit_binary_guard_mirrors_apply` (`fixers/strip.rs`); one per fixer that has both paths | every `cargo test` |
| **e2e Layer A (scenario)** | `alint fix` over a real tree yields exact bytes | new `crates/alint-e2e/scenarios/fix/<name>.yml`; auto-discovered by `dir_test`; assert `applied/skipped/unfixable` + `expect_tree` (byte-exact). OS-gate with `tags:` | every `cargo test` |
| **e2e Layer B (binary)** | the shipped binary mutates disk and prints the right summary | new `crates/alint/tests/cli/<name>.toml` + `.in/` + `.out/` + `.stdout`; regen with `TRYCMD=overwrite`; hand-restore `[EXE]` in any regenerated help snapshot | every `cargo test` |
| **proptest (laws)** | algebraic invariants hold over generated trees | extend `crates/alint-e2e/tests/invariants.rs` + the generators in `crates/alint-testkit/src/strategies.rs` (widen `fixable_scenario_tree` as ops land) | every `cargo test` (48 cases; `PROPTEST_CASES` scales) |
| **Kani (bounded proof)** | one combinatorial invariant is exhaustively correct | add beside `confine_steps_is_sound` (`pathsafe.rs`); `#[kani::proof]` + `#[kani::unwind(N)]` under `#[cfg(kani)]`; **see R-KANI for the `-p` fix** | weekly CI + `cargo kani` local |
| **Perf gate** | no fix-path speed regression | add a `--dry-run` `fix` cell to `det_check.rs` (must be dry-run to keep the byte-stable-path invariant); wall-clock characterization in `fix_throughput.rs` | advisory perf-gate CI |
| **Dogfood** | alint's own repo stays clean under the new rules | the repo `.alint.yml` + the coverage-audit gates in `crates/alint-e2e/tests/coverage_audit_*.rs` | every `cargo test` |

**The per-op / per-flag coverage matrix** (the minimum bar for any phase to be "done"): for each new
op, ship all of `{unit fire, unit silent (no-op when already correct), unit idempotence, parity,
one Layer-A scenario, one Layer-B trycmd case}`; for each format in Phase 2 add
`{comment/order-preservation golden, invalid-output-demoted-to-Suggestion, value-serialization
matrix}`; for each new CLI flag add `{Layer-B help snapshot, one scenario or trycmd exercising it}`;
for each engine invariant add `{a proptest law}`; extend Layer-A tags for any OS-specific op.

### The proptest laws to add (by phase)

`invariants.rs` today encodes dry-run purity, idempotence, convergence, and no-panic. New laws:

- **Order-independence of a Safe-only run modulo reproducibility** (Phase 0/1): shuffling
  independent rule order yields the same tree *for disjoint edits*; document that overlapping edits
  are reproducible-not-canonical (matches 5.8, do not over-claim confluence).
- **Tier monotonicity** (Phase 0): a default `fix` never applies an Unsafe/Suggestion edit; `fix
  --unsafe-fixes` is a superset of default `fix`.
- **Safe multiset descent** (Phase 0/1): a Safe-only pass strictly reduces the outstanding-violation
  count (the Dershowitz-Manna contract, 5.8), so the fixpoint terminates without the cap.
- **Round-trip / lens laws** (Phase 2): GetPut (writing back the current value is a byte no-op) and
  PutGet (after `set_value`, the query returns the intended value) per format.
- **Structured no-op preserves comments/order** (Phase 2): a Safe structured fix touches only the
  targeted span (the golden-file property, generalized).

## 3. Downstream artifacts and their gates

alint gates its own generated surfaces; a phase is not mergeable until these are updated **in the
same PR**. This list is the pre-PR checklist for any change that adds an op or a status.

| Artifact | When | Gate that catches you |
|---|---|---|
| `crates/alint-core/src/config.rs` `FixSpec` variant + inner `*FixSpec` struct + `op_name()` arm | new op | compile + `expecting`-message test (`config.rs:865-871`) |
| `crates/alint-rules/src/fixers/*.rs` new `pub struct *Fixer` | new op (drives the count) | `readme_auto_fix_ops_count_matches_fixers` (`crates/alint-e2e/tests/coverage_audit_readme_claims.rs:309-325`) |
| host builder(s) accept the op; every other builder auto-rejects via its catch-all | new op | per-builder `fix.<op> is not compatible with <kind>` tests |
| `schemas/v1/config.json` `$defs/fix` (hand-written `oneOf` branch, `additionalProperties:false`, `applicability` inside the op object) then `xtask gen-schema` to sync `crates/alint-dsl/schemas/v1/config.json` | new op | `--check` drift + `in_crate_schema_matches_root` |
| `schemas/v1/fix-report.json` (hand-written) + the `FixReport` serializer in `alint-output` | new **status** (`suggested`) | `crates/alint-output/tests/fix_report_schema.rs` round-trip (NOT gen-schema) |
| `facts.json` (regenerate via xtask) | new op | `facts.rs:806` cross-check + README headline |
| `README.md` headline "N auto-fix ops" | new op | `readme_auto_fix_ops_count_matches_fixers` |
| `docs/rules.md` per-op reference | new op | doc-export `--check` |
| `docs/design/ARCHITECTURE.md` fix-ops table + execution step 9 (already stale: omits `file_footer`) | new op / engine change | doc-export `--check`; the fixpoint re-walk also amends the "walk once" invariant text (`ARCHITECTURE.md:353`) |
| `ROADMAP.md` / `roadmap.json` | when a phase is scheduled | `gen-roadmap --check` (leave untouched until scheduling, to keep the gate green) |
| spawn-gate tests (`crates/alint/tests/spawn_gate.rs`, `coverage_audit_spawn_gate.rs`) | a spawning op | those gates (they gate kinds; a spawning fixer needs the new fix-level gate of W2) |

Note: `Format::ALL` parity (`structured_format.rs:61`) is about the 8 file formats, not fix ops;
adding a plain op does not touch it. It IS relevant to Phase 2 (a value serializer per format).

## 4. Cross-cutting workstreams

Five threads span phases. They are called out here and then scheduled inside the phases.

- **W1 - Tier and report plumbing.** The `Applicability` enum, `FixStatus::Suggested(edit)`, and
  extending `has_unresolved` to count `Suggested`. Lands in **Phase 0** (the model exists before any
  fixer uses it). Compiler-forced match arms: `report.rs:106-110`, JSON `crates/alint-output/src/json.rs:207-240`,
  human `human.rs` `write_fix_human`, markdown `markdown.rs` `write_fix_markdown`, engine mapping
  `engine.rs:1046-1053`.
- **W2 - Trust and provenance (security).** Per ADR-0017 decision 3. Split by cost: the
  **spawning-fixer refusal** reuses the existing reject-at-load pattern (`reject_command_rules_in`
  cluster, `loader.rs:163-167` / `nested.rs:205`) and lands in **Phase 0** (a `fix:` block that
  carries a spawning op is refused from any non-top-level source). The **content-fixer demotion +
  `trusted_extends:`** requires new per-source provenance (top-level / local-path / bundled /
  remote-URL) captured at the single classification site `loader.rs:143-158` and threaded onto
  `RuleSpec` / `RuleEntry` (neither has an origin field today); it must land **before Phase 1's
  `replace` can be honored from `extends:`**, so it is a Phase 1 prerequisite.
- **W3 - Check-side fix output.** SARIF `result.fixes[]` (new `fixes` field on `SarifResult`
  `sarif.rs:279-298`, populated in `base_result` `:194-226`) and the `agent`/`json` `proposed_edit`
  field. These need `Fixer::fix_edit`/`collect_edits` computed during `check`, gated to
  fix-carrying formats only. Lands in **Phase 2** (when Suggestions first exist), though the
  `proposed_edit` scaffolding can ride Phase 1.
- **W4 - Baseline-aware fix.** `fix` rejects `--baseline` today (`main.rs:179-186`). Make `fix`
  baseline-aware (skip suppressed, surface as Suggestions, fix only new). Must land **by Phase 2**,
  before structured edits could auto-rewrite a grandfathered value. Extends
  [ADR-0006](../adr/0006-baseline-suppression.md) from `check` to `fix`.
- **W5 - Versioning and deprecation.** The tiers ship in **v0.17** with a deprecation warning for
  `file_remove`; `file_remove` flips Safe -> Unsafe about two minors later. Wire `ROADMAP.md` only
  when scheduling.

## 5. Phase 0: the fix-engine foundation

**Goal.** Add the substrate every later phase needs, while shipping **zero** new user-facing fixers
and remaining a **genuine no-op** for existing configs. Whole-file ops still compose in config order
(they do not go through the overlap machinery), so today's behavior reproduces exactly.

**Code changes.**

1. `FixEdit` gains `ReplaceRange { path, range: Range<usize>, content: Vec<u8> }` and
   `SetMode { path, mode: u32 }` (`rule.rs:540-550`). Compiler forces arms in
   `fix_edit_to_workspace_edit` (`lib.rs:771-828`) and every `FixStatus`/`FixEdit` match.
2. New `Applicability` enum (Safe/Unsafe/Suggestion/Never) beside `FixEdit` (W1).
3. Rule-level `collect_edits(&[Violation], file, bytes, root) -> Vec<(FixEdit, Applicability)>` on the
   `Rule` trait next to `fixer()` (`rule.rs:390`), with a **default** that delegates to the existing
   per-violation `fixer().fix_edit()` so all 12 simple fixers are untouched.
4. Rework `Engine::fix` (`engine.rs:909-1067`) into two regimes: whole-file transforms compose in
   config order in memory (one write per file); located edits go through collect -> tier-filter ->
   group-by-file -> total-order sort `(start, end, rule_index, violation_index)` -> skip-overlap ->
   re-parse verify -> `write_atomic`. **The batch, overlap-skip, and fixpoint are built here but
   dormant** (only whole-file ops ship, so no located edit is produced yet).
5. Thread `WalkOptions` into the engine (new field + builder mirroring `with_fix_size_limit`
   `engine.rs:275-279`) OR hoist the fixpoint loop to `cmd_fix` which already holds the walk. This is
   the structural prerequisite for the Phase 1 re-walk; in Phase 0 it is wired but the loop runs
   once.
6. Relocate the `fix_size_limit` guard onto the read-only collect step (today it lives in each
   fixer's `apply` via `read_for_fix`; the generalized `collect_edits` path must re-apply it, since
   `fix_edit` is un-guarded by design, `rule.rs:571-574`).
7. W1 report plumbing: `FixStatus::Suggested(edit)`; extend `has_unresolved` (`report.rs:106-110`).
8. W2 spawning-fixer refusal: scan each `extends:`-ed rule's `fix:` block for a spawning op and
   refuse at load (reuse `reject_*` cluster).
9. New CLI flags on `Command::Fix` (`cli.rs:208-224`), threaded to `cmd_fix`: `--unsafe-fixes`
   (widen the tier threshold), `--diff` (render a unified diff of would-apply edits; pairs with
   `--dry-run`), `--fix-only` (filter `entries` to `rule.fixer().is_some()`, distinct from the global
   `--only <id>`). `fix --dry-run` remains the CI gate (no separate `fix --check`).

**Downstream artifacts.** ARCHITECTURE.md execution step 9 + the "walk once" invariant text (now
"once per fix pass"); schema for the new report status (`fix-report.json` + round-trip test); no new
op yet, so no `facts.json`/README count change.

**Test coverage.**
- Unit: `ReplaceRange` splice correctness (insert = empty range, delete = empty content, replace);
  `SetMode` mode application (unix) and skip-with-note (non-unix); tier filter; total-order sort
  determinism; overlap-skip picks the total-order winner; re-parse-fail -> `Suggested`.
- Parity: keep the existing per-fixer `apply`/`fix_edit` parity guards; add one that the new
  `collect_edits` default matches `fix_edit` byte-for-byte for the 12 existing fixers (the "genuine
  no-op" proof).
- e2e Layer A: re-run the 15 existing `fix/*.yml` unchanged (they are the no-op regression); add the
  adversarial same-file pairs (`no_trailing_whitespace` + `final_newline`; `file_header` prepend +
  `max_consecutive_blank_lines`) asserting both fixes still apply (a naive `0..len` overlap-skip
  would drop one).
- e2e Layer B: `fix --diff` and `fix --fix-only` trycmd cases; regenerated `help-fix.stdout`
  (restore `[EXE]`).
- proptest: add tier-monotonicity and Safe-multiset-descent laws to `invariants.rs`.
- Kani: a bounded proof that the overlap detector yields a pairwise-disjoint applied set, OR that the
  splice primitive is byte-correct (sequel to `confine_steps_is_sound`). **Place it per R-KANI.**
- Perf: add a `fix --dry-run` cell to `det_check.rs` + a fixpoint-convergence characterization; keep
  `fix_throughput.rs` current.

**Acceptance gate.** The whole existing test suite is byte-identical green (the no-op proof); the new
primitives/flags are covered; `det_check.rs` has a fix cell. **Risk: low** (nothing user-visible
changes behavior).

## 6. Phase 1: located content replacement and the fixpoint

**Goal.** Ship the first located-edit op and activate the fixpoint. This is where classes 2 and 4
become real.

**Code changes.**
1. New `replace` op (`FixSpec` variant + `ReplaceFixSpec { replacement }`): a Rust regex (from the
   host rule's `pattern:` on `file_content_forbidden`/`_matches`, or `matches:` on `*_path_matches`)
   plus a replacement template with capture substitution, emitting one `ReplaceRange` per match via
   `collect_edits`. Unsafe by default; Safe only when the replacement is provably a normalization.
2. **Activate** the fixpoint + index-invalidation re-walk (built dormant in Phase 0): a path-mutating
   edit forces a deterministic re-walk before the next pass; content-only passes re-check touched
   files; loop to a fixed point with a loud non-convergence cap.
3. W2 provenance: thread the four-way source onto `RuleSpec`/`RuleEntry` at `loader.rs:143-158`;
   demote a remote-URL `extends:` content fixer to Suggestion; add the top-level `trusted_extends:`
   allowlist; promotion stays top-level-only. **This must merge with (1)**, because `replace` is the
   first content-injecting new op reachable via `extends:`.

**Downstream artifacts.** `config.json` `$defs/fix` (+`replace`), new `*Fixer` struct (count bump),
`facts.json`, README headline, `docs/rules.md`, ARCHITECTURE fix-ops table.

**Test coverage.**
- Unit: capture substitution; anchored vs global match; multiline; no-match no-op; the
  Unsafe-by-default classification; a provably-normalizing Safe case.
- e2e Layer A: `replace` removes a banned token (`console.log`), rewrites a captured span; a two-rule
  same-file overlap where the total order decides and the loser defers to the next pass; a
  cross-fixer non-convergence that hits the cap (assert the loud error).
- e2e Layer B: a `fix --unsafe-fixes` trycmd case (default `fix` leaves the Unsafe `replace`
  unapplied; `--unsafe-fixes` applies it).
- Trust tests: a remote `extends:` `replace` is demoted to Suggestion; a `trusted_extends:` entry
  re-honors it; a spawning `fix:` from `extends:` is refused at load (W2). These extend
  `crates/alint-dsl` loader tests + a coverage-audit gate.
- proptest: order-independence-modulo-reproducibility for disjoint `replace` edits; fixpoint
  termination under the cap for a non-idempotent Unsafe `replace`.
- Perf: a `fix --dry-run` cell exercising a multi-match `replace` tree.

**Acceptance gate.** `replace` fires/silents/converges; the fixpoint cap is enforced and loud; the
trust demotion + `trusted_extends:` are covered by loader tests. **Risk: medium** (first real engine
behavior change; the fixpoint and trust plumbing are the load-bearing parts).

## 7. Phase 2: structured value edits (the flagship)

**Goal.** Make the 25-kind structured-query family fixable via format-preserving `set_value` /
`remove_value`, the biggest differentiator. Two hard parts: **locate** the node's byte span (5.3) and
**serialize** the replacement value (5.4), driven by the rule-level `collect_edits` binding that
re-runs the query with `query_located` (available on the current `serde_json_path` 0.7.2 pin).

**Sub-phases, ordered strictly by dependency cost** (value lands early):

| Sub | Formats | Dep reality (verified) |
|---|---|---|
| 2a | HCL, XML, dotenv, INI | **zero new deps.** HCL: `hcl::edit` (re-export of `hcl-edit` 0.8.8, already prod). XML: `roxmltree` 0.20.0 `Node::range()` / `Attribute::range_value()` (`positions` on; **do not bump to 0.21**, R-ROXML). dotenv/INI: alint-owned parsers (`dotenv.rs`, `ini.rs`), add per-value byte-offset tracking. |
| 2b | TOML | promote `toml_edit` 0.25.11 **dev -> prod** (present today only via `trycmd`). `serde_spanned` is already prod but only spans typed `Spanned<T>` deserialization, not an arbitrary JSONPath location, so `toml_edit`'s CST is the realistic write-back substrate. |
| 2c | properties | zero new deps; hand-roll a line/value-span locator over `java-properties` (which is span-less): separators `=`/`:`/space, backslash continuations, `\uXXXX`. |
| 2d | JSON | **new prod dep `jsonc-parser`** (dprint); records comment/value ranges. The existing `strip_jsonc` (`structured_format.rs:227-312`) is a lossy string rewrite, not a span source. |
| 2e | YAML | **new prod dep `saphyr-parser`**, scalar-span splice only (Unsafe until the re-parse postcondition passes); structural edits stay Suggestion. `serde_yaml_ng` is detached. Gate 2e on the open question (is scalar-only enough?) with corpus evidence. |

**Code changes.** `set_value` (host `*_path_equals`, reads the target `path:` + value `equals:`) and
`remove_value` (host `*_path_absent`, reads `path:`). Safe `set_value` is restricted to a **scalar
replacing an existing scalar**; object/array values, zero-match insertion, and nested creation are
Suggestions (the lens `get` is undefined on a zero-match path, 5.8). `remove_value` is Unsafe and
must delete the node's separator (trailing comma / whole `key = value` line / element) per format.
Per-format: a span resolver (normalized path -> byte range) and a value serializer (quoting,
escaping, type fidelity `"true"` vs `true`, entity-encoding). W3 (SARIF/agent `proposed_edit`) and W4
(baseline-aware fix) land in this phase.

**Downstream artifacts.** Two new ops (`set_value`, `remove_value`) through the full checklist; two
new prod deps (2d, 2e) into `Cargo.toml` + `Cargo.lock` + the license/supply-chain gates; the
`fix-report.json` `suggested` status (if not already in Phase 0).

**Test coverage (per format, the defining bar).**
- Unit: scalar set fires/silents/idempotent; type fidelity matrix; `remove_value` separator surgery;
  a splice that produces an invalid document is **demoted to Suggestion** (not written).
- Golden files: a comment/whitespace/key-order-preservation golden per format (the anti-Repolinter
  proof); assert byte-exact preservation of everything except the targeted span.
- e2e Layer A: `set_value` on each format's fixture; `remove_value`; a Suggestion case (object value,
  nested creation) that is surfaced but not written; a baseline case (grandfathered value skipped).
- proptest: GetPut and PutGet lens laws per format; the structured no-op preserves comments/order.
- W3 tests: `check --format sarif` emits `result.fixes[]` for all tiers; `agent`/`json
  --include-fixes` carry `proposed_edit`; the default check path computes no edits (perf-neutral).
- W4 tests: `fix --baseline` skips suppressed, surfaces as Suggestions, fixes only new (extend the
  baseline scenario fixtures).

**Acceptance gate.** Each sub-phase is independently shippable and green before the next; the golden
files prove format preservation; Suggestions never write; SARIF/agent carry fixes; baseline-aware fix
works. **Risk: medium-high** (the flagship; per-format serialization is the deep work, and the two new
deps carry supply-chain review + the `hcl-rs`/`roxmltree` stack-overflow discipline).

## 8. Phase 3: metadata, VCS, and repo-scale cross-file

**Goal.** The permission, VCS-tree, and tree-shaped fix classes.

**Code changes.**
- `chmod` (`SetMode`), host `executable_bit` (set/clear, Unsafe), `shebang_has_executable` (add +x,
  Safe), `executable_has_shebang` (Suggestion). The current rejection at
  `shebang_has_executable.rs:85-90` ("chmod auto-apply is deferred") is the seam to remove. Unix-gated
  (`#[cfg(unix)] PermissionsExt`); a non-unix `SetMode` simply never enters the apply set.
- `git_untrack` (spawning, top-level-only per W2), host `file_absent` (and `no_committed_binaries`
  once that kind exists): `git rm --cached` plus an optional `.gitignore` line (`gitignore: bool`,
  default true). Interacts with the spawn-gate tests.
- `sync_from` (whole-file copy from the host `cross_file` rule's `source:`; reads the rule's
  `source:`, introduces no `content_from:`-style field; `cross_file` parses no `fix:` block today, so
  this is new fix-block plumbing on that kind), and cross-file **create-and-register** using the
  multi-file transaction (all-or-nothing through verify, best-effort per-file writes with a loud
  partial-apply report).
- `dir_create` (host `dir_exists`), Safe.

**Test coverage.** Unit per op; e2e Layer A for chmod (unix-tagged), untrack (git-block fixtures),
`sync_from` (multi-file `expect_tree`), `dir_create`; a multi-file transaction test that a mid-batch
verify failure writes nothing, and that a real per-file write failure reports a loud partial apply;
spawn-gate tests for `git_untrack` from `extends:` (refused). **Risk: medium** (git spawning + the
honest multi-file transaction semantics).

## 9. Phase 4: ordering, canonicalization, and headers

**Goal.** The clean deterministic wins.

**Code changes.**
- `sort` (host `ordered_block`, reuses the rule's `comparator`/`start`/`end`/`select`/`unique`) and
  `dedup` (host `ordered_block` **only** - a `unique_by` collision has no Safe fix, its ambiguous
  survivor routes to Suggestion). Both Safe, deterministic.
- `indent_style` leading-indent tab/space conversion (Safe for pure leading indentation only, Unsafe
  otherwise).
- `.gitattributes` / `.gitignore` line insertion (Safe, presence-guarded).
- `insert_header` (host `file_header`): comment-style-aware (by extension), shebang- and
  xml-decl-aware, `.license` sidecar for uncommentable types; new op fields `text` + `comment_style`.
  **Distinct from** the existing `file_prepend`-on-`file_header` fix (verbatim bytes) - a rule carries
  one or the other. Presence-guarded, Safe.

**Test coverage.** Unit per op; e2e Layer A for a sorted block, a deduped block, a header inserted in
each comment style + shebang/xml-decl placement + the `.license` sidecar; idempotence (a
presence-guarded insert never double-inserts). **Risk: low-medium** (mostly self-contained
transforms; `indent_style` needs care to stay Safe only for pure indentation).

## 10. Deferred and special

Not numbered phases; each needs an explicit opt-in and its own design record when demanded.

- **Reference/version pinning (class 7)** and **regenerate-from-command**: reach outside the tree
  (network or a spawned generator), so top-level-only, explicitly opted in, spawn-gated, or a future
  WASM plugin. Never the default binary; preserves telemetry-free-at-runtime.
- **Whole-file reprint (class 1)**: out of scope (alint is not a formatter).
- **`line_max_width` reflow; encoding transcode; JSON-Schema-guided fill**: unsafe or ambiguous;
  Suggestion at most, deferred behind demand.
- **`unique_by` collision auto-resolution**: no Safe fix (ambiguous survivor); stays a Suggestion.

## 11. Risk register

| ID | Risk | Mitigation |
|---|---|---|
| R-KANI | `.github/workflows/kani.yml` runs `-p alint-rules`, but the only proof is in `alint-core` (`pathsafe.rs`); a new proof added elsewhere silently does not run in CI | fix the `-p` arg (or add the proof to `alint-rules`) in the same PR that adds a Kani proof; assert it runs by checking the weekly job log |
| R-DETGATE | a real `fix` mutates the fixed-path tree the deterministic gate relies on, destroying byte-stable `Ir` and corrupting shared `check` cells | the gated `fix` cell must be `--dry-run`; a convergence signal uses a `materialize`-per-iteration criterion bench, not the deterministic gate |
| R-UTF16 | LSP `Position.character` is a UTF-16 code unit; the current code dodges this by replacing whole documents; a `ReplaceRange` needs offset -> UTF-16 conversion | convert from the `bytes: &[u8]` the fixer already receives; add multi-byte / emoji / CRLF unit tests before shipping minimal-diff LSP edits |
| R-EXE | regenerating `help-*.stdout` on Linux strips the trycmd `[EXE]` placeholder, reddening Windows CI | hand-restore `Usage: alint[EXE] <sub>` after every `TRYCMD=overwrite` |
| R-ROXML | bumping `roxmltree` past 0.20 reintroduces a deep-nesting stack overflow the pin guards against | keep the 0.20 pin; a dependabot bump's red CI can mask it, so verify the `xml_deeply_nested` guard test |
| R-DEP | Phase 2d/2e add `jsonc-parser` and `saphyr-parser` as prod deps (supply-chain surface, new-crate publish gate) | run the supply-chain / license gates; a new workspace dep of a published crate cannot be `publish=false`; stage the deps in their own reviewable commits |
| R-TWOOP | the Rust untagged `FixSpec` load does not reject a two-op `fix:` block (it silently picks the first); only the JSON-Schema (editor/LSP) rejects it | do not assume `alint` load rejects two ops; if strictness is wanted, add an explicit guard + test |
| R-PROV | the content-fixer trust demotion needs per-source provenance that does not exist in the loader today | build it at the single classification site `loader.rs:143-158` and thread onto `RuleSpec`/`RuleEntry`; treat as security-load-bearing, with adversarial `extends:` tests |
| R-WALK | the Phase 1 re-walk amends ARCHITECTURE's "walk once per invocation" invariant | update ARCHITECTURE.md text for the `fix` path; keep `check` unchanged; the re-walk is deterministic |
| R-SHARED-WORKTREE | parallel implementation agents in one checkout can contaminate each other's builds | any agent that mutates tracked source runs in an isolated worktree branched from the feature tip (not stale `origin/main`) |

## 12. Sequencing, milestones, and versioning

- **Milestone A (v0.17): Phase 0 + Phase 1.** The tiers, the primitives, the batched engine + active
  fixpoint, `--unsafe-fixes`/`--diff`/`--fix-only`, the trust gate, and the first located op
  (`replace`). Ships the deprecation warning that `file_remove` will become Unsafe.
- **Milestone B: Phase 2 (2a -> 2e).** Each format sub-phase is its own shippable increment; the
  flagship value (HCL/XML/dotenv/INI) lands first with zero new deps. `file_remove` flips Safe ->
  Unsafe about two minors after v0.17.
- **Milestone C: Phase 3 + Phase 4.** Metadata/VCS/cross-file, then the ordering/header wins.
- **Deferred** items stay demand-gated.

Each phase is one PR following the repo's phased-rollout convention (one commit per phase, or a small
series with a forward `Next: Phase N` pointer), keeps `ROADMAP.md` untouched until the phase is
actually scheduled, and lands its downstream-artifact updates in the same PR.

## 13. Definition of done

A phase is done when, and only when:

1. Every applicable rung of section 2 is green in CI (unit, parity, e2e Layer A + B, proptest, Kani
   where a proof was added, perf gate).
2. The section 3 downstream artifacts are updated in the same PR and their gates pass.
3. The phase is a no-op for configs that do not use its new ops (backward compatibility for the
   original 12 ops is proven by the untouched existing scenarios).
4. `alint` dogfoods clean on its own repo.
5. The design doc's false-positive/safety surface (auto-fix.md section 7) has a corresponding test
   for each mitigation the phase touches.
6. The formal contracts the phase relies on (auto-fix.md section 5.8) are encoded as proptest laws or
   debug-asserts, not just prose.

The bar is deliberately high: auto-fix mutates users' files, so "a fix is not done until a gate
asserts its invariant."
