# v0.17 auto-fix: completion plan and priorities

Status: **living plan** as of 2026-09-20. Branch: `phase-0-fix-engine` (the
long-lived v0.17 integration line). This document is the SSOT for what remains to
cut v0.17, written after an arc-wide adversarial audit; it supersedes the
warn-then-flip `file_remove` sections of
[`auto-fix-implementation-plan.md`](../auto-fix-implementation-plan.md) (§4 W5,
§5, §11 R-FILEREMOVE, §12) and [`auto-fix.md`](../auto-fix.md) §9 (see §2 below).

## 1. Where the arc stands

**Complete and independently audited:**

- **Phase 0** (fix-engine foundation): tiers (`Applicability`),
  `CollectedEdit`/`EditVerifier`, the fixpoint driver, `--unsafe-fixes` /
  `--fix-only` / `--diff` / `--dry-run`, report + exit plumbing, the two-op
  guard, the coverage gate, the Kani overlap-skip proof.
- **Phase 1**: the located `replace` op, the active fixpoint, `--changed`
  write-confinement, the LSP `replace` code action, W2 content-fixer trust
  (remote-`extends:` demotion, R-RETRO).
- **Phase 2** (flagship): `set_value` / `remove_value` across all 8 formats
  (HCL, XML, dotenv, INI, TOML, properties, JSON, YAML) + `replace` on the 8
  `*_path_matches` kinds; whole-doc multi-rule coalescing (`minimal_replace`).
- **W3 (SARIF `result.fixes[]`)**: `check --format sarif` advertises the
  concrete fix each fixable finding would apply, derived from the REAL pipeline
  (so it equals what `alint fix` writes).

**15 fix ops ship:** `set_value`, `remove_value`, `replace`, `file_create`,
`file_remove`, `file_rename`, `file_prepend`, `file_append`,
`file_trim_trailing_whitespace`, `file_strip_bom`, `file_normalize_line_endings`,
`file_collapse_blank_lines`, `file_append_final_newline`, `file_strip_bidi`,
`file_strip_zero_width`.

**Arc-wide audit (2026-09-20, 4 independent agents).** The core algorithms held
up under adversarial probing (no silent corruption or uncaught over-deletion was
reproducible across ~150 fixtures; the verify/demote cascade, overlap-skip,
fixpoint, `--changed` confinement, and W2 provenance are sound). Fixed 4
cross-cutting seam bugs (LSP verify/tier bypass; `--diff` silent drop of a
deferred located edit; HCL BOM skip; `--fix-only` json flag) and filled the
highest-value coverage gaps (`replace` × the other 6 formats, located+whole-file
composition, the dead `FixDryRun` corpus step, structured non-convergence exit-2,
W2 `file_prepend`/`file_append` demotion, `fix --baseline` rejection). Test count
2648 green. Findings + fixes are in commits `1092e2fb`, `89098fb2`, `da256d28`,
`0bfe00b3`.

## 2. "Deferred to v0.18" -> v0.17: already resolved

The one auto-fix item the plan deferred to v0.18 was the `file_remove` Safe ->
Unsafe default flip. **It is already in v0.17** (commit `266c88f9`, which lands
the flip "at the v0.17 fix-engine rework, a natural breaking point, rather than
after a separate deprecation-warning release"). `file_remove` is Unsafe today; a
bare `fix` only suggests it, `--unsafe-fixes` applies it; the scenarios
(`file_remove_unsafe_by_default.yml`, `..._unsafe_flag_applies.yml`) assert this.
There is **no deprecation-warning release** and **no v0.18 flip** -- so the
"pull v0.18 into v0.17" request is satisfied.

Docs reconciliation (done): the stale warn-then-flip prose in
`auto-fix-implementation-plan.md` (§4 W5, §5 op-classification, §11 R-FILEREMOVE,
and both §12 bullets -- Phase 0 and the flip paragraph) and `auto-fix.md` §9 has
been corrected to the shipped reality (each spot marked SUPERSEDED or pointing at
the flip commit `266c88f9`), so the design record no longer claims a deprecation
warning or a v0.18 migration.

## 3. Remaining v0.17 work, prioritized

### P0 - Finish Phase 2's tail (W3 remainder + W4)

- **W3b - `agent` / `json` `proposed_edit`.** The plan's W3 (§4, §7 DoD) is
  SARIF `result.fixes[]` AND an `agent`/`json --include-fixes` `proposed_edit`
  field. Only SARIF ships. The infrastructure (`alint_core::proposed_fix` +
  `Violation.proposed_edits`) is format-agnostic, so this is: run
  `attach_proposed_edits` for `agent` and `json`, add the field to
  `AgentViolation` / `JsonViolation`, and test. Small, self-contained. (One small
  gating decision, §6: the DoD phrasing "`agent`/`json --include-fixes` carry
  `proposed_edit`" is ambiguous whether `agent` is always-on; the natural reading
  is `agent` always-on -- mirroring its existing always-on `fix_command` -- and
  `json` behind an `--include-fixes` flag.)
- **W3 tier scope: RESOLVED -> Safe-only (DECISION 2026-09-20).** The SARIF /
  agent / json machine surfaces advertise ONLY Safe (applyable) fixes -- the
  DoD's original "all tiers" is superseded, because SARIF has no machine-honored
  per-fix safety field, so a third-party auto-applier could apply an Unsafe edit
  blind. This is the CURRENT behavior (gated on per-violation `is_fixable`), so
  W3b requires no tier change: `agent`/`json` simply surface the same Safe-only
  proposed edits SARIF already does. alint's OWN `fix` keeps the full tier
  control (Unsafe fixes shown as suggestions, applied only with
  `--unsafe-fixes`). Docs (auto-fix.md 5.7 / §9, plan §7) reconciled.
- **W4 - baseline-aware `fix`.** `fix` still rejects `--baseline` /
  `--strict-baseline` / `--show-baselined` (main.rs). Make it baseline-aware:
  skip suppressed findings, surface them as Suggestions, fix only new ones.
  Scheduled "by Phase 2"; extends ADR-0006. (Sub-task: this INVERTS the
  `baseline-flag-fix-rejected` trycmd added in `0bfe00b3` -- the reject test
  becomes an accept/behaves test.)

### P1 - Phase 3 (metadata, VCS, repo-scale cross-file)

Ops not started (though the core plumbing is pre-laid: the `FixEdit::SetMode`
edit variant is defined + dispatched, and the empty `SPAWNING_FIX_OPS` SSOT +
its emptiness gate already exist). Ops: `chmod` (`SetMode`; `executable_bit`, `shebang_has_executable`,
`executable_has_shebang`); **`git_untrack`** (the FIRST spawning fix op -> W2's
spawning-refusal gate goes live: `SPAWNING_FIX_OPS` + the parity gate + the
`extends:`-refusal canary, R-SPAWNGATE); a user `command`-backed fix;
`sync_from` (Unsafe whole-file copy) + cross-file create-and-register + cross-file
value propagation (multi-file transaction with an injectable-writer test seam);
`dir_create`; lockfile `relocate`. Risk: medium.

### P2 - Phase 4 (ordering, canonicalization, headers)

Not started. Ops: `sort` / `dedup` (on `ordered_block`), `indent_style` (lift the
`indent_style.rs` rejection), a gitignore/gitattributes `insert_line`,
`insert_header` (on `file_header`, comment-style/shebang/xml-decl/.license
aware). Risk: low-medium. The clean deterministic wins; a good final phase.

### Coverage follow-ups (tracked from the audit; regression-prevention)

The core algorithms held and the highest-value gaps are filled; these remain
(not blocking, but wanted for the "comprehensive suite" bar):

- `fix --base <ref>` (only working-tree `--changed` is tested).
- CRLF e2e goldens for the structured ops (units cover HCL/dotenv/TOML/properties
  only; no structured CRLF e2e).
- LSP tier/W2 gate test: a session `extends:`-ing an untrusted remote asserts the
  demoted whole-file content fixer is not offered as a quick-fix (the located
  demote is now gated by `code_action_withholds_a_fix_the_pipeline_would_demote`;
  the whole-file W2 case needs a remote-`extends:` LSP fixture).
- `--diff` of a located edit in isolation; `located_deferred` poisoning;
  `located_status` per-`LocatedOutcome`-arm unit test.
- Structured `replace` in the proptest catalogue (GetPut/PutGet); a unit-level
  localized-equivalence proptest for the resolvers; a `minimal_replace`
  reconstruction proptest.
- CLI fix-report snapshots (`--diff`, `--fix-only`, json/markdown fix reports).
- json rule-level vs SARIF/agent per-violation fixability consistency doc-test.
- Gate hardening: make `ALL_OP_NAMES` type-derived (e.g. `strum::EnumCount`) so a
  new `FixSpec` variant can't slip past the fix-coverage gate.

### Engineering follow-ups

- **Full multi-pass `--diff` fidelity** (currently a deferred located edit warns
  on stderr; a faithful preview would re-collect against the composed bytes or
  run the stage as a fixpoint).
- **LSP UX for Unsafe fixes** (the plan intends offering Suggestions to a human;
  the audit flags one-click Unsafe delete + presenting Unsafe/demoted fixes with
  no tier signal -- decide whether to label them distinctly).
- **TOML array-of-tables fixability** (`[[x]]` inner values are unfixable today:
  `toml_::navigate_mut` handles only `Key`).
- **`structured_fix/mod.rs` `formats/` split** (~1820 lines, nearing the 2000
  cap).

## 4. Deferred-and-special items (§10): keep deferred

Each needs explicit opt-in + its own design record; none is v0.18-bound, and
several have no Safe form, so pulling them into v0.17 wholesale would balloon
scope without a clear win. Recommend they STAY out of v0.17 unless a specific one
is prioritized:

- **Reference/version pinning** + **regenerate-from-command**: reach outside the
  tree (top-level-only, spawn-gated, or a future WASM plugin).
- **JSON-Schema-guided fill** (`json_schema_passes`): synthesizing a doc to
  satisfy a schema is ambiguous -> Suggestion at most.
- **Whole-file reprint**, **`line_max_width` reflow**, **encoding transcode**:
  out of scope / Suggestion at most.
- **`unique_by` collision auto-resolution**: no Safe fix (ambiguous survivor).

## 5. Recommended order to cut v0.17

1. **W3b + W4** (P0): small, completes Phase 2's DoD; land with the "all-tiers"
   decision.
2. **Docs reconciliation** (§2): correct the file_remove warn/flip prose.
3. **Phase 3**, then **Phase 4** (each its own PR to the integration line).
4. **Coverage + engineering follow-ups**: fold the tracked items in alongside the
   phase that touches their code (e.g. the LSP tier test with any LSP work; the
   `--diff` fidelity with Phase 3's write-path rework).
5. **DoD sweep + release**: all blocking rungs green; downstream artifacts
   updated (docs-export / rule pages / `facts.json` / the gen-X `--check`
   artifacts); CHANGELOG `[Unreleased]` -> `[0.17.0]` finalized (the `file_remove`
   breaking entry already sits there); version bump; `ROADMAP.md`/`roadmap.json`
   v0.17 entry finalized; tag v0.17. **Then** the release is not live until
   alint.org's install-pins + prose claims are bumped (the STALE DOCS BUNDLE
   guard blocks deploy otherwise) -- see the general `RELEASING.md` process and
   the `alint.org` pin-bump step.

## 6. Decisions

**Resolved:**
- **SARIF/machine tier scope -> Safe-only** (2026-09-20). Machine surfaces
  advertise only Safe fixes; alint's own `fix` keeps full tier control. See §3 P0.

**Still open:**
- **LSP Unsafe UX** (§3 engineering): keep offering Unsafe fixes as plain
  quick-fixes (current) vs label/gate them.
- **§10 pull-in**: keep all deferred (recommended) vs pull a specific item into
  v0.17.
- **W3b `agent` gating** (§3 P0 W3b): `agent` carries `proposed_edit` always
  (recommended, mirrors its always-on `fix_command`) vs behind `--include-fixes`
  like `json`. Minor.
