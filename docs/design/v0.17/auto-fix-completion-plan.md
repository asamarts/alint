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

- **Phase 3 `command` fix (second SPAWNING op)**: `fix: { command: { run: [...] }
  }` on the `command` rule runs a user-supplied fix command (e.g. `eslint --fix
  {path}`), reusing the spawn gate. Unsafe by default, Safe-promotable in the
  user's own top-level config. Convergence-EXEMPT (an arbitrary command's
  idempotence is the author's business), so excluded from the property net +
  `CONVERGENCE_EXEMPT`, with its own fire/silent tests.

- **Phase 3 `dir_create` (Safe)**: `fix: { dir_create: {} }` on `dir_exists`
  creates the required literal directory when missing (glob/multi/`..` rejected at
  load; the host's violation is path-less, so the fixer carries the target). Safe,
  fixed-behavior, converges + idempotent (joins the property net).

**21 fix ops ship:** `set_value`, `remove_value`, `replace`, `file_create`,
`file_remove`, `file_rename`, `file_prepend`, `file_append`,
`file_trim_trailing_whitespace`, `file_strip_bom`, `file_normalize_line_endings`,
`file_collapse_blank_lines`, `file_append_final_newline`, `file_strip_bidi`,
`file_strip_zero_width`, `chmod`, `git_untrack`, `command`, `dir_create`,
`sync_from` (cross-file mirror + value propagation), `relocate`.

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

- **`command` fix. DONE (commit `e315b72d`).** The SECOND spawning op: `fix: {
  command: { run: [...] } }` on the `command` rule lifts its fix rejection and
  runs a user-supplied fix command (`CommandFixFixer` reusing `spawn::run_capturing`;
  exit 0 -> Applied, non-zero/spawn-error/timeout -> fix error; dry-run/`--diff`
  report "would run" and spawn nothing; no `fix_edit`). **Unsafe by default,
  Safe-promotable** in the user's own top-level config (asamarts's call -- uniform
  with file_remove/git_untrack, reusing `reject_fix_promotion_in`, no new gate).
  Second entry in `SPAWNING_FIX_OPS` (belt-and-suspenders atop the top-level-only
  `command` rule kind). Convergence-EXEMPT (`CONVERGENCE_EXEMPT = ["command"]`,
  excluded from the property net), with its own fire/silent tests + the e2e fire
  scenario + the parity gate (`command` -> `command_ops.rs`).
  - **AUDIT-HARDENED (commit `777c84ba`; 3 agents -- injection + trust gate got a
    clean bill). Fixed:** (behavior) a non-converging command-fix re-ran the
    command to the 10-pass fixpoint cap -- now the fixer reports `Applied` only when
    it CHANGES `{path}` (Skip = the idempotence signal), so it runs <=2x; (MED) a
    failing command's output was surfaced uncapped (64 MiB) -- now truncated to 16
    KiB. Filled coverage: non-command-kind extends refusal (H1), Safe-promoted +
    default-suggested tiers (H2), the silent half (M1), the TimedOut arm (M2), the
    editless `--dry-run` preview (M3), chmod default tier (L3). Docs: timeout
    default 30, the templated program token, the auto-fix.md chmod tier.

- **`dir_create`. DONE (commit `3aed5abf`) + AUDIT-HARDENED (3 independent
  agents + own probe -- path confinement was the CRITICAL miss).** Safe fix on
  `dir_exists`: `DirCreateFixer` (in `fixers/file_ops.rs`) creates the required
  literal directory. `dir_exists` fires a PATH-LESS violation, so the fixer
  carries the target; `build()` requires `paths` to be one literal dir (glob /
  multiple / `..` rejected at load). No-op skip when the dir already exists; no
  `fix_edit` (an empty dir has no worktree-diff form). Converges + idempotent ->
  in the property net; fixed-behavior in the partition.
  - **C1 (CRITICAL, own probe -> agents escalated): out-of-root write.** An
    absolute `paths: "/tmp/x"` or a symlinked-parent `paths: "link/sub"` made
    `ctx.root.join(dir)` mkdir OUTSIDE the repo. Since `dir_create` is
    fixed-behavior it is honored from an untrusted `extends:`, so a remote
    ruleset could mkdir anywhere on a bare `alint fix`. FIX: route through the
    shared `creators::confine_fix_path` (lexical + filesystem symlink-parent
    check); out-of-root now needs a top-level `allow_out_of_root`.
  - **H1: `git_tracked_only` never converges** (git cannot track an empty dir)
    and **H2: `root_only` + a nested path never converges.** Both now
    **rejected at `build()`** with an actionable message (H1 points at a
    `.gitkeep` via `file_create`).
  - **M1: broken-symlink errno leak** -> explicit symlink guard returns a clean
    `Skipped` ("exists but is not a directory (a symlink)"), so the fixer truly
    refuses to clobber a non-directory. **M2: no stage-mode test** -> added.
  - Gates: 13 fixer units (was 4; +7 for confinement / symlink / stage / broken
    symlink) + build-reject tests + e2e + generator.

- **`sync_from`. DONE.** The first CROSS-FILE content fix op, on
  `cross_file` `relation: identical`: overwrite a drifted target with the
  canonical `source:` so the rule converges (the workspace LICENSE-mirroring
  case). `SyncFromFixer` (in `fixers/cross_file_ops.rs`) is a content fixer -- it
  routes a whole-file write through the compose buffer (`SetContent` for the
  editor / SARIF / `--diff` surfaces), so it needs no `apply`-direct-write path.
  **`Unsafe` by default** (a whole-file overwrite can discard uncommitted target
  content), and **content-injecting** in the W2 partition (the ruleset's `source:`
  chooses which file overwrites which, so an untrusted remote demotes it to a
  suggestion). `cross_file` grew a `fixer: Option<SyncFromFixer>` + `fn fixer`;
  `build()` accepts `sync_from` ONLY on `relation: identical` with
  `skip_header_lines: 0` and a single-file source (a value / set / resolves
  relation, or a preserved header, is rejected at load -- a whole-file copy has no
  single "correct bytes" there / would clobber the kept header). Both endpoints
  are confined via the shared `confine_fix_path` (an absolute / symlinked-parent
  `source:` or target can't read or write out of tree -- built in from the start,
  not a post-audit fix). Converges + idempotent (target := source -> identical ->
  no violation; a second pass finds them equal -> Skip), so it is IN the property
  net (`rule_sync_from` + a planted `_trig/sync_src.txt` / `_trig/sync_dst.txt`
  pair, applied under `--unsafe-fixes`). Gates: 13 fixer units (mirror / binary /
  dry-run / stage-compose / confinement x2 / missing-source / self-copy /
  allow_out_of_root / fix_edit x2 / tier) + 4 build tests + 2 e2e (applied +
  suggested) + facts.json (auto_fix_ops 20). **The whole-file overwrite is the
  deliberate scope: cross-file value propagation (`equals`) is a SEPARATE, harder
  increment (a located value patch with the injectable-writer seam), and a
  header-preserving sync is a deferred follow-up.**
  - **AUDIT-HARDENED (3 independent agents + own 11-probe pass). 1 HIGH + 1 LOW
    fixed:** (HIGH) `read_for_fix` (alint-core) did a bare `std::fs::read` with NO
    non-regular-file guard, so a FIFO named as a `sync_from` `targets:` LIST entry
    (a config-verbatim path that skips the walker's special-file filter) HUNG `fix`
    forever -- including the `--dry-run` / `--diff` previews. `check` was safe
    (`read_capped` refuses non-regular), so the fixer diverged. FIX: `read_for_fix`
    now refuses a non-regular file (a `metadata().is_file()` guard, mirroring
    `read_capped`/`open_regular`) -> a clean Skip; this hardens EVERY fixer, not
    just sync_from. (LOW) no `w2_remote_sync_from_is_demoted_to_suggestion` test
    existed (every sibling content op has one) and `declared_content_tier` lacked a
    `SyncFrom` arm -- the demotion WORKED but nothing locked it in; both added.
    Agents confirmed the rest SOUND: confinement (absolute + symlinked-parent),
    convergence (incl. mutually-referential swap/cycle rules), tier honesty,
    machine-surface Safe-only gating, and the untrusted-remote demotion firing.

- **`cross_file.rs` split into a `cross_file/` module (spec / eval / mod).** The
  file had passed 2000 lines (the dogfooded `rust-file-max-lines` limit); split by
  responsibility with co-located tests, all three files well under. Faithful
  refactor (a third agent verified 39==39 tests preserved + byte-identical
  production code); `xtask` `rule_source_files` now resolves a `<stem>/mod.rs`
  directory module so alint.org's source links stay valid.

- **`sync_from` on `equals` (value propagation). DONE (Phase 1).** Extends
  `sync_from` to the value relation: propagate the source's single extracted
  scalar into each drifting target's node, per format. `CrossFileValueFixer` (in
  `fixers/cross_file_ops.rs`) is a WHOLE-FILE `apply` fixer that reuses the located
  resolver INTERNALLY -- it builds a `StructuredFixer::set` for the target node,
  takes its located edit, and applies + verifies it via `located_fix::
  apply_file_edits`, then `commit_write`s the whole result. **Design finding
  (recorded in `cross-file-value-propagation.md`): a located `collect_edits` fixer
  must NOT host on `cross_file` -- the engine's located branch assumes per-file
  hosts and would silently escape the `--changed` blast radius on a
  `requires_full_index` rule (engine.rs:1633-1656). The whole-file `apply` approach
  routes through the existing blast-radius-demoted path, so NO engine change.**
  `cross_file`'s `fixer` field widened to `Option<Box<dyn Fixer>>` (identical ->
  `SyncFromFixer`, equals -> the value fixer). `build()` accepts equals + all
  STRUCTURED target extracts (all 8 formats via the `StructuredFixer` reuse);
  regex-extract targets are the Phase-2 follow-up (rejected at load); a set /
  resolves relation is rejected. Unsafe + content-injecting (W2 covers `sync_from`
  already). Gates: 6 fixer units (propagate / idempotent / not-one-value / dry-run
  / confine / tier) + 3 build tests (accepts equals+structured, rejects
  regex-target, rejects set) + an e2e (`sync_from_equals_propagates_a_value`,
  applied under `--unsafe-fixes` + convergent). No facts change (still `sync_from`,
  op count 20). Design doc: `docs/design/v0.17/cross-file-value-propagation.md`.
  - **AUDIT-HARDENED (2 independent agents + own ~12-probe pass). 1 MED + 1 LOW +
    an LSP gap, all fixed; the load-bearing `--changed` claim VERIFIED live.**
    (MED, F1) the fixer always writes a `Value::String` (extraction yields text),
    so a NUMERIC/BOOL target node was silently coerced to a quoted string and
    `check` went false-green -- FIX: a type-preservation guard declines a
    non-string scalar node (the `equals` check compares only string leaves, so a
    coercion is the only thing that "converges", masking the mismatch; a typed pin
    belongs in same-file `set_value`). (LOW, F2) `--changed` over-demoted a
    `./`-prefixed LIST target that IS the changed file (config-verbatim path vs
    git-canonical diff spelling) -- FIX: normalize the list path at the single
    resolution point (`resolve_targets`), so the violation path AND the fixer's
    stored target path both match the diff. (LSP, 4c) the value fixer had no
    `fix_edit`, so the LSP could never offer it even Safe-promoted (SARIF/agent DO
    advertise it via the compose pass) -- FIX: implemented `fix_edit` (a
    `SetContent` reusing `propagated_bytes`); changes only the LSP. Added: the W2
    demotion test for the EQUALS form, the value fixer's `--changed`-demote CLI
    test (the "no engine change" claim's gate), type-coercion + non-scalar-skip +
    fix_edit units, a JSON e2e, and two stale-comment fixes. Agents confirmed SOUND:
    all 8 formats, PutGet verify (special chars / control chars demote), source +
    target confinement (incl. a real FIFO), compose + loud non-convergence, tier
    honesty across human/sarif/agent/json, and the partition gate.
  - **Phase 2 DONE: regex-extract targets.** `ValueTargets` now carries the whole
    `Extract` (structured or regex); `propagated_bytes` dispatches to
    `propagate_structured` (Phase 1) or `propagate_regex` (new). The regex path
    rewrites EACH match's capture group 1 to the source value via a `ReplaceRange`
    batch through `apply_file_edits`, then RE-EXTRACTS to verify (a value that
    breaks the surrounding pattern -- e.g. a `"` inside a `"([^"]+)"` capture, or a
    digit into a `[a-z]+` capture -- fails and declines, never writing a value the
    check would still reject); requires valid UTF-8 (byte-offset-safe). `build()`
    accepts a structured OR regex target extract; a `lines`/`whole_file` target is
    rejected (no single value). Gates: 4 regex fixer units + a build-accept +
    a `lines`-reject build test + a regex e2e (`sync_from_equals_regex`, a README
    badge + Dockerfile `ARG`). The value-propagation feature is now complete.
  - **AUDIT-HARDENED (2 independent agents + own probing). 1 CRITICAL + 2 HIGH,
    all fixed -- the value fixer was re-deriving edits from raw bytes without
    correlating to the check (the located-fixer-correlation lesson, violated).**
    (CRITICAL) making `cross_file: equals` fixable exposed that it emits KEYLESS
    multi-findings per path (a non-literal `${...}` NOTE + a keyless "no literal
    value" violation) that collide on `violation_key` -- `alint fix` PANICKED in
    debug (a plain `image: myapp:${VERSION}` config) / silently dropped one in
    release. FIX: the notes now carry a `reason`-discriminated `baseline_key`
    (`eval.rs`), so every finding on a path is uniquely keyed. (HIGH) the fixer
    CLOBBERED non-literal `${...}` captures the check deliberately skips, and
    (HIGH/MED) it IGNORED `normalize:`, over-writing a capture the check deemed
    correct (`2.5.7`->`2.5.0` under `semver-minor`). FIX: an `is_drift` guard --
    the fixer rewrites a capture/node ONLY if the check flagged it (LITERAL and
    normalize-different from the source); the fixer now carries the rule's
    `normalize`, and the regex re-extract verify is normalize + literal aware.
    Also F-1 (a "matched nothing" regex now says so, not "no capture group 1").
    Gates: non-literal-skip (regex + structured), normalize-correlation,
    multi-capture, no-match, non-UTF-8, mixed-list build, and a
    `sync_from_equals_skips_a_template` e2e (the debug panic gate). Agents
    confirmed the re-extract verify SOUND (verify-pass implies check-pass) + byte
    offsets, confinement, --changed, tiers all hold.

- **`relocate` (lockfile relocate). DONE.** A new fix op (`FixSpec::Relocate`,
  #21) hosted on `file_absent`: when the rule flags a file in a SUBDIRECTORY, move
  it back to the repository root keeping its basename -- the "a lockfile drifted
  into a member directory" case (`paths: "**/*/Cargo.lock"` + `fix: { relocate:
  {} }`). Design decisions: (a) HOST = reuse `file_absent` (the violation carries
  the subdir path), joining `file_remove` / `git_untrack` in its `match &spec.fix`
  -- no new rule kind, the smaller and self-contained option; a dedicated
  `file_at_root` kind was considered and left as a future option. (b) MECHANISM =
  `RelocateFixer` modeled on `FileRenameFixer` (a shared PURE `resolve_relocate_
  target` feeds `apply` + `fix_edit` + `can_fix`; emits `FixEdit::RenameFile` to
  root; honors dry-run + `--diff` stage via the `stage_ops` sink). (c) TIER =
  Unsafe by default (a rename moves a real file, destination inferred),
  Safe-promotable; FIXED-BEHAVIOR in W2 (no ruleset bytes, no spawn -- honored
  from any source, gated only by its tier). (d) UNAMBIGUOUS-ONLY: an already-at-
  root file (nowhere to move, `X->X` never converges) is `can_fix`-false and
  Skipped; an occupied root slot is a fix-time collision Skip (never clobbered,
  and the same-file case that `FileRenameFixer` needs cannot arise -- source is in
  a subdir, target at root); a staged-collision (two nested files, one root slot)
  skips the second so `--diff` never emits a self-conflicting patch. Convergence
  requires a SUBDIRECTORY-anchored pattern (`**/*/Cargo.lock`, not `**/Cargo.lock`);
  the docs + the fixspec doccomment say so. Gate cascade: `FixSpec::Relocate` +
  `RelocateFixSpec` + `op_name` + `ALL_OP_NAMES` + the `cases` op-name test
  (config.rs); `file_absent` build arm + `build_accepts_relocate_fix`; W2
  fixed-behavior partition (`FIXED_BEHAVIOR_FIX_OPS`) + `w2_remote_relocate_is_not_
  demoted` (with an explicit `declared_content_tier` arm for teeth); property net
  (`rule_relocate` + a planted nested `_trig/reloctrig.txt`, drawn into the
  convergence / idempotence / dry-run-purity laws); fix-coverage e2e (4 scenarios:
  moves-a-nested-lockfile [convergent], suggested-under-bare-fix, skips-when-root-
  slot-taken, declines-a-root-level-file); 9 `RelocateFixer` unit tests; facts.json
  20->21; README count (26 + 60) + prose; CHANGELOG. No standalone design doc (a
  focused op like `dir_create`); the decisions live here.
  - **AUDIT-HARDENED (2 independent worktree-isolated agents, both base-verified at
    the tip + own CLI probing).** Both verdicts: **sound and safe to ship** -- no
    data-loss, no clobber of a real committed file, dry-run/stage disk-pure,
    converges + idempotent; and the gate cascade is sound with every claimed
    invariant asserted with teeth (the W2 classification -- the highest-risk area --
    is correct and genuinely enforced: an untrusted remote cannot promote relocate
    to Safe, clobber, retarget onto an arbitrary name (dest is always root + the
    ORIGINAL basename), or path-traverse). All findings were LOW except two MED
    preview/scope edges, all either FIXED, by-design, or shared-with-`FileRenameFixer`.
    FIXED this round: (1) collision check now uses `symlink_metadata` not `exists()`,
    so a DANGLING symlink at the root slot is never clobbered (Agent A #4); (2) a
    DESTINATION `has_pending_write` guard, so a self-contradictory `file_create`
    root/X + relocate nested/X->root/X config yields instead of racing the flush
    (Agent B F4) -- both stricter than `FileRenameFixer`, with a backport follow-up;
    (3) the docs-manifest `count_canonical_auto_fix_ops` now keys off
    `FixSpec::ALL_OP_NAMES` (was a `pub struct *Fixer` text-count equal only by
    coincidence -- a future struct-reuse would have drifted the published count from
    facts/README; Agent B F1); (4) `dir_exists` `build_rejects_an_incompatible_fix_op`
    now covers `relocate` (proving its only valid host is `file_absent`; Agent B F3);
    (5) the README line-60 "N ops covering" prose count is now gated (Agent B F2).
    By-design / accepted (documented, NOT bugs): Agent A #2 (`--changed
    --unsafe-fixes` creates the root `to` target -- a rename's target is intrinsically
    new; the engine correctly keys the blast-radius demote on the in-scope
    violation/`from` path, and an UNTOUCHED nested file IS demoted to a suggestion;
    demoting on the `to` would make every rename un-fixable under `--changed` and
    regress `file_rename`); Agent A #3 (the fixer does no self-confinement -- the
    target is confined by construction via `file_name`, and the source comes from
    the walker's root-stripped, symlink-pruned index, so `..`-escape is unreachable
    through the only host, matching `file_remove`/`file_rename`); Agent A #5 /
    Agent B F5 (`can_fix`-true + `apply`-Skipped for a taken slot is intentional and
    mirrors `FileRenameFixer`; `fix_is_idempotent` is vacuous for every Unsafe op
    because it runs at the Safe threshold -- `fix_unsafe_converges` is the real
    idempotence teeth).

- **`create_and_register` (cross-file create-and-register). IN PROGRESS -- the LAST
  Phase-3 op.** Design doc: `docs/design/v0.17/cross-file-create-and-register.md`
  (asamarts approved the shape 2026-09-24). A new `cross_file` `relation: registered`
  + `fix: { create_and_register: {} }`: a workspace member must both EXIST and be
  REGISTERED in a manifest list (`$.workspace.members`). **KEY MODEL (asamarts):
  two idempotent postconditions -- `exists` (repair = create from `content_from:`)
  and `registered` (repair = append to the list) -- each applied ONLY if unmet;
  "create only if it doesn't exist" is not a mode flag, just the existence repair,
  a no-op whenever the member already exists (always so for a glob-discovered one).**
  So the common case (existing-but-unregistered) degrades to a single-file list
  append; the 5.2.4 multi-file transaction engages only when a create actually
  fires (a missing NAMED source). Unsafe + content-injecting.
  - **Phase 1a -- the `registered` CHECK. DONE (commit `6cbdd63b`).** The relation
    + `check_registered` (filesystem-path source -> per-target subset check) +
    `register_as` templating + the source-glob / shape validations + schema regen
    (worked through a stale-base-`Relation` `$defs` gen-schema collision). 3 e2e +
    4 build tests.
  - **Phase 1b -- the register-only FIX. DONE (this commit).** `create_and_register`
    (`FixSpec::CreateAndRegister`, #22): a `CreateAndRegisterFixer` (whole-file
    apply, content-injecting, Unsafe-default) appends each missing member the check
    recorded IN ITS `baseline_key` (the check->fixer channel, so the fixer never
    re-globs and can't diverge from the check's gitignore-aware member set) to the
    target's array via a new `structured_fix::document_append` (TOML via `toml_edit`,
    format-preserving + idempotent; JSON/YAML decline as a fast-follow). The target
    `extract` selects elements with a trailing `[*]`; the fixer strips it to the
    array path. Full gate cascade (FixSpec + op_name + ALL_OP_NAMES + cases; W2
    content-injecting + `w2_remote_create_and_register_is_demoted`; property net
    `rule_create_and_register` + planted `_trig/reg_manifest.toml` + `_trig/reg_member`;
    3 build tests; 2 fix e2e [convergent + suggested-under-bare]; facts 21->22;
    README 26+60; CHANGELOG; options snapshot). Full preflight green.
  - **Phase 1b AUDIT-HARDENED (2 independent worktree-isolated agents base-verified
    at 7b86e51b + own CLI probing).** Verdict: SECURE (W2 demotion, confinement,
    TOML-injection escaping, `--changed` blast-radius all sound -- proven), gate
    cascade structurally sound (W2 teeth by mutation, property net 115/115
    fired+converged), but NOT fully correct -- fixed 2 HIGH + several MED, all gated:
    (1 HIGH, PRE-EXISTING+SHARED) `finalize`'s blanket `\n`->`\r\n` DOUBLED the `\r`
    toml_edit keeps INSIDE a multi-line string -> INVALID TOML on a uniformly-CRLF
    manifest (also reachable via set_value/remove_value); FIX = normalize `\r\n`->`\n`
    first. (2 HIGH) `normalize:` made the fixer register the NORMALIZED value (a path
    that doesn't exist) while `check` falsely went green; FIX = REJECT `normalize` on
    `registered` (compare verbatim). (A#1 HIGH) the flagship `files: "crates/*"`
    matched LOOSE files (README/.gitkeep) -> corrupt members; FIX = document the
    manifest-anchored `crates/*/Cargo.toml` + `register_as: {dir}` pattern everywhere
    + a loose-file-exclusion e2e gate. (A#2/B#5 MED) a multi-match target query
    (`$..members[*]`) appended to the WRONG array; FIX = reject a non-static array
    query at build. (A#4 MED) `--baseline` un-grandfathered siblings because the
    per-target key encoded the whole missing set; FIX = ONE violation PER-MEMBER with
    a stable per-member key (viable because read_for_fix is compose-aware, so N
    appends to one file accumulate). (A#6) a `./`-prefixed named source read as
    missing; FIX = normalize_confined it. (A#7) a zero-match glob silently passed;
    FIX = fire "matched no files". (B#3) added a post-splice re-parse+member-present
    verify (declines rather than write corrupt/wrong bytes). Gates: CRLF+multiline
    finalize test (append + set_value), loose-file-exclusion e2e, normalize-reject +
    multi-match-reject build tests. **DEFERRED (documented): A#5 (register into an
    ABSENT `members` array -- create the array) -> Phase 2 alongside the create half;
    A#5a (can_fix over-promises when the array is absent -- inherent fix-time state
    like FileRenameFixer, fails LOUD not silent); B#4 (a new element in a MULTI-LINE
    array is appended inline after the last, not on its own line -- valid TOML, a
    formatter tidies).** **REUSABLE LESSONS: (1) `finalize`'s CRLF restore must
    normalize `\r\n`->`\n` before `\n`->`\r\n`, or it doubles a `\r\n` toml_edit keeps
    inside a multi-line string (invalid TOML) -- a PRE-EXISTING bug all 3 toml_edit
    whole-doc ops share; (2) a value fixer must register the RAW value, never a
    normalized one -- reject `normalize` where the fixer echoes the compared value;
    (3) per-member (not per-target-with-a-list) baseline keys keep each finding's
    fingerprint stable so partial progress doesn't un-grandfather siblings; (4) an
    array-locating JSONPath for a fixer must be STATIC (one array) -- reject residual
    wildcards, or the check unions while the fix targets the first.**
  - **Phase 2 -- create-if-missing + the 5.2.4 transaction.** NOT STARTED: the
    conditional `CreateFile` half + the stage/verify-against-staged/all-or-nothing
    write group + the injectable-writer `test-hooks` seam + fault-injection tests.
    Also the JSON/YAML `document_append` fast-follow. Risk: medium (builds the
    multi-file-transaction infra + reworks the engine write step).
  - Each phase: full preflight + an independent adversarial audit round.

After create-and-register: Phase 3 is COMPLETE; on to Phase 4.

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
- **`fix --dry-run` collision-awareness (generic; from the relocate audit, Agent A
  #1).** A plain `--dry-run` has no stage sink, so the staged-collision dedupe
  (stage-mode-only) does not run: N nested files sharing a basename each report
  "would move -> root/X", over-counting vs the real `fix` (1 applied, N-1 skipped)
  and disagreeing with `--diff` (which dedupes). Bare `fix` (suggestion mode)
  likewise suggests all N to the same slot via the stateless per-violation
  `fix_edit`. No data risk (the real apply dedupes via the collision check), purely
  preview/count fidelity. relocate makes it a NORMAL case (two stray lockfiles in a
  monorepo), but the fix is generic (shared with `file_rename`/`file_create`): give
  the `--dry-run` branch in `Engine::fix` a throwaway `stage_ops` sink so the
  collision guard fires there too. Its own focused change + a CLI-count gate; NOT
  bundled into the relocate op (would under-test the other affected ops).
- **Backport relocate's stricter no-clobber guards to `FileRenameFixer`.** relocate
  now uses `symlink_metadata` (not `exists()`) for the target-collision check (so a
  dangling symlink is never clobbered) and guards `has_pending_write` on the
  DESTINATION as well as the source (so a same-pass composed write to the target is
  not raced). `FileRenameFixer` still uses `exists()` + a source-only pending-write
  guard (the shared baseline the audit flagged as Agent A #4 / Agent B F4). Both are
  pathological, but the guards are cheap and safe; apply the same two to
  `FileRenameFixer` for parity.
- **Docs (RELEASE-BLOCKING for v0.17): `docs/site/concepts/adoption/fixing.md` is
  arc-stale.** It still says "The twelve ops" and lists only the 7 content + 5
  path ops (its frontmatter `description:` too), missing the NINE added across the
  arc: `set_value`, `remove_value`, `replace`, `chmod`, `git_untrack`, `command`,
  `dir_create`, `sync_from`, `relocate`. This is pre-existing, arc-wide drift (not
  a `relocate` regression), so it wants ONE comprehensive rewrite of the catalogue
  section + the `fix_size_limit` coverage list (the located structured ops read
  the target too) at release, not a per-op patch that would leave "twelve"
  inconsistent. The README (26 + 60) and CHANGELOG are current; this site page is
  the gap. (No count-gate catches it -- the prose "twelve" is not machine-checked;
  consider a `num_before("ops")` gate on this page as part of the fix.)

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
