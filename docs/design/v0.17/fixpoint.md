# The fix fixpoint: re-walk, apply-once, termination (Phase 1, increment 2)

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

It is also **unsound for a non-normalizing fixer that does not resolve its own
violation**. The motivating regression: a `file_content_matches` + `file_append`
rule whose appended boilerplate does not literally contain the rule's `pattern:`
(an SPDX/licence header) re-appends **every** pass -- N duplicated copies, a
file-corruption regression for an existing valid config -- unless the loop knows
not to re-attempt a fix it already tried.

Increment 2 activates a **fixpoint**: re-detect and re-fix until nothing new
applies, with an **apply-once** guard that makes "nothing new applies"
well-defined and a **cap** that bounds the pathological case.

## 2. Decisions (asamarts, 2026-09-13)

1. **Apply-once key = `(rule_id, file, violation-fingerprint)`** (fine, per
   violation), reusing `baseline::violation_fingerprint`. A rule may re-fix a
   file across passes only for a genuinely *new* violation; a re-appearing
   identical violation is never re-attempted. Matches the plan (Ruff/ESLint
   model); the cap backstops re-matches whose fingerprint changes each pass.
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

## 5. Termination

Two mechanisms, in order:

- **Apply-once (primary).** The loop maintains a set of attempted violation
  fingerprints (`violation_fingerprint` already encodes `rule_id` + path +
  identity). Before dispatching a rule's violations, those already in the set are
  skipped. A survivor's fingerprint is added **only if its fix was `Applied`**
  (see the interaction note below). So each progressing pass either resolves a
  violation (gone next pass) or marks a non-resolving-but-applied one (skipped
  next pass) -- the set of still-appliable violations strictly shrinks by >=1, so
  the loop terminates. For a **normalizing** fixer the fix removes the violation,
  so its fingerprint does not recur; the loop terminates in a few passes without
  the cap. This is the Dershowitz-Manna well-founded-multiset contract of
  auto-fix.md 5.8: a Safe fix strictly reduces the violation multiset without
  introducing new violations, so the fixpoint descends to empty.

  **Granularity (as-built).** The apply-once fingerprint is computed with
  `file_bytes: None`, so `violation_fingerprint`'s discriminator is: the
  `baseline_key` if the rule set one; else, for a **path-bearing** violation, the
  empty discriminator -- identity is `(rule_id, path)`, and the (volatile)
  message and line number are NOT hashed; else (pathless) the trimmed message.
  Consequence: a rule fixes a given `(rule_id, path)` **at most once per `fix`
  invocation**, regardless of how its message/line shift as the file changes.
  This is exactly what makes real fixers bound naturally -- a `file_append` /
  `file_prepend` / `replace` / whole-file content fixer re-firing on the same
  file after its own edit is filtered, so it cannot loop. The cap (below) is only
  reachable by the residual pathological shape the fingerprint cannot bound: a
  **pathless** violation whose message changes every pass (a rule with no stable
  identity that never resolves). That shape is what the `fix_bails_at_the_cap...`
  engine test constructs to exercise the cap deterministically.

  **Interaction with increment-1's within-pass defers (audit finding).** Two
  outcomes are "skip THIS pass, retry NEXT": a `file_rename`/`file_remove`
  yielding to a pending content edit on the same file (`has_pending_write`), and
  the located byte-consistency defer. These fire exactly when a content fixer and
  a path/located fixer touch one file -- the common case the fixpoint exists to
  resolve (the next pass sees the flushed content and lands the deferred edit).
  Marking a violation attempted **before** dispatch (or on any non-applied
  outcome) would mark these and the next pass would skip them, silently **losing**
  the deferred fix. Marking **only on `Applied`** avoids it with no "is-this-a-
  defer" flag: a deferred edit is not applied -> not marked -> retried and landed
  next pass, then (if it now persists) marked. A terminal skip (binary / size /
  unfixable / below-threshold suggestion) is also not marked; it re-evaluates each
  pass but contributes zero applied, so it neither blocks termination nor is lost
  -- the cross-pass report (section 7) dedupes it by fingerprint.

  **Located-path fingerprint (audit finding).** The located path reports synthetic
  "located edit in X" items whose fingerprint does NOT match the originating rule
  violation's (different path-identity derivation). Apply-once must therefore mark
  the **original** violation's fingerprint (the one the pre-dispatch filter saw),
  threaded through the located collection: remember each `(rule, file)` batch's
  originating fingerprints at collect time and mark them iff that file's batch was
  applied (not deferred).
- **Cap (backstop).** A *non-normalizing* fixer can churn fingerprints -- each
  pass produces a violation with a *new* fingerprint (a rewrite that shifts
  content, or two rules that oscillate). Apply-once does not bound that, so the
  loop hard-stops after **10 passes** with the exit-2 error. The
  increment-1 `replace` guard (drop a replacement that itself still matches)
  already removes the common self-reintroducing case *before* it reaches the
  loop; the cap covers the residual (e.g. a boundary re-match, or a genuine
  oscillation between two rules).

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

`Engine::fix` aggregates the per-pass `FixReport`s into one with **keep-last-per-
violation** semantics: as each pass merges in, any earlier item for a violation
it re-touched is retained-out and replaced by this pass's item (keyed by the same
fingerprint `attempt_fingerprint` uses). So a violation's terminal status is its
status on the last pass that produced an item for it, and `Applied` items across
distinct violations accumulate (a cascade legitimately applies over several
passes). Two consequences worth stating:

- **Within-pass siblings both survive.** A located batch can emit an `Applied`
  edit AND an isolation-conflict `Skipped` under one source violation. Apply-once
  marks that source attempted the moment the batch lands, so it never re-appears
  to be superseded -- both items stay in the aggregate (this is the
  `located_regime_applies_batch_and_excludes_isolation_group` invariant, now
  asserted through the multi-pass loop).
- **Transient defers are not reported.** The located byte-consistency / removal
  defer (a batch yielding to a concurrent write, increment 1) no longer emits a
  provisional "rerun to apply" skip: the re-walk IS the rerun, so the retry pass
  reports the real outcome (applied, or a genuine terminal skip). Emitting the
  provisional skip would either duplicate that outcome or, when the same
  concurrent change resolves the source (e.g. the file was removed), strand a
  stale skip that wrongly drives a nonzero exit.

The `fix_exit_code` contract (round-7) is unchanged except for the new exit-2
non-convergence case (`fix_exit_status` returns 2 when `non_convergent`, and it
outranks every other branch including `--fix-only`'s residual suppression).

## 8. Scope + gates

- **2a (this increment, delivered):** the loop in `Engine::fix` (re-walk via
  `walk`, `MAX_PASSES = 10`), apply-once threaded through `fix_run` (marking only
  on `Applied`, keyed off original violation fingerprints on the located path),
  the cap + exit-2 (`fix_exit_status`), keep-last-per-violation report
  aggregation, and these gates:
  - the **apply-once regression** gate: `append_applies_once_when_not_self_satisfying`
    -- a `file_content_matches` + `file_append` whose boilerplate does not satisfy
    the pattern applies exactly ONCE (one appended copy), not once per pass;
  - a **cascade** gate: `create_then_content_fix_cascades` -- `file_create` then a
    content fix on the created file converges in a single `fix` (the created file
    enters the index on the re-walk);
  - the updated **content-edit + rename** gate:
    `content_edit_and_rename_same_file_no_corruption` -- one `fix` now lands both
    (was two manual `fix` steps), no duplication;
  - a **cap** gate: `fix_bails_at_the_cap_on_a_nonconvergent_config` (engine unit
    test) -- a pathless changing-message config hard-stops at 10 with
    `non_convergent`, capped applications on disk, no rollback;
  - an **exit-2** gate: `fix_exit_status_maps_the_fix_contract` -- `non_convergent`
    maps to exit 2 and outranks `--fix-only` and residual findings;
  - the property invariants (`fix_is_idempotent`, `fix_converges...`) re-pointed:
    their stale "single-pass in Phase 0" rationale is corrected, and cross-rule
    cascades now covered by the scenarios above.

  The ARCHITECTURE "walk once" invariant (principles 1 and 4, and the pipeline
  invariants block) is amended for the fix path (R-WALK). Follow-up (tracked, not
  blocking 2a): a fixpoint **benchmark cell** on `kbench` (create-then-cascade + a
  converging content fixpoint), recorded per the bench protocol.
- **2b (next):** `--changed` confinement -- created files join the changed set so
  create-cascades finish; a required out-of-scope write demotes to `Suggestion`
  (auto-fix.md 5.7).
