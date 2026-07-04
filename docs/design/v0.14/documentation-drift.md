# Documentation and site-drift: remediation + prevention (v0.14)

Status: **Plan / Draft** (2026-07-03; revised after an adversarial self-review that
verified every load-bearing claim against the pipeline code). Flip items to `[x]` as they
land. Decisions: [ADR-0007](../../adr/0007-release-aware-documentation.md).
Relationship: extends [`post_v0.13_audit.md`](./post_v0.13_audit.md) Phase 6 (doc drift)
and Phase 7 (site drift W1-W7). This doc adds the drift a fresh two-repo audit
(2026-07-03) found that those phases did not cover, plus the recurrence-prevention layer.

Em dashes are avoided (house convention). Status legend: `[ ]` planned, `[x]` done,
`[~]` partial, `[-]` deferred-with-rationale.

> **Revision note (adversarial review).** The first draft made four claims that
> verification against `docs-bundle.yml` + `docs_export.rs` + `check-version-pins.sh`
> proved wrong or incomplete. They are corrected inline and called out in §9. The
> headline corrections: (1) the leak has THREE main-overlaid vectors, not one (rule
> pages, the LikeC4 model, and `docs/site/reference/**`); (2) Decision 1's `since`
> mechanism is not type-derived for the target kinds and needs an explicit
> released-version oracle plus new sentinel machinery, and the field must be renamed
> (`since` is already a rule-option name); (3) Decision 3's "no exemptions" is infeasible
> and is reframed to "no SILENT counts, with an explicit enumerated allowlist"; (4) the
> "docs/site is tag-pinned" assumption is false for `reference/**`.

## How to read this doc

§2 is the consolidated finding list (status + `file:line`). §3 records the three
decisions AS CORRECTED by the review. §4 is immediate remediation (deployable against
v0.13.0). §5 is prevention. §6 is release-gated v0.14 work. §9 lists the review
corrections so a reader trusts the rest.

---

## 1. Context: the release model and the three leak vectors

Docs are generated from versioned contracts; alint.org consumes them. The load-bearing
split, VERIFIED against `.github/workflows/docs-bundle.yml`:

**Tag-pinned** (live site shows the last RELEASED value, cannot leak): `facts.json`
(explicitly not overlaid, `docs-bundle.yml:88`), the raw `configuration/schema.json`
(the tag's schema, `docs_export.rs:164-166`), the CLI reference pages (built from the tag
binary), and most of `docs/site/**`.

**Main-overlaid** (live site tracks `main`, so doc fixes ship without a release, by
design). The EXACT set (`docs-bundle.yml:97-99` working-tree overlay + dedicated bridge
steps): `docs/site/reference/**`, `docs/design/ARCHITECTURE.md`, `docs/design/ROADMAP.md`,
`roadmap.json`, the LikeC4 `docs/design/architecture/model/**` (`*.c4`),
`docs/design/architecture/crate-graph.md`, `docs/site/about/architecture-diagrams.md`,
`docs/site/concepts/how-it-works.md`, the per-rule `rules/**` pages (rules-only bridge,
`docs-bundle.yml:135-175`), and a regeneration of `benchmarks-trajectory.json` from main
(`docs-bundle.yml:177-190`).

Because the overlay tracks `main`, any content added to an EXISTING overlaid artifact on
`main` reaches the live site before the binary that implements it. The "refresh-existing"
safety (`docs-bundle.yml:167-175`, a copy loop driven by `find target/docs-bundle/rules`)
blocks NEW pages but not NEW content on existing pages (verified). Three concrete leak
vectors follow from this, all confirmed:

1. **Per-rule pages** (rules-only bridge): unreleased Options rows (schema-injected from
   main's schema) and unreleased prose.
2. **The LikeC4 model** (`*.c4` overlay): unreleased architecture elements.
3. **`docs/site/reference/**`** (raw overlay): unreleased reference prose. The workflow
   already flags this risk in a comment (`docs-bundle.yml:73-79`: "Keep reference pages to
   released behaviour"), i.e. it is currently guarded only by author discipline.

Note (verified, no action): `roadmap.json`/`ROADMAP.md` are main-overlaid too, but that is
INTENDED (the roadmap is meant to show planned versions; its per-phase shipped/planned
status is derived from the released `facts.json` version). The v0.14 "Security and
correctness hardening + baseline" phase surfacing on `/roadmap/` is correct, not a leak.

---

## 2. Findings (consolidated, with status + evidence)

### A. Live leaks: unreleased v0.14 content on the live site `[ ]`

The `root_only` EXTENSION to the existence family landed post-v0.13.0 (#98); only
`file_exists` honored it at release (its three siblings' structs had no `root_only` at
v0.13.0). Baseline mode (#83-#94) is likewise unreleased.

- `[ ]` **A1** `root_only` Options-table row on the live `file_absent`, `dir_exists`,
  `dir_absent` pages (schema diff `v0.13.0..HEAD` adds it to these three; injected by
  `docs_export.rs` via `rule_options_table.rs::options_section`). **Confirmed live** on
  `alint.org/docs/rules/existence/file_absent/`. A v0.13.0 user copying it gets a silent
  no-op.
- `[ ]` **A2** `root_only` prose leak for `dir_exists` (`docs/rules.md:92`, main-overlaid).
- `[ ]` **A3** Prose leaks describing unreleased L1/M4 hardening: expanded zero-width set
  (`U+2060`/`U+180E`) on `no_zero_width_chars`, and the `no_symlinks` escaping-symlink
  caveat (`docs/rules.md` diff `v0.13.0..HEAD`). NOTE: the `max`->`max_width`/`max_depth`
  changes in the same diff are NOT leaks (verified: `max_width`/`max_depth` are in the
  v0.13.0 schema; `max:` was an invalid example that would fail `deny_unknown_fields`).
- `[ ]` **A4** (found by the review) **LikeC4 model leak.** `config-model.c4` gained
  `f_baseline = field 'baseline'` since v0.13.0 (hand-authored), so the **unreleased
  `baseline` config key is live in alint.org's interactive architecture diagram**.
  `crate-graph.gen.c4`/`DIAGRAMS.md` also leak a new dev-dep edge (`e2e -> cli.output`),
  trivial severity, same class. This vector cannot use the schema mechanism (§3 D1); it is
  fixed by tag-pinning the arch-model overlay (§5 P-C4).

Boundary note (verified clean, no action): the top-level `baseline:` key entered main's
schema but surfaces only in the tag-pinned `configuration/schema.json`, not the per-rule
Options tables, so it does not leak there. Baseline CLI flags do not leak (CLI pages
tag-pinned). `docs/site/reference/output-formats/**` has no baseline content yet, so the
reference vector (§1.3) is a latent risk, not an active leak.

### B. Stale hardcoded counts on live marketing pages `[ ]`

- `[ ]` **B1** `ls-lint.astro:106` "83-rule catalogue" should be 89 (points at the full
  catalogue). Not in the gate's `CLAIM_FILES`, and the noun is "catalogue" not the gated
  "rule kinds". Straight fix: interpolate `{alint.ruleKinds}`.
- `[ ]` **B2** `benchmarks.astro:26` (meta) interpolates the catalogue count
  (`${alint.ruleKinds}` = 89) into the nixpkgs 273 ms claim, which the body (`:138`,
  `:201`, `:223`) correctly records as the real 79-rule scenario. **The 89 interpolation is
  the bug; 79 is truthful.** See Decision 2.
- `[ ]` **B3** `monorepo-linter.astro:182`, `repository-structure-linter.astro:208`
  hardcode "79-rule" for the same nixpkgs scenario (correct value, but ungated and
  duplicated). Fold into the scenario-count handling (§5 P2).
- `[ ]` **B4** "oss-baseline (15 rules)" hardcoded at `compare.astro:337`,
  `repolinter-alternative.astro:106`. Sourceable once `facts.rs` exposes per-ruleset sizes
  (§5 P2). Verified: oss-baseline.yml = 15.

### C. Already fixed on the audit branch, not deployed `[~]`

Phase 7 W1-W7 landed on the alint.org branch `audit/post-v0.13-drift` (4 commits, NOT
pushed, so NOT live): compare "21"->22 (W1), blog "v0.11"->v0.13.0 (W2), API 404s (W3/W4),
`reference/` gitignore (W5), case-study banner (W7). **W6 (drift-gate blind spots) is
`[~]`.** Action: complete + merge + deploy (§4).

### D. In-repo (alint) inconsistencies `[ ]`

- `[ ]` **D-b** CHANGELOG: the `root_only` extension sits under **Fixed** (~L247-254),
  not **Added**; a reader scanning "Added" misses it. Re-file or cross-reference.
- `[-]` **D-c** `facts.json` main mixed-state (version 0.13.0 + 11 subcommands). Deferred:
  resolves at the v0.14 version bump; the tag pin keeps it invisible live. Do not touch the
  version field mid-cycle.
- (The v1 draft's D-a "add root_only prose to file_absent/dir_absent NOW" is REMOVED: once
  D1 strips the unreleased Options rows pre-release, those pages show no `root_only` at
  all, which IS consistent. The prose is added at the v0.14 release instead, §6, where it
  needs no sentinel.)

### E. Comprehensiveness gaps `[ ]` / `[-]`

- `[-]` **E1 Baseline mode alint.org docs (deferred to v0.14 ship).** In-repo docs
  complete (`docs/design/baseline.md`, ADR-0006, CHANGELOG). `docs/site/**` has zero
  user-facing baseline content (correct; unreleased). At v0.14: a baseline concept/guide
  page; the `baseline:` key in the configuration reference; an output-formats note (SARIF
  marks-not-removes, JSON `baselined_suppressed`, `--show-baselined`). **CORRECTED
  pre-write guidance:** pages under tag-pinned areas (`docs/site/configuration/`,
  `docs/site/cookbook/`, `docs/site/getting-started/`) are SAFE to pre-write on main; the
  output-formats note lives under `docs/site/reference/**` which IS main-overlaid, so it
  would leak - defer it to release or gate it via §5 P-REF.
- `[ ]` **E2 Kani claim narrowing.** The `roadmap.json` v0.13 blurb and changelog say "a
  Kani-verified path-confinement proof"; post-v0.13 H1 re-scoped the proof to the LEXICAL
  policy only (`post_v0.13_audit.md` H1). At v0.14, narrow the phrasing; consider one
  honest, correctly-scoped formal-methods sentence on the site (do not overclaim). Manual
  one-time fix, not a recurring drift class.
- `[ ]` **E3** Schema-derived Options tables render live but are not headlined as a v0.13
  differentiator. Minor polish.
- `[ ]` **E4** `alint.org/marketing/STATE.md` header stale (v0.9.22 / "70 rule kinds").
  Internal tracking doc; reconcile to v0.13.0 and fold a reconcile step into the release
  checklist (§5 P5).

---

## 3. Decisions (as CORRECTED by the review)

- **Decision 1 - Version-annotate the schema (`x-since`).** Rule options carry an
  introducing-version keyword in `schemas/v1/config.json`; `docs_export` omits any option
  whose `x-since` exceeds the RELEASED version when building the bundle, and strips prose
  blocks marked `<!-- alint:since=X -->` by the same comparison. Corrections from the
  review, all folded into §5 P1: (i) the keyword is **`x-since`**, not `since` (which is
  already a rule-option NAME on 10+ rules); (ii) it is NOT type-derived for the four
  existence kinds (they are not schemars-migrated) - the preferred path is to migrate them
  (which also fills their empty Options descriptions), with a base-schema hand-edit as the
  fast fallback; (iii) the released version is NOT available in the main worktree the
  bridge runs in - it must be passed as an explicit `--released-version` arg sourced from
  the workflow's already-resolved `releases/latest` tag; (iv) the prose sentinel is NEW
  machinery (adapt `elide_internal_blocks` from `roadmap_generator.rs`), not a reuse of
  `alint:ignore-example` (which is a test-only validation-skip). (ADR-0007.)
- **Decision 1b - Tag-pin the arch-model overlay** (added by the review for vector A4). The
  hand-authored LikeC4 model and the generated crate graph cannot carry `x-since` and
  cannot be safely element-stripped (dangling view references), so they are tag-pinned
  rather than main-overlaid. Cost: arch-doc fixes wait for a release; acceptable because
  the model changes rarely. (ADR-0007.)
- **Decision 2 - Fix the bench conflation now; scenario counts are sourced, not
  forced-canonical.** Immediately: remove the `benchmarks.astro:26` catalogue-count
  interpolation from the specific-bench claim (the bug); the recorded scenario count (79)
  stays and is registered as a sourced count (§5 P2 allowlist / scenario source). A full
  per-scenario bench-count CONTRACT (recording resolved+deduped counts into the trajectory
  JSON) is a larger engine task and is OPTIONAL future rigor (§5 P3), not a prerequisite,
  because the trajectory is timing-only for 4 of the page's 9 scenarios.
- **Decision 3 - Maximal-SSOT counts with an EXPLICIT allowlist (no SILENT counts).**
  CORRECTED from "no exemptions" (which the review proved infeasible: competitor counts
  and dated/historical snapshots have no derivable source, and catalogue-vs-scenario counts
  are lexically identical but semantically opposite). Every count that CAN be sourced MUST
  interpolate from a contract (`facts.json`, a new per-ruleset-size field, case-study
  `rules:` frontmatter); every remaining count must appear in an explicit, enumerated,
  justified allowlist (number + `file:line` + reason). The gate fails on any bare
  count-noun integer in scope that is neither an interpolation nor an allowlist entry.
  Scope carves out the sync-generated `src/content/docs/docs/**` (drift-proof by
  construction) and treats `src/content/blog/**` as dated. (ADR-0007.)

---

## 4. Immediate remediation (deployable against v0.13.0)

### alint (engine repo) - the leak root cause

1. `[ ]` **A1/A2/A3 + D1** Implement §5 P1 (the `x-since` field + released-version oracle +
   prose sentinel + filter). This is what removes the live `root_only` rows and the
   zero-width/no_symlinks prose on the next docs-bundle push (the bridge runs main's
   `docs_export`, verified). Regenerate + commit BOTH schema copies (`gen-schema --check`
   byte-compares them; LF already pinned).
2. `[ ]` **A4 + D1b** Tag-pin the arch-model + crate-graph overlay in `docs-bundle.yml`
   (remove the `docs/design/architecture/model` and `crate-graph.md` overlay/bridge steps).
   Removes the live `baseline` element + dev-dep edge on the next push.
3. `[ ]` **D-b** Re-file/cross-reference the `root_only` extension under CHANGELOG Added.

### alint.org (site repo)

4. `[ ]` **Complete + merge + deploy `audit/post-v0.13-drift`** (W1-W5, W7 already green),
   adding items 5-7.
5. `[ ]` **B1** `ls-lint.astro:106` "83-rule catalogue" -> `{alint.ruleKinds}`.
6. `[ ]` **B2** `benchmarks.astro:26` remove the catalogue interpolation from the nixpkgs
   claim; describe it without conflating the catalogue size (the 79 scenario count stays in
   the body, registered per §5 P2).
7. `[ ]` **E4** Reconcile `marketing/STATE.md` to v0.13.0 / 89 / 22 / 30.

Note on sequencing: the leak fixes (items 1-2) are the ONLY way to clear vectors A1-A4 -
there is no clean site-only fix, because the leaked content is in GENERATED/overlaid
artifacts, not hand-written pages. If an immediate stopgap is required before item 1 lands,
temporarily tag-pin the rules-only bridge (revert when P1 ships).

---

## 5. Prevention (make each class structurally impossible)

Each fix ships with a revert-sensitive regression test (repo convention).

### P1. Release-aware rule pages (Decision 1) `[ ]`

- `[ ]` **P1.1 `x-since` field.** Preferred: migrate `file_absent`/`dir_exists`/
  `dir_absent` (and, for symmetry, `file_exists`) to schemars (`#[derive(JsonSchema)]` +
  `options_schema_for!` + a `migrated_option_schemas()` entry), then
  `#[schemars(extend("x-since" = "0.14"))]` on the three siblings' `root_only` field. This
  also discharges the deferred "empty Options descriptions" debt for these kinds. Fallback
  (if the migration is too costly this cycle): hand-edit `schemas/v1/config.json` at the
  three `root_only` subschemas (dir_absent@1021, dir_exists@1074, file_absent@1202; leave
  file_exists@1292) to add `"x-since": "0.14"`, then run `gen-schema` to sync the in-crate
  copy. Either way the field lives in the schema and downstream is identical.
- `[ ]` **P1.2 Released-version oracle.** Add `--released-version <V>` to the `DocsExport`
  clap struct (`main.rs:228-244`); in `docs-bundle.yml:166` pass `${BUNDLE_SOURCE#v}` (the
  `releases/latest` tag the workflow already resolves at `:59-62`). Do NOT use
  `CARGO_PKG_VERSION` (compile-time, reflects the main worktree, flips the moment the
  release-bump commit lands on main before publication) or a main-worktree `facts.json`.
- `[ ]` **P1.3 Filter + prose strip.** In `rule_options_table.rs::options_section` (the row
  loop, ~:94-111) drop any option whose `x-since` exceeds the released version; thread the
  version through `options_section` <- `process_family_h3s` (`docs_export.rs:537-538`) <-
  `generate_rules_pages` (`:410-420`) <- `docs_export`. For prose, add a version-conditional
  paired-sentinel stripper (factor out `elide_internal_blocks` from
  `roadmap_generator.rs:29-30` and gate it on the released version) applied to the
  rules.md body before `emit_rule_page` injects it (`docs_export.rs:867-868`). Wrap the
  `dir_exists` root_only paragraph (rules.md:92) — the clean, block-level case. NOTE (from
  implementation): the `no_zero_width_chars` (U+2060/U+180E) and `no_symlinks` changes are
  mid-SENTENCE detection-scope refinements, not block-wrappable without hiding the whole
  released sentence; they are low-severity (description accuracy, not a config capability)
  and left as an A3 residual for a v0.14 doc pass — the authoritative Options tables are
  already release-gated regardless.
- `[ ]` **P1.4 Regression test (revert-sensitive).** Unit test `options_section`: given a
  branch whose option carries `x-since: "0.14"` and released_version `0.13.0`, assert the
  row is dropped; with released_version `0.14.0`, assert it is kept. Add a sentinel-strip
  unit test (block stripped when released < since, kept otherwise). These fail if P1.3 is
  reverted.

### P-C4. Tag-pin the architecture-model overlay (Decision 1b, vector A4) `[ ]`

- `[ ]` Remove `docs/design/architecture/model` + `crate-graph.md` from the main overlay in
  `docs-bundle.yml` so the bundle uses the tag's arch model. Regression: a test/assertion
  that the bundled `*.c4` / crate-graph match the tag, not main. (The v0.13.0 tag already
  contains the full model, so nothing is lost at the current release.)

### P-REF. Reference-page overlay (vector §1.3) `[ ]`

- `[ ]` `docs/site/reference/**` is main-overlaid and currently guarded only by the
  workflow's "keep to released behaviour" comment (author discipline). Make it mechanical:
  either extend the P1.3 prose-sentinel strip to the reference overlay step, or tag-pin
  `reference/**` (losing fast reference-doc fixes). Recommend the sentinel extension so
  released-reference fixes still flow. This also unblocks pre-writing the baseline
  output-formats note behind a sentinel (E1).

### P2. Maximal-SSOT counts with an explicit allowlist (Decision 3) `[ ]`

- `[ ]` **P2.1 Add the cheap contract source.** Extend `xtask/src/facts.rs::bundled_rulesets`
  (:171-192) to emit a `{ruleset_id: rule_count}` map into `facts.json` (count `- id:` per
  `rulesets/v1/*.yml`). Sources leaf claims like "oss-baseline 15 rules".
- `[ ]` **P2.2 Interpolate what is sourceable**, sweep `src/pages/**`: catalogue counts ->
  `{alint.*}`; per-ruleset sizes -> the new field; case-study per-repo counts -> refactor
  the `headline`/body to interpolate from the `rules:` frontmatter where the number stands
  alone (the `{study.data.rules}` meta cell already does this; the prose copies do not and
  can drift - `nixos-nixpkgs.md` even carries a "may have moved past 79" TODO).
- `[ ]` **P2.3 The explicit allowlist** (`scripts/counts-allowlist.*` or inline): enumerate
  every count that cannot be contract-sourced, each as `number + file:line + reason`.
  Known members (from the review's HARD CASES): competitor counts (Repolinter "~30",
  ls-lint "~5" - no possible source); bench scenario sizes (S1/S2/S4/S5 = 8/8/5/4,
  S3 "~34", S9 "~26" composed+deduped, S6 13 - config-derived, `~` deliberately
  approximate); dated blog snapshots; the `api/rules.json.ts` code-comment "89".
- `[ ]` **P2.4 The gate** (extend `scripts/check-version-pins.sh` or a new
  `scripts/check-counts.mjs`): fail on any bare integer adjacent to a
  rule/ruleset/family/format noun in `src/pages/**` (and case-study frontmatter-render
  sites) that is neither a `{...}`/`{{...}}` interpolation nor an allowlist entry. CARVE
  OUT `src/content/docs/docs/**` (sync-generated, drift-proof) and treat
  `src/content/blog/**` as dated. Run in the PR gate, the daily cron, AND at deploy time
  (closes W6's "deploy.yml does not run the gates"). Revert-sensitive test: a fixture page
  with a bare "83-rule catalogue" must fail; "79-rule pass" with an allowlist entry must
  pass.

### P3. Bench-count contract (OPTIONAL future rigor; Decision 2) `[-]`

- `[-]` Only if the scenario counts are wanted as live contract data rather than allowlist
  entries: at a v0.14 bench cycle, have the harness RESOLVE each scenario's `extends:` +
  dedup + count, bump `benchmarks-trajectory.json`'s `schema_version` to add a per-scenario
  rule-count, expand the trajectory beyond its current 4 headline cells to cover S1/S2/S4/S5,
  update `render-history.py` (:442-462) + `benchmarks.astro` to interpolate. Deferred because
  it is new-data engine work spanning the harness, the producer, and the page - the
  allowlist (P2.3) covers these honestly in the meantime.

### P4. Complete the site drift-gate blind spots (W6) `[ ]`

- `[ ]` **P4.1** Add the count anchors the gate lacks: families, case-study count, examples
  count (all already contract-interpolated).
- `[ ]` **P4.2** Gate the `/api/{rules,rulesets,versions}.json` endpoints for source/URI
  resolvability (the W3/W4 404 class), so a new aliased/nested kind cannot silently 404.
- `[ ]` **P4.3** Run the count/link/pin gates at deploy time in `deploy.yml`, not only
  PR + cron.

### P5. Release-checklist + tracking hygiene `[ ]`

- `[ ]` **P5.1** In `RELEASING.md`: (i) confirm the `x-since` filter + count gate are green;
  (ii) reconcile `marketing/STATE.md`; (iii) narrow any claim whose scope changed
  (the Kani/E2 pattern); (iv) add the newly-released options' prose (e.g. root_only for
  file_absent/dir_absent, §6).
- `[ ]` **P5.2** Reconcile `STATE.md` at every release cut (folds into P5.1) so it never
  drifts seven weeks again.

---

## 6. Release-gated v0.14 work (deferred, tracked)

- `[-]` **E1 Baseline site docs.** Pre-write the guide + `baseline:` configuration-reference
  entry NOW (tag-pinned areas, safe); the output-formats note waits for release OR lands
  behind a §5 P-REF sentinel.
- `[-]` **root_only prose** for `file_absent`/`dir_absent` (and keep `dir_exists`): add at
  release, unsentineled (released then). Pre-release consistency is already achieved by D1
  stripping the Options rows.
- `[-]` **E2** Narrow the Kani claim to the lexical policy in roadmap/changelog phrasing.
- `[-]` **Pin bump** (four install-pins + prose claims) at the cut.

---

## 7. Sequencing

1. **Engine leak fixes (P1 + P-C4)** first: they clear vectors A1-A4 and propagate via the
   docs-bundle push (no release needed; verified the bridge runs main's `docs_export`).
2. **alint.org immediate (§4 items 4-7)** in parallel: independent hand-written fixes on the
   existing `audit/post-v0.13-drift` branch.
3. **Prevention P2 (counts) + P4 (W6) + P-REF**: the sourcing, the allowlist, the gate,
   the reference-overlay hardening. The count gate goes advisory first, blocking once the
   sweep + allowlist are complete.
4. **P3 (optional) and §6** at the v0.14 cut.

## 8. Open questions

- **P1.1 route** (migrate the existence kinds to schemars vs hand-edit the base schema):
  resolvable at implementation; the ADR is agnostic to which populates the `x-since`
  keyword. Recommend migration (also fixes empty descriptions) unless it proves costly.
- **P-REF** (extend the sentinel to reference vs tag-pin reference): recommend the sentinel
  so released-reference fixes keep flowing.

## 9. Adversarial-review corrections (v1 draft -> this revision)

1. **"Single structural leak vector" was wrong.** Three vectors: rule pages, the LikeC4
   model (A4, confirmed `f_baseline` leak), and `docs/site/reference/**`. §1, §2.A4, P-C4,
   P-REF added.
2. **Decision 1's mechanism was misstated.** `since` collides with an existing option name
   (-> `x-since`); it is not type-derived for the target kinds (not schemars-migrated); the
   version oracle is not available in the bridge's main worktree (-> explicit
   `--released-version`); the prose sentinel is new machinery, not `alint:ignore-example`
   reuse. §3 D1, P1.1-P1.3.
3. **Decision 3's "no exemptions" is infeasible.** Competitor + dated/historical counts have
   no source; catalogue-vs-scenario counts are lexically identical, semantically opposite.
   Reframed to "no SILENT counts + explicit enumerated allowlist," with the sync-docs and
   blog subtrees carved out. §3 D3, P2.
4. **"docs/site is tag-pinned" is false for `reference/**`** (and `architecture-diagrams.md`,
   `how-it-works.md`). Corrected E1's pre-write guidance; added P-REF.
5. **P3 was under-scoped.** `benchmarks-trajectory.json` is timing-only for 4 of the page's
   9 scenarios; per-scenario counts need `extends:` resolution + dedup. Downgraded to
   optional; the allowlist covers the scenario counts now.
6. **D-a removed** (adding unreleased prose that would itself need a sentinel); D1 stripping
   already yields pre-release consistency, and the prose is added at release (§6).

## 10. Execution status (branch `v0.14-doc-drift`)

- `[x]` **P-C4** LANDED — `docs-bundle.yml` no longer overlays the LikeC4 model / crate-graph
  from main; they are tag-pinned. Clears A4 (the `f_baseline` diagram leak) on the next
  docs-bundle push. (YAML validated; the tag's docs-export already emits both artifacts.)
- `[x]` **P1** LANDED — the `x-since` schema keyword + `--released-version` +
  `options_section` filter + the `<!-- alint:since=X -->` prose stripper. Clears A1/A2
  (root_only Options rows + the `dir_exists` prose) for `file_absent`/`dir_exists`/`dir_absent`
  while leaving released `file_exists` untouched. Verified end-to-end (export at 0.13.0 vs
  0.14.0 vs local), revert-sensitive unit tests, gen-schema/docs-export gates, dogfood,
  clippy, fmt all green. A3 zero-width/no_symlinks residual noted in P1.3.
- `[x]` **alint.org immediate fixes (§4)** LANDED on branch `audit/post-v0.13-drift` (local):
  **B1** ls-lint `83`->`{alint.ruleKinds}`, **B2** benchmarks meta deconflated, **E4** STATE.md
  reconciled (atop the prior session's W1-W7).
- `[x]` **P2.1** LANDED (engine) — `facts.json` gains `bundled_ruleset_sizes`
  (oss-baseline == 15, test-anchored); the per-ruleset contract source. gen-facts --check green.
- `[x]` **P2.3/P2.4** LANDED (alint.org) — `scripts/check-counts.mjs`: a hardcoded
  rule/ruleset/family/format count must be a `{alint.*}` interpolation or an explicit
  allowlist entry (10 scoped counts enumerated with reasons). `--self-test` + a negative
  test prove it catches the B1 drift; wired into `check-pins.yml` (PR + push + cron).
- `[x]` **P2.2** captured — the sweep confirmed catalogue counts are already interpolated;
  the residual scoped counts are allowlisted (bench/competitor permanent; oss-baseline +
  case-study are sourced follow-ups: interpolate oss-baseline once P2.1's field syncs to
  alint.org, and the case-study index cards from their `rules:` frontmatter).
- `[x]` **P4.1/P4.3** LANDED (alint.org) — count gate extended to case-study counts; run as a
  pre-flight in `deploy.yml` so a count-drift aborts the deploy. **P4.2 DEFERRED** (external
  `source_url` resolvability needs network-flaky GitHub probing; internal `docs_url` is already
  covered by `check-internal-links.mjs`).
- `[x]` **P-REF** LANDED (engine) — `copy_site_tree` strips `<!-- alint:since=X -->` blocks from
  the main-overlaid `docs/site/reference/**` when `--released-version` is set; docs-bundle passes
  it. Revert-sensitive test. Closes the third leak vector mechanically + unblocks E1's pre-write.
- `[x]` **P5 + D-b** LANDED (engine) — RELEASING.md gains a per-release doc-drift checklist
  (unwrap x-since options/prose, narrow walked-back claims, write feature site docs, reconcile
  pins + STATE.md, confirm gates). D-b: the `root_only` CHANGELOG entry stays correctly under
  Fixed; the checklist gives it release-time visibility.
- `[x]` **Existence kinds migrated to schemars + `x-since` type-derived (#118, merged).**
  Replaces the P1.1 base-schema hand-edit route with `#[derive(JsonSchema)]` +
  `#[schemars(extend("x-since"="0.14"))]`. `git_tracked_only` and the `file_exists`-only
  `respect_gitignore` become kind-specific `Options` (ADR-0008), closing the last
  RuleSpec-vs-schema divergence and filling the four kinds' empty option descriptions. An
  adversarial pre-merge review caught + fixed a `respect_gitignore`-in-`rule_common`
  fail-quietly (a `no_bom: {respect_gitignore: false}` that validated, loaded, and no-op'd).
- `[x]` **A3 residual CLOSED (1c).** `no_zero_width_chars` U+2060/U+180E split into an
  `alint:since=0.14` block (the released U+200B/C/D sentence stays); the `no_symlinks`
  escaping-symlink caveat wrapped whole. Both stripped at 0.13.0, shown at 0.14.0 (verified
  both directions).
- `[x]` **root_only prose for `file_absent`/`dir_absent` (1b) — sentinel-gated NOW** (not
  deferred to §6): each gains an `alint:since=0.14` paragraph mirroring `dir_exists`; behaviour
  verified honored (build + evaluate), stripped pre-release.
- `[x]` **E1 baseline site docs PRE-WRITTEN (1d).** A `concepts/baseline.md` guide + the
  `baseline:` configuration-reference key (both tag-pinned, safe on main) + a sentinel-wrapped
  output-formats note (reference/** is main-overlaid, so P-REF strips it pre-release). Nav
  wiring + un-sentineling happen at the v0.14 cut.
- `[x]` **2a — API `docs_url` gate LANDED (alint.org #15) + caught a live bug.**
  `check-internal-links.mjs` now also scans the prerendered `/api/*.json` endpoints'
  internal `docs_url` (the HTML walk never saw them). It immediately flagged the 10
  short-name aliases + `cross_file_value_equals`, whose `docs_url` 404 **live**
  (`/docs/rules/content/content_matches/` → 404; aliases have no page of their own).
  Fixed `docsUrlOf` to resolve aliases to the canonical kind's page; a fresh build has
  100 API links all resolving. External `source_url` probing stays the deferred P4.2 tail.
- `[-]` **2b — case-study index-card count interpolation: DEFERRED with rationale.**
  Infeasible as specified. The card counts (golang/go "31 rules") are curated *narrative*
  subsets that diverge ~2x from the study `rules:` frontmatter (64 = the effective total
  incl. `extends:`) AND the authored `- id:` count (29); interpolating from `rules:` would
  display the wrong number. Corrected the misleading `check-counts` allowlist reason; the
  literal stays allowlisted.
- `[ ]` **NEW finding — systemic case-study count divergence.** The `rules:` frontmatter
  (rendered in each study's meta cell) runs ~2x the authored `- id:` count (golang-go 64/29,
  kubernetes 49/25, turbo 88/34, tokio 74/31). It appears to mean "effective total incl.
  `extends:`" but is unverified per-study. Making it a trustworthy contract needs an
  extends-resolution + dedup pass (the P3 class); until then the meta cells + card narratives
  are hand-curated. Tracked, not urgent.
- `[x]` **3 — housekeeping DONE.** `.github-account=asamarts` marker added to alint.org (the
  alint repo already had a tracked one); the work-stream branches (git-tracked-kind-option,
  v0.14-doc-followups, v0.14-doc-drift) deleted in both repos.
- `[ ]` **True remainder:** P3 (optional bench-count contract — now also subsumes the
  case-study count reconciler above), P4.2's EXTERNAL `source_url` probe, and the §6
  release-cut tail (Kani-claim narrowing, nav wiring + un-sentinel the pre-written v0.14
  prose, pin bump).
- **Deployment:** the engine drift-prevention (P1/P-C4/P-REF/P2.1/P5) and the alint.org
  immediate fixes (§4) merged in the prior session; this session merged #118 (ADR-0008),
  #119 (1b/1c/1d), and alint.org #15 (2a + alias fix). The alias-`docs_url` fix is live on
  alint.org's `/api/rules.json`; the next `docs-bundle.yml` push carries the description/strip
  fixes live (the binary validation-tightening ships at the v0.14 cut).
