# Case-study revalidation log — 2026-05-07

Master tracking file for the 30-case-study revalidation pass. Each
batch of subagents writes its findings into the matching section
below. The parent agent aggregates cross-cutting patterns into the
"Cross-cutting findings" section at the bottom.

## What this pass checks

The case-study READMEs were written incrementally over the P2a +
P2b waves and accumulated drift against alint's current reality:

- **Pitfall numbering drift.** Catalogue went 12 → 17 → 19 → 21
  across waves; READMEs may cite stale numbers.
- **Rule-kind candidate drift.** Some "v0.10 candidate" entries
  promoted to "v0.10 ship-target" via deeper data; sources counts
  up. Some "v0.11 flagship" promoted to "v0.11 ship-target."
- **Version-pin drift.** v0.9.16 was tag-only-no-artifacts;
  v0.9.17 is the published release. Anything saying "will be fixed
  in v0.9.16" should reflect that v0.9.17 shipped the fix.
- **Pitfall fixes shipped in engine.** Pitfall #18 (per-rule
  `respect_gitignore: false`) and #19 (literal_is_nested runtime
  guard) shipped in v0.9.17. Workarounds in case-study READMEs
  pointing at these may now be superseded by direct fixes.
- **Bundled-ruleset rule counts.** Authoritative counts (as of
  2026-05-07): oss-baseline=15, rust=11, python=9. Other rulesets
  to be confirmed by agents.

## Per-case-study findings

Per-batch findings live in companion files:

- [`case-study-revalidation-batch-1.md`](case-study-revalidation-batch-1.md) — angular, airflow, arrow, spark, ruff (336 lines, 5 case studies)
- [`case-study-revalidation-batch-2.md`](case-study-revalidation-batch-2.md) — uv, bazel, clap, deno, dotnet-runtime (281 lines)
- [`case-study-revalidation-batch-3.md`](case-study-revalidation-batch-3.md) — react, flutter, golang/go, helm, istio (569 lines)
- [`case-study-revalidation-batch-4.md`](case-study-revalidation-batch-4.md) — k8s, typescript, vscode, nixpkgs, nodejs (271 lines)
- [`case-study-revalidation-batch-5.md`](case-study-revalidation-batch-5.md) — pnpm, prettier, protobuf, cpython, pytorch (470 lines)
- [`case-study-revalidation-batch-6.md`](case-study-revalidation-batch-6.md) — rust, tensorflow, tokio, next.js, turbo (236 lines)

### Batch 5 (2026-05-07) — pnpm, prettier, protobuf, cpython, pytorch

Full findings in
[`case-study-revalidation-batch-5.md`](case-study-revalidation-batch-5.md).
Headlines:

- **5/5 READMEs updated** with Validation status footer + Future
  analysis section.
- **Universal stale-reference pattern**: every README cites stale
  source counts for `cross_file_value_equals` /
  `registry_paths_resolve` / `ordered_block` / `generated_file_fresh`
  / `import_gate` — all five are now v0.10 ship-target.
- **Rule-count drift**: prettier (24→68), cpython (39→72) understate
  total by ~50% by ignoring bundled rules. pnpm + pytorch are close.
  protobuf matches exactly (108).
- **`cross_language_implementation_complete`** still framed as v0.11+
  candidate (4 sources) in protobuf; now v0.11+ ship-target (5
  sources, added angular). Updated.
- **One .alint.yml flagged**: `examples/pytorch-pytorch/.alint.yml`
  uses `root_only: true` with multi-segment literal paths in 3 rules
  (pitfall #19 shape). Rules fire correctly post-v0.9.17 fix but the
  flag is misleading; recommend dropping `root_only:` from those 3
  rules. Engine bug not present; this is a config-DX cleanup.
- **No `respect_gitignore: false` workarounds** anywhere in this
  batch — pitfall #18 doesn't apply.
- **Live-tree recheck**: protobuf at /tmp/protobuf produces the exact
  150-violation / 14-failing summary the README claims. Engine
  behaviour stable across v0.9.16 → v0.9.17.

## Cross-cutting findings

Aggregated from all 6 batch reports. Patterns observed across
multiple case studies (≥3 batches), in descending order of
frequency:

1. **Stale rule-kind candidate status was the dominant drift
   pattern.** Every batch reported at least one v0.10/v0.11
   candidate that has been promoted to a ship-target since the
   case study was written. The most-frequently-cited promotions:
   - `cross_file_value_equals` v0.10 candidate → v0.10 ship-target
     (now 10 sources past saturation; cited in airflow, vscode,
     nodejs, nixpkgs, react, pnpm, protobuf, tokio, next.js,
     turbo, istio's per-file-extractor variant)
   - `cross_language_implementation_complete` v0.11+ flagship →
     v0.11+ ship-target (now 5 sources: arrow + tensorflow +
     protobuf + angular + flutter; cited in arrow, tensorflow,
     protobuf, angular, flutter)
   - `xml_path_*` v0.10 candidate → v0.10 ship-target (now
     2 sources: spark + dotnet/runtime at ~2,300 manifests; cited
     in spark, dotnet-runtime)
   - `ordered_block` v0.10 candidate → v0.10 ship-target (now
     7 sources: rust, airflow, tokio, cpython, arrow, golang/go,
     protobuf failure_lists; cited in rust, airflow, tokio,
     cpython, arrow, golang/go, protobuf, flutter)
   - `registry_paths_resolve` 6 → 8 sources, v0.10 ship-target
   - `apache/governance@v1` v0.10+ idea → v0.10 ship-target
     (3 Apache TLPs: arrow, spark, airflow)
   - `dotnet@v1` not on candidate list → v0.10 ship-target (new,
     surfaced by dotnet/runtime)

2. **Bundled-overlay rule counts were systematically
   underestimated** in 5+ READMEs. Cause: ruleset content grew
   over time (oss-baseline 13 → 15 between v0.9.5 and v0.9.17),
   case-study prose wasn't kept current. Most-affected: arrow
   (35 → 44), spark (49 → 52), prettier (24 → 68), cpython
   (39 → 72), helm (23 → 58). Validation footers added in this
   pass include the `validate-config` rule count as the
   authoritative reconciler.

3. **Pitfall #18 fix verified end-to-end.** v0.9.17 ships per-rule
   `respect_gitignore: false`.
   - Bazel agent verified on `/tmp/bazel/.bazelversion` (passes
     with the override, fails without). Bazel `.alint.yml` was
     flagged for a follow-up edit to re-add the dropped
     `bazel-version-file-exists` rule using the new override.
   - Flutter README updated to reflect that pitfall #18 is fixed
     in v0.9.17. Flutter is the 2nd source for the underlying
     pattern (`pubspec.lock` tracked-AND-gitignored).

4. **Pitfall #19 fix has zero relevance to existing case
   studies** — none of the 30 .alint.yml configs currently use
   the pattern (`root_only: true` with multi-component literal).
   Pytorch was flagged for a separate config-DX cleanup: 3 rules
   use `root_only: true` with multi-segment literals. The rules
   fire correctly post-v0.9.17 fix (the literal_is_nested guard
   handles them), but the flag is misleading; recommend dropping
   `root_only:` from those 3 rules. Engine bug not present; this
   is a config-DX cleanup.

5. **Pitfalls #20 + #21 remain open with documented workarounds.**
   istio is the named source for both v0.10 design candidates
   (`value_extractor:` block on `cross_file_value_equals`;
   `multi_doc_mode:` knob on `yaml_path_*`).

6. **Pitfall-count drift was uniform.** 14 of 30 READMEs cited
   stale catalogue sizes (12 / 16 / 17 / 19); all updated to 21.

7. **`agent-context@v1` overlay underadopted.** Only 2 of 30
   case studies extend it despite many shipping AGENTS.md or
   equivalent. Flagged in batch 3 + batch 4 Future-analysis
   sections.

8. **`compliance/reuse@v1` / `compliance/apache-2@v1`** would
   simplify per-rule license-header constructs in react, helm,
   istio, multiple Apache TLPs. Flagged in 4+ batches.

9. **`nested_configs: true`** would benefit monorepo-shaped
   repos (istio per-component subdirs, flutter engine subtree,
   nixpkgs by-name fan-out). Flagged in 3+ batches.

10. **`scope_filter`** refactor opportunity surfaced consistently
    in batch 6 (rust, tensorflow, next.js, turbo) — documented
    as a v0.10+ adoption-ladder rung in each Future analysis.

11. **`alint suggest` against case-study directories yields zero
    proposals** (only README + config in each dir; needs a real
    working tree to be meaningful). Live-tree rechecks pending
    for most repos — only `/tmp/bazel/`, `/tmp/protobuf/`,
    `/tmp/nodejs-node/` are present.

12. **Two new bundled-ruleset opportunities surfaced from this
    pass** that weren't previously catalogued: `tidy@v1`
    (proposed by rust-lang/rust agent — would cover ~13 of ~32
    src/tools/tidy/ checks declaratively, raising the case-study
    pitch from "18 lines" to "1 line"), `bazel-monorepo@v1`
    (proposed by tensorflow agent — would replace per-config
    Bazel-monorepo detection that `alint suggest` currently
    misses).

13. **No new pitfalls or new ≥3-source rule-kind candidates
    surfaced from this revalidation pass.** Matches the
    saturation observation documented in `launch-evidence.md`.
    The candidate backlog is stable at the counts recorded
    there.

## Open issues / gaps / inconsistencies / opportunities

Punch list distilled from cross-cutting findings + per-batch
"flagged but not auto-fixed" items. Each entry tagged by scope:

**Engine / config flags (zero engine bugs):**

- **`examples/bazelbuild-bazel/.alint.yml`** — re-add the dropped
  `bazel-version-file-exists` rule using v0.9.17's per-rule
  `respect_gitignore: false` knob. Demonstrates the pitfall #18
  fix on its canonical motivating example. (Out of revalidation
  scope; flagged as a small follow-up commit.)
- **`examples/pytorch-pytorch/.alint.yml`** — drop `root_only:
  true` from 3 rules with multi-segment literal paths. Rules fire
  correctly post-v0.9.17 (literal_is_nested guard handles them)
  but the flag is misleading. (Out of revalidation scope; flagged
  as a small follow-up commit.)

**Bundled-ruleset adoption (low-effort wins):**

- 28 of 30 case studies don't extend `agent-context@v1` despite
  shipping AGENTS.md or equivalent.
- 4+ case studies (react, helm, istio, multiple Apache TLPs)
  carry per-rule license-header constructs that
  `compliance/reuse@v1` or `compliance/apache-2@v1` would
  consolidate.
- 3+ monorepo-shaped repos (istio, flutter, nixpkgs) would
  benefit from `nested_configs: true` for subtree-scoped rules.

**Bundled-ruleset proposals (deferred to the v0.10 ship-list):**

- `tidy@v1` — rust-lang/rust source, 13/32 tidy checks coverable.
- `bazel-monorepo@v1` — tensorflow source, plus bazel-shape
  monorepos that `alint suggest` doesn't currently flag.

**Live-tree rechecks pending for 27 of 30 repos** — only bazel,
protobuf, nodejs-node have a /tmp/<repo>/ checkout to validate
against. The case-study pages document this in their Validation
status footers. Future re-runs that fresh-clone all 30 would
provide stronger drift evidence; deferred for now (bandwidth).

**Future analysis sections** were added to every README. The
2-3 ideas per repo are concrete, scoped, and documented; no
single one is launch-blocking.
