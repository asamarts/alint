# v0.17 auto-fix: completion plan and priorities

Status: **living plan** as of 2026-09-21. Branch: `phase-0-fix-engine` (the
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
- **Phase 3 `chmod` (first metadata op)**: `fix: { chmod: {} }` on
  `executable_bit` / `shebang_has_executable` sets or clears the Unix `0o111`
  bits, preserving every other bit. Safe by default; Unix-only; `fix --diff`
  renders a git-style `old mode`/`new mode` pair. Drawn by the property net
  (single_fixable strategy + a planted shebang trigger) so convergence /
  idempotence / dry-run purity are asserted across 3000 cases.
- **Phase 3 `git_untrack` (first SPAWNING op -> the W2 spawn gate is LIVE)**:
  `fix: { git_untrack: {} }` on `file_absent` runs `git rm --cached` to drop a
  committed artifact from the index while keeping it on disk (converges with
  `git_tracked_only: true`). Unsafe by default; refused from any non-top-level
  source. The empty `SPAWNING_FIX_OPS` SSOT is now `["git_untrack"]`, wired to
  `reject_spawning_fix_ops_in` (+ template / finalize / nested backstops) and both
  R-SPAWNGATE tests (the parity gate + the RCE canary). An index-only op has no
  worktree-diff form, so no `FixEdit` variant was needed.

**17 fix ops ship:** `set_value`, `remove_value`, `replace`, `file_create`,
`file_remove`, `file_rename`, `file_prepend`, `file_append`,
`file_trim_trailing_whitespace`, `file_strip_bom`, `file_normalize_line_endings`,
`file_collapse_blank_lines`, `file_append_final_newline`, `file_strip_bidi`,
`file_strip_zero_width`, `chmod`, `git_untrack`.

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

- **W3b - `agent` / `json` `proposed_edit`. DONE + AUDIT-HARDENED.** `agent`
  (always) and `json` (behind the new `--include-fixes` flag) now carry a
  `proposed_edit` array (`{path, region, inserted}`) -- the same Safe-only edits
  SARIF advertises, reusing the format-agnostic `attach_proposed_edits`. Gate:
  `agent_and_json_carry_proposed_edit_per_the_include_fixes_flag`.
  - **Independent P0 fidelity audit (2 agents, 2026-09-20) -> 7 findings fixed.**
    THE FIDELITY INVARIANT (machine surfaces == what `alint fix` writes) had
    diverged because `attach_proposed_edits` derived edits PER RULE while `fix`
    composes PER FILE. Fixed: (CRITICAL) two located rules whose spans overlap on
    one file now batch into one overlap-deconflicting pass, so no overlapping edit
    `fix` skips is advertised; (HIGH) whole-file normalizers are threaded per file
    in config order (each sees the previous one's output), so no spurious
    `final_newline`-after-trim edit; (HIGH) `cmd_check` now wires
    `fix_size_limit`; (MEDIUM) the per-node `replace` verify round-trips keys with
    `'`/`\`/controls (was silently demoting safe fixes); (MEDIUM) a `no_bom`
    empty-region no-op is omitted; (LOW) F3 baseline-fingerprint change noted in
    CHANGELOG; (LOW) a non-UTF-8 byte-fix stays unadvertised (documented). Two A1
    interactions caught by grounding were fixed first: the located `replace`
    fidelity + the LSP keyless fix-all fallback. Gates in `sarif_fixes.rs` +
    `structured.rs`. Per-edit granularity preserved.
- **W3 tier scope: RESOLVED -> Safe-only (DECISION 2026-09-20).** The SARIF /
  agent / json machine surfaces advertise ONLY Safe (applyable) fixes -- the
  DoD's original "all tiers" is superseded, because SARIF has no machine-honored
  per-fix safety field, so a third-party auto-applier could apply an Unsafe edit
  blind. This is the CURRENT behavior (gated on per-violation `is_fixable`), so
  W3b requires no tier change: `agent`/`json` simply surface the same Safe-only
  proposed edits SARIF already does. alint's OWN `fix` keeps the full tier
  control (Unsafe fixes shown as suggestions, applied only with
  `--unsafe-fixes`). Docs (auto-fix.md 5.7 / §9, plan §7) reconciled.
- **W4 - baseline-aware `fix`. DONE + AUDIT-HARDENED (`--baseline`).** `fix
  --baseline` (and a config `baseline:` key) SKIPS the grandfathered findings and
  resolves only NEW ones -- classified by reusing `baseline::apply` per rule (the
  fingerprint includes the rule id, so per-rule == report-level), per fixpoint
  pass, on the current content (identical to `check --baseline`). A grandfathered
  finding is a benign `baselined` skip (`SkipKind::Baselined`, excluded from
  `has_unresolved`), so it does not fail the exit -- a converged `fix --baseline`
  with only accepted debt left exits 0. Gates: `fix_baseline.rs` (10 tests) +
  `report` units.
  **Follow-up:** `--strict-baseline` (stale-fail across a fixpoint) and
  `--show-baselined` (suppressed visibility) for `fix` are still `check`-only
  (loudly rejected, not silent). Extends ADR-0006.
  - **Independent adversarial audit (2 agents, 2026-09-20) -> 5 findings fixed:**
    (1) CRITICAL: the located `StructuredFixer` ignored its `violations` arg and
    re-derived every failing node, so `fix --baseline` REWROTE grandfathered
    `*_path_matches` nodes -- fixed with count-aware correlation on the shared
    `matches_baseline_key` + per-node (not whole-query) verify. (2) HIGH / (3)
    MEDIUM: the skip exit-code class was sniffed from a forgeable reason PREFIX
    (`baselined:` / `fix error:`), so a filename could flip the exit code -- now
    a structural `SkipKind`. (4) MEDIUM: three whole-file-fixer rules
    (no_zero_width_chars, no_bidi_controls, max_consecutive_blank_lines) lacked a
    path `baseline_key` and stripped grandfathered occurrences -- now keyed on the
    path (the file is the unit of accepted debt). (5) LOW: a debug tripwire now
    asserts the `violation_key` no-collision invariant the fixpoint merge relies
    on. All five have regression gates; the baseline classifier and per-rule ==
    report-level equivalence audited CLEAN.

### P1 - Phase 3 (metadata, VCS, repo-scale cross-file)

- **`chmod`. DONE (commit `9643891e`).** `SetMode` on `executable_bit`
  (direction from `require:`) and `shebang_has_executable` (always +x);
  `executable_has_shebang` stays fix-less (ambiguous target). The `ChmodFixer`
  does the `set_permissions` itself and records `StagedKind::Chmod` for the
  git-style `--diff`; dry-run / `--diff` short-circuit before touching disk.
  Gates: property net (single_fixable + planted `_trig/needsx.sh`), 2 e2e
  scenarios (+x and -x), unit tests (set/clear/idempotent, dry-run, stage,
  fix_edit), fix_spec cases + `w2_content_injecting_ssot` (`chmod` in
  FIXED_BEHAVIOR_FIX_OPS) + facts.json (auto_fix_ops 16). No ruleset bytes, so
  no W2 trust surface.

- **`git_untrack`. DONE (commit `f0688643`).** The FIRST spawning fix op on
  `file_absent`: `git rm --cached` via a new `alint_core::git::untrack_path`
  (idempotent, `--` option-injection guard, non-git-repo skip). Unsafe by
  default. **The W2 spawn gate is now LIVE**: `SPAWNING_FIX_OPS = ["git_untrack"]`
  + `reject_spawning_fix_ops_in` (rules at every `require:` depth) +
  `reject_spawning_fix_op_templates_in` + a `finalize` backstop, wired in
  `loader.rs` (extends) and `nested.rs`. R-SPAWNGATE landed as TWO tests in two
  files: the parity gate `coverage_audit_fix_spawn_gate.rs` (the fixer that
  spawns == `SPAWNING_FIX_OPS`; the rule gate now skips `fixers/`) and the RCE
  canary `crates/alint/tests/fix_spawn_gate.rs` (drives the real binary in `fix
  --unsafe-fixes`; every smuggled vector refused, the tracked file stays tracked;
  a positive control proves a trusted top-level git_untrack untracks). The W2
  partition gate is now three-way (content / spawning / fixed). An index-only op
  has no worktree-diff form, so no `FixEdit` variant was added.
  - **AUDIT-HARDENED (commit `c12c0b64`; 3 independent agents -- the spawn gate got
    a clean bill on 25+ bypass vectors). Fixed:** (CRITICAL) `git rm --cached --
    <path>` glob-expands the path as a git PATHSPEC (`--` blocks options, NOT
    globbing; `*` crosses `/`), so a `[id].tsx`/`*`-named file collaterally
    untracked siblings / emptied the index -- fixed with `GIT_LITERAL_PATHSPECS=1`
    (a trap for EVERY future git-shelling fixer). (MED) dry-run/`--diff` now routes
    through git's own `--dry-run` so it can't diverge from the real run on a
    staged-differs path. (MED) chmod `--diff` rendered an invalid git mode for a
    suid file + downgraded a binary chmod to a summary -- fixed with a
    pre-content-gate render + canonical `100644`/`100755`. (MED) an editless Unsafe
    suggestion (git_untrack) was a misleading skip, not a `requires --unsafe-fixes`
    suggestion -- `FixStatus::Suggested.edit` is now `Option`. (LOW) the finalize
    template backstop now recurses `require:` like the per-source gate.
  - **Fast-follow:** the optional `.gitignore` append (`gitignore: bool`, default
    true per the op table) is deferred -- untrack alone converges; the append adds
    content-mutation + `--diff`-hunk work worth its own increment.

Ops remaining. A user `command`-backed fix (the SECOND spawning op; lifts the
`command.rs` fix rejection, reuses the now-live `SPAWNING_FIX_OPS` gate; exempt
from the rung-8 convergence requirement); `sync_from` (Unsafe whole-file copy) +
cross-file create-and-register + cross-file value propagation (multi-file
transaction with an injectable-writer test seam); `dir_create` (Safe); lockfile
`relocate`. Risk: medium.

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
- W4 baseline-fix interactions the audit verified CLEAN but left un-gated (heavier
  fixtures, no behavior gap): non-convergence WITH a baseline present -> exit 2
  (baselined skips must not fake convergence); `--changed` + `--baseline`
  precedence (a grandfathered-and-out-of-scope finding surfaces as the benign
  baselined skip, not an out-of-scope Suggestion); a baselined skip surviving a
  multi-pass cascade exactly once (not dropped, not duplicated).

### Engineering follow-ups

- **Full multi-pass `--diff` fidelity** (currently a deferred located edit warns
  on stderr; a faithful preview would re-collect against the composed bytes or
  run the stage as a fixpoint).
- **LSP UX for Unsafe fixes: RESOLVED (option b, DECISION 2026-09-20).** Unsafe
  fixes are still offered as quick-fixes (human-in-the-loop) but their title is
  labeled `(unsafe)` and they are never marked preferred, so the click is a
  visible, deliberate opt-in (the LSP analogue of `--unsafe-fixes`) rather than a
  silent one-click apply of a behavior-changing edit. Safe fixes are unchanged.
  Suggestion / W2-demoted fixes remain excluded (audit HIGH-1/HIGH-2).
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
- **LSP Unsafe UX -> option b** (2026-09-20). Unsafe fixes offered but labeled
  `(unsafe)` + never auto-preferred. Implemented; see §3 engineering.
- **§10 pull-in -> keep all deferred** (2026-09-20). No §10 item is a clean Safe
  win (out-of-scope / ambiguous / Suggestion-only / a security surface); each
  stays demand-gated with its own future design record. See §4.
- **W3b `agent` gating -> `agent` always-on** (2026-09-20). `agent` carries
  `proposed_edit` always (mirroring its always-on `fix_command`); `json` carries
  it behind `--include-fixes`. Applies when W3b is built.

_All open decisions are resolved; the plan is ready to execute (start with P0)._
