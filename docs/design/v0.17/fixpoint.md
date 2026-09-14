# The fix fixpoint: re-walk, converge, cap (Phase 1, increment 2)

Status: design accepted 2026-09-13 (decisions below); 2a (core fixpoint)
implemented and gated 2026-09-13. 2b (`--changed` confinement) is next.
Scope: `alint fix` only. `alint check` still walks exactly once per invocation.

## 1. Problem

Through Phase 0 and Phase-1 increment 1, `alint fix` is a **single pass**
(`Engine::fix` calls `fix_run` once): evaluate every rule against one index,
apply fixers, done. That is correct for a self-contained fix but cannot handle a
**cascade**, where applying one fix creates work for another:

- Rule A's `file_create` writes `REQUIRED.md`; rule B (`file_content_matches` on
  `**/*.md`) must then add a header to it -- a second pass.
- A `file_rename` vacates a path a stale-index violation still points at.
- A content edit changes a file a cross-file rule relates to another.

A single pass is also insufficient for a fixer that makes **partial progress**:
`replace` clears matches left-to-right but its replacement can re-form the
forbidden pattern across a splice boundary (`--` -> `-` on `---` yields `--`),
which it explicitly defers to the fixpoint to finish (`--- -> -- -> -`).

Increment 2 activates a **byte-level fixpoint**: re-walk and re-fix until a pass
changes nothing (a fixed point), bounded by a **cap** that turns a genuinely
non-convergent config into a loud exit-2 failure rather than an infinite loop.
(An earlier design added an `apply-once` guard on top of this; the 2a audit
removed it -- it broke `replace`'s multi-pass progress and, worse, silently
converged genuine non-convergence. See decision 1 and section 5.)

## 2. Decisions (asamarts, 2026-09-13)

1. ~~**Apply-once key = `(rule_id, file, violation-fingerprint)`** (fine, per
   violation), reusing `baseline::violation_fingerprint`.~~ **SUPERSEDED after the
   2a audit (2026-09-13) -- apply-once was removed; see the note below.** The
   original intent (don't re-attempt a non-resolving fix) assumed a re-forming
   violation would get a *new* fingerprint each pass so the cap could catch it.
   But `violation_fingerprint` for a path-bearing rule is **constant across
   passes** (it keys on `(rule_id, path)` / `baseline_key`, not content), so
   apply-once filtered a legitimately re-forming violation after one pass and the
   loop "converged" against a still-violating file (`replace` on `file_content_forbidden`:
   `--` -> `-` on `---` stopped at `--`, exit 0, `check` fails). Two further facts
   made apply-once pure downside: every shipped fixer already self-bounds
   (`file_append` skips when the file already ends with the content; `replace`
   drops a self-reintroducing match; normalizing fixers are idempotent), so it
   guarded nothing real; and its one live effect was to turn a genuinely
   non-convergent config's *loud* exit-2 into a *silent* exit-0 with the violation
   standing. **Resolution: byte-level fixpoint** -- re-run while a pass changes the
   tree (`applied() > 0`), converge when a pass applies nothing, cap the rest. The
   Ruff/ESLint model, and the Dershowitz-Manna argument (5) holds directly. This
   preserves decisions 2-4.
2. **Non-convergence cap = 10 passes -> hard error, exit 2.** On the cap, alint
   prints a loud stderr error naming the rules/files that keep re-triggering and
   exits `2` ("fix could not complete"), distinct from `1` ("fix ran, violations
   remain"). Fixes are applied incrementally per pass, so whatever landed before
   the cap stays on disk (no rollback); the cap stops the loop and reports.
3. **Re-walk = full re-walk each pass** (rebuild the index with `walk`, re-run
   all rules). Correct by construction (see 4). A targeted incremental re-check
   is deferred and decided from measured data (see 6); the naive "re-check only
   the touched files" is **unsound** (see 3) and is not shipped.
4. **Scope split.** 2a: the core fixpoint (this doc) + benchmarks. 2b: `--changed`
   write-confinement. 2a under `--changed` is safe meanwhile (existing
   `skip_for_changed` still confines writes to the changed set; a create-then-
   cascade merely under-fixes rather than corrupting).

`--dry-run` / `--diff` stay single-pass previews: they do not write, so there is
no changed tree to re-walk. They show the first pass; a real `fix` may do more.
This is documented, not a defect.

## 3. Dependency model: what a fix can change, and whose verdict it affects

A pass applies edits of three shapes: **content** (in-place rewrite of a file's
bytes), **path** (create / remove / rename -- changes the file *set*), and
(Phase 3) **mode**. The next pass must re-evaluate exactly the rules whose
verdict *could* differ. alint's rules partition by dependency:

- **Per-file rules (28 of 80 kinds; `Rule::as_per_file` is `Some`).**
  `verdict(rule, X) = f(bytes(X), path(X), run-constant facts/vars)`. A content
  edit to X can change only rules-on-X. Editing Y cannot change a per-file
  verdict on X. -> *A targeted re-check of the touched files is sound for these.*
- **Whole-index / cross-file rules (52 of 80).** `file_exists`, `file_absent`,
  `dir_*`, `cross_file`, `file_graph`, `filename_case`, `filename_regex`,
  `every_matching_has`, `for_each_dir`/`for_each_file`, `executable_bit`,
  `generated_file_fresh`, ... Their verdict depends on the **file set** and/or on
  **several files' content**. A create/remove flips `file_absent`; a rename flips
  `filename_case`; a content edit to X flips a `cross_file` X<->Y relation even
  though **Y was never touched**. -> *A touched-files-only re-check silently
  misses these; they must re-run whenever any file/path in their scope changed.*
- **Fact-dependent rules.** Facts are computed once at the top of `fix_run`. Two
  fact kinds read file content -- `file_content_matches` and `custom` (spawns a
  process that may read files). A content fix can flip such a fact, which can
  flip a rule's `when:` gate and thus its applicability on the next pass. ->
  *Facts must be re-evaluated when their inputs changed.*

**Consequence.** A *correct* incremental re-check requires all three: re-check
touched files (per-file), re-run whole-index/cross-file rules on any relevant
delta, and re-evaluate facts on input delta -- a real dependency-tracking
subsystem with a completeness obligation. The **full re-walk is the trivial
complete case**: it re-evaluates every rule over a freshly-walked index, so it is
a superset of any correct dependency closure and cannot miss a changed verdict.
`walk` and rule evaluation are deterministic, so the re-walk is deterministic.

## 4. Correctness of the full re-walk

Claim: running `{walk; evaluate all; apply}` to a fixpoint never leaves a
violation that a fix could have resolved *and* never applies a fix against stale
state.

- **No missed re-evaluation.** Each pass rebuilds the index from disk and
  evaluates every rule against it, so the dependency closure of the previous
  pass's edits (section 3) is fully contained in what the next pass evaluates.
- **No stale-state apply.** Located edits already defer against a mid-pass
  whole-file change (increment 1's byte-consistency guard); across passes, the
  fresh walk + fresh read means each pass's edits are computed against the bytes
  they are applied to.
- **Determinism.** `walk` yields a deterministic index; evaluation order is
  fixed; the apply order is the engine's existing deterministic order. Same input
  tree + config -> same sequence of passes.

## 5. Termination (byte-level fixpoint)

The loop terminates on **progress, not on a fingerprint heuristic** (the original
apply-once design was removed after the 2a audit; see decision 1). It has two
mechanisms:

- **Converge on no change (primary).** After each pass, `Engine::fix` checks
  `report.applied()`: a pass that applied nothing is a **fixed point** and the
  loop stops. A pass that applied something re-walks and runs again. `applied()`
  is a faithful "the tree changed" signal because every effect is idempotent at
  the fixer level: a whole-file fixer returns `Skipped` when there is nothing left
  to do (so `Applied` <=> it changed bytes), a create/remove/rename returns
  `Skipped` when the target is already in the desired state, and a located batch
  that nets no byte change is reported `Skipped` (so an identity edit is not read
  as progress). Termination is the Dershowitz-Manna well-founded-multiset contract
  of auto-fix.md 5.8: a Safe/normalizing fix strictly reduces the violation
  multiset without introducing new violations, so each progressing pass descends
  and the descent is finite. A `replace` that re-forms a match across a splice
  boundary (`--` -> `-` on `---`) keeps making progress (`--- -> -- -> -`) and
  then a pass finds no match and converges -- exactly the multi-pass case the
  removed apply-once heuristic wrongly cut off after one pass.

  *Why no apply-once:* the shipped fixers already self-bound (their own
  idempotence guards make a non-progressing pass apply nothing), so a fingerprint
  "tried it, don't retry" set added no termination guarantee -- and, keyed on a
  content-independent fingerprint, it actively broke legitimate multi-pass
  progress and silently converged genuine non-convergence. Removing it makes the
  loop both simpler and strictly more correct.

- **Cap (backstop).** A genuinely non-convergent config changes the tree every
  pass and never reaches a fixed point: two `replace` rules that undo each other
  (`a` -> `b`, `b` -> `a`), or any fixer that rewrites without settling. The loop
  hard-stops after **`MAX_PASSES` = 10** and reports non-convergence -- exit 2,
  with a loud stderr line naming the rules that were still applying on the final
  pass (`last_applied_rules`; a pass that applies nothing would have converged, so
  the final pass of a capped run always applied >=1 and the culprit set is
  non-empty). Whatever landed before the cap stays on disk (no rollback). This is
  the honest opposite of the removed apply-once, which would have converged such a
  config silently at exit 0 with the violation standing.

  The one edge to keep the primary signal honest: a **located identity edit**
  (replacement equal to the spanned bytes, or a batch whose edits cancel) makes no
  byte change, so the loop must not read it as progress. When a located batch nets
  `new_bytes == original`, its edits are reported `Skipped` ("left the file
  unchanged"), not `Applied`. `ReplaceFixer` already refuses to emit such an edit,
  so this only guards a future located fixer or the engine test fixture.

## 6. Performance: measure, then decide

The full re-walk costs `O(passes x single-pass-cost)`; passes converge in 1-3 for
realistic configs, and `fix` is a deliberate mutating command, not the
per-keystroke hot path the sub-second floor protects. Plan:

- Add a fixpoint benchmark cell (a create-then-cascade + a converging content
  fixpoint) to the det-perf / bench harness and record a baseline on `kbench`
  (the canonical host; `ALINT_BENCH_DROP_CACHES=1`), both Valgrind-Ir (load-
  immune) and wall-clock.
- Gate it so a future regression is caught.
- **Only** build the targeted incremental re-check (section 3's proven closure)
  if the measured full-re-walk cost is material -- and then against this model,
  gated by the harness. Do not pre-optimize behind an unproven dependency map.

## 7. Report semantics across passes

`Engine::fix` aggregates the per-pass `FixReport`s into one with **Applied-wins
per violation** semantics, keyed by `violation_key` (the baseline fingerprint --
an aggregation key here, NOT a termination device):

- An **`Applied`** for a violation WINS and is *locked*. Once a pass records an
  `Applied` for a key, a later pass cannot overwrite it. This matters because a
  fix that lands in an early pass will, on the confirming pass, find nothing to do
  and report a `Skipped` (its own idempotence guard) -- that later skip must not
  mask the fix that actually happened. `Applied` items across distinct violations
  thus accumulate (a cascade legitimately applies over several passes).
- A violation **never yet `Applied`** keeps its LAST status: a later pass's item
  for the key retains-out the earlier one, so a transient state is superseded by
  the real outcome once a later pass acts on it.
- **Within-pass siblings both survive.** A located batch can emit an `Applied`
  edit AND an isolation-conflict `Skipped` under one source violation (same key).
  The lock is applied only AFTER a whole pass's items are merged, so the sibling
  skip is kept; and it does not re-appear, because the batch that produced it made
  progress and the next pass's re-eval differs. This is the
  `located_regime_applies_batch_and_excludes_isolation_group` invariant, now
  asserted through the multi-pass loop.
- **Transient defers are not reported.** The located byte-consistency / removal
  defer (a batch yielding to a concurrent write, increment 1) emits no provisional
  "rerun to apply" skip: the re-walk IS the rerun, so the retry pass reports the
  real outcome. Emitting one would duplicate that outcome or, when the same
  concurrent change resolves the source, strand a stale skip that wrongly drives a
  nonzero exit.

The `fix_exit_code` contract (round-7) is unchanged except for the new exit-2
non-convergence case: `fix_exit_status` returns 2 when `non_convergent`, and it
outranks every other branch including `--fix-only`'s residual suppression. The
flag is also surfaced in `--format json` (`summary.non_convergent`) -- the only
structured signal of the distinct exit 2, since a capped run's items are mostly
`applied` and otherwise look like a clean run.

## 8. Scope + gates

- **2a (this increment, delivered):** the byte-level fixpoint loop in
  `Engine::fix` (re-walk via `walk`, converge on `applied() == 0`, `MAX_PASSES =
  10`), the located no-op reporting that keeps `applied()` honest, the cap +
  exit-2 (`fix_exit_status`), Applied-wins report aggregation, `non_convergent` in
  the JSON summary, and these gates:
  - the **boundary-re-match** gate (the audit's F1 regression):
    `replace_boundary_rematch_converges` -- `replace` `--` -> `-` on `---` runs
    `--- -> -- -> -` to a CLEAN file in one `fix` (a premature cutoff stopped at
    `--`, exit 0, `check` failing);
  - a **progressive-convergence** gate: `fix_converges_after_several_progressing_passes`
    (engine) -- a fixer making progress over several passes runs to completion, not
    cut off after one;
  - a **cascade** gate: `create_then_content_fix_cascades` -- `file_create` then a
    content fix on the created file converges in a single `fix`;
  - the **content-edit + rename** gate:
    `content_edit_and_rename_same_file_no_corruption` -- one `fix` lands both, no
    duplication;
  - the **append-once** end-to-end gate: `append_applies_once_when_not_self_satisfying`
    -- a non-self-satisfying `file_append` leaves exactly one copy (its own
    self-guard, confirmed end-to-end);
  - a **cap** gate: `fix_bails_at_the_cap_on_a_nonconvergent_config` (engine) --
    a never-settling fixer hard-stops at 10 with `non_convergent`, capped writes on
    disk, no rollback;
  - an **exit-2** gate: `nonconvergent_config_hits_the_cap_and_exits_2` (CLI, two
    oscillating `replace` rules) -- exit 2 + loud stderr naming a stuck rule; plus
    `fix_exit_status_maps_the_fix_contract` (unit) for the exit-code precedence;
  - a **`--changed` multi-pass confinement** gate:
    `fix_changed_confinement_holds_across_the_multipass_fixpoint` -- an in-scope
    create-then-trim cascade completes across the re-walk while an out-of-diff file
    is never touched (the "safe under `--changed`" claim);
  - the JSON **non-convergence** gate: `non_convergent_report_surfaces_the_flag_and_validates`;
  - the property invariants (`fix_is_idempotent`, `fix_converges...`) re-pointed:
    stale "single-pass in Phase 0" rationale corrected, and `fix_converges...` now
    asserts `!non_convergent` (no single fixable rule may fail to converge -- the
    direct invariant that replaces the old fingerprint-stability coupling).

  The ARCHITECTURE "walk once" invariant (principles 1 and 4, and the pipeline
  invariants block) is amended for the fix path (R-WALK). Follow-up (tracked, not
  blocking 2a): a fixpoint **benchmark cell** on `kbench` (create-then-cascade + a
  converging content fixpoint), recorded per the bench protocol.
- **2b (next):** `--changed` confinement -- created files join the changed set so
  create-cascades finish; a required out-of-scope write demotes to `Suggestion`
  (auto-fix.md 5.7).
