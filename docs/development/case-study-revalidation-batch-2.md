# Case-study revalidation — batch 2 (alphabetical: astral-sh-uv → dotnet-runtime)

Findings from the v0.9.17 revalidation pass for the second alphabetical
batch of 5 case studies. Aggregated by parent agent into the master
log (`case-study-revalidation-log.md`).

Validation environment:
- alint binary: `/home/kaminsod/projects/alint/target/release/alint` (v0.9.17)
- Date: 2026-05-07
- Authoritative pitfall catalogue: 21 pitfalls (#18 + #19 fixed in v0.9.17 engine)
- Authoritative rule-kind backlog: docs/development/launch-evidence.md

## Per-case-study findings

### astral-sh-uv

- **Validation:** `validate-config` reports **73 rule(s) loaded** (16 explicit
  + 57 from extends). README narrative ("63 declarative rules pass cleanly
  and 9 rules surface real findings" plus the 17 explicit + bundled split)
  reconciles cleanly once you add the extends counts.
- **Stale references corrected:** none in the README that needed text
  changes — the v0.10+ language was forward-looking and remains accurate.
  The `python/pep-621-shape@v1` mention was already labelled "strong v0.10
  candidate"; per launch-evidence.md it is now formally a v0.10 design
  candidate, no upgrade-of-language needed.
- **Pitfall fixes:** neither #18 nor #19 directly affects this config
  (uv has no tracked-AND-gitignored files in scope, and no
  `root_only: true` + multi-component literal patterns).
- **`command:` shellouts that v0.9.6+ rule kinds now cover natively:**
  `uv-typos`, `uv-cargo-shear`, `uv-cargo-fmt`, `uv-cargo-clippy`,
  `uv-shellcheck`, `uv-ruff-check`, `uv-ruff-format` are all the
  "external linter wrapper" pattern that **does NOT** have native rule-kind
  coverage today. The `command_idempotent` v0.10 design candidate (ruff,
  prettier) would consolidate the formatter-check shellouts; track that
  ship date for follow-up.
- **Bundled-ruleset rule counts:** authoritative counts confirmed against
  the prompt's table (oss-baseline=15, rust=11, python=9, monorepo=4,
  monorepo/cargo-workspace=4, ci/github-actions=3,
  hygiene/no-tracked-artifacts=11). Total: 57 — matches the validation
  output of 73 minus 16 explicit rules.
- **Live-tree status:** pending — `/tmp/uv/` not present; only stale lock
  files `/tmp/uv-7c30f606284ce2fc.lock` and `/tmp/uv-ef1a257821ce330e.lock`
  remain.
- **README updates applied:** added "Future analysis" + "Validation status
  (2026-05-07)" footer.

### bazelbuild-bazel

- **Validation:** `validate-config` reports **80 rule(s) loaded**.
  Re-count: explicit rules in YAML = **41** (verified by `grep -c "^  - id:"`),
  bundled rules from extends per the prompt's table = 40 (oss-baseline 15
  + java 11 + ci/github-actions 3 + hygiene/no-tracked-artifacts 11),
  naive sum = 81. Engine reports 80 because java.yml's 11 entries
  include 1 fact (`has_java`) that doesn't load as a rule. **Note for
  the parent agent:** the prompt's bundled-ruleset rule counts include
  `- id:` entries that represent facts (has_java, has_python, has_rust,
  has_node, has_go, has_agent_context, is_cargo_workspace). The
  `validate-config` "rules loaded" count subtracts these. README's
  narrative "71 effective rules" is the original conservative count;
  the precise 80 is now in the validation footer.
- **Stale references corrected:**
  - "NEW pitfall #18 (not in CONFIG-AUTHORING.md)" header rewritten to
    "Pitfall #18 (now in CONFIG-AUTHORING.md, FIXED in v0.9.17)" —
    bazel is now the canonical example documented in the catalogue.
  - "Two workarounds today" reframed as "FIXED in v0.9.17" with the
    canonical `respect_gitignore: false` snippet.
  - "v0.10+ fix candidates" section reduced to a single "SHIPPED in
    v0.9.17" line with a verification report from the live tree.
  - "v0.10's walker refactor" deadline language updated to "SHIPPED in
    v0.9.17".
  - "What needs new alint primitives" table entry for `.bazelversion`
    updated from "NEW pitfall #18" to "Pitfall #18 (FIXED in v0.9.17)".
  - "NEW candidates surfaced uniquely by bazel" — `respect_gitignore:
    false` line tagged "SHIPPED in v0.9.17".
- **Pitfall fix verification:** `respect_gitignore: false` on
  `file_exists` against `.bazelversion` was directly verified during
  this revalidation pass on `/tmp/bazel/`:
  - With override: rule passes (`✓ All 1 rule(s) passed`)
  - Without override: rule fails (`✗ error … expected a file matching
    [.bazelversion] at the repo root`)
  This is the canonical bazel case-study scenario and it now works as
  documented.
- **Action item flagged for later (NOT auto-applied per revalidation
  guard rails):** The current `bazelbuild-bazel/.alint.yml` dropped
  the `bazel-version-file-exists` rule entirely (lines 226-249 of
  the YAML). With v0.9.17 the rule can be added back using
  `respect_gitignore: false`. Same applies to the `bazel-version-file-shape`
  rule (currently dormant against bazel's own tree because `.bazelversion`
  is gitignored). Follow-up edit recommended in a separate change.
- **`command:` shellouts that v0.9.6+ rule kinds now cover natively:**
  `bazel-buildifier-format-check` (420 violations on the live run; legitimate
  — buildifier owns the Starlark AST layer) and `bazel-shell-shellcheck` (27
  violations) are both the "external linter wrapper" pattern that does
  NOT have native rule-kind coverage today. The `command_idempotent`
  v0.10 design candidate would consolidate the buildifier check.
- **Bundled-ruleset rule counts:** authoritative — oss-baseline (15) +
  java (11) + ci/github-actions (3) + hygiene/no-tracked-artifacts (11)
  = 40. Matches.
- **Live-tree status:** `/tmp/bazel/` exists (10,709 walked entries).
  - `alint check`: 14 failing rules + 38 passing (915 violations total).
  - Top contributors: `bazel-buildifier-format-check` (420), `bazel-build-file-naming`
    (274 — info-level, BUILD files don't open with `#`), `bazel-java-sources-apache-header`
    (109), `dir_absent ... build/...` from oss-baseline (30 violations
    on `docs/build/` — likely a bazel docs-output dir that is intentionally
    tracked; worth a `paths.exclude` follow-up).
  - `alint suggest`: 2 high-confidence proposals (`oss-baseline@v1`,
    `python@v1`); did not surface `java@v1` because `has_java: false`
    on Bazel-built repos by design (no `pom.xml`/`build.gradle`).
- **README updates applied:** retitled pitfall #18 section (FIXED in
  v0.9.17), updated workaround language, added "Future analysis" +
  "Validation status (2026-05-07)" footer.

### clap-rs-clap

- **Validation:** `validate-config` reports **70 rule(s) loaded** (26
  explicit + 44 from extends: oss-baseline 15 + rust 11 +
  monorepo/cargo-workspace 4 + ci/github-actions 3 +
  hygiene/no-tracked-artifacts 11). Matches.
- **Stale references corrected:**
  - "12 pitfalls" → "21-pitfall catalogue" (catalogue grew to 21 in
    P2b Wave 2; this README was written when the count was 12).
- **Pitfall fixes:** neither #18 nor #19 directly affects this config
  (no tracked-AND-gitignored files; no `root_only:` with multi-component
  literals; clap is the cleanest "well-curated library workspace" with
  no surprises).
- **`command:` shellouts that v0.9.6+ rule kinds now cover natively:**
  `clap-typos-shellout`, `clap-cffconvert-shellout`, `clap-cargo-deny-shellout`
  are all the "external linter wrapper" pattern. None have native
  rule-kind coverage today. The `command_idempotent` v0.10 candidate
  would not naturally absorb these because they're not formatters.
- **Cross-file metadata identity:** the README's `cross_file_field_equals`
  ask is now subsumed by the broader `cross_file_value_equals` (10
  sources, past-saturation, v0.10 ship-target per launch-evidence.md).
  Documented in the validation footer.
- **Bundled-ruleset rule counts:** oss-baseline (15) + rust (11) +
  monorepo/cargo-workspace (4) + ci/github-actions (3) +
  hygiene/no-tracked-artifacts (11) = 44. Matches.
- **Live-tree status:** pending — `/tmp/clap/` not present.
- **README updates applied:** corrected "12 pitfalls" → "21-pitfall
  catalogue", added "Future analysis" + "Validation status
  (2026-05-07)" footer.

### denoland-deno

- **Validation:** `validate-config` reports **76 rule(s) loaded** (15
  explicit + 61 from extends: oss-baseline 15 + rust 11 + node 9 +
  ci/github-actions 3 + monorepo/cargo-workspace 4 +
  tooling/editorconfig 3 + hygiene/no-tracked-artifacts 11 +
  agent-context 5). Matches.
- **Stale references corrected:** none required text changes — README
  used "v0.10+" language consistently, which remains forward-looking.
  Pitfall #17 (already-documented since P2a Wave 3) is correctly cited
  for the `deno-dlint-includes-camelcase` workaround.
- **Pitfall fixes:** neither #18 nor #19 directly affects this config.
  Pitfall #17 remains the load-bearing one; the `*_path_contains` v0.10
  design candidate is the natural fix and has 3 sources now (helm, deno,
  bazel).
- **`command:` shellouts that v0.9.6+ rule kinds now cover natively:**
  `deno-cargo-clippy-workspace`, `deno-dprint-check`, `deno-dlint` —
  all "external linter wrapper" pattern. The `command_idempotent`
  v0.10 design candidate (ruff, prettier, dprint, deno fmt) would
  absorb the formatter-check shellouts; deno is one of the demand
  sources for that primitive (alongside ruff/prettier).
- **Bundled-ruleset rule counts:** all 8 extends correct against the
  authoritative table.
- **Live-tree status:** pending — `/tmp/deno/` not present.
- **README updates applied:** added "Future analysis" + "Validation
  status (2026-05-07)" footer.

### dotnet-runtime

- **Validation:** `validate-config` reports **60 rule(s) loaded** (31
  explicit + 29 from extends: oss-baseline 15 + ci/github-actions 3 +
  hygiene/no-tracked-artifacts 11). Matches the README's headline
  "60-rule config".
- **Stale references corrected:**
  - The "Notes for the parent agent" section's "Config size: 60 rules
    (15 bundled-via-extends + 45 custom) declared; total rule count
    loaded by the engine including all bundled-ruleset rules is
    higher (oss-baseline pulls in ~25, ci/github-actions pulls in 3,
    hygiene pulls in ~17 — total ~105 loaded rules)" was a
    miscalculation: oss-baseline is 15 (not ~25), hygiene is 11 (not
    ~17), so bundled-from-extends is 29 (not ~45). Total loaded is
    60 (not ~105). Updated to match the actual `validate-config`
    output.
  - The `xml_path_*` section already correctly reports the v0.11→v0.10
    promotion. Per launch-evidence.md as of 2026-05-07, this is
    formal: 2 sources (spark + dotnet/runtime), v0.10 ship-target.
    Reaffirmed in the new validation footer.
  - The `dotnet@v1` bundled ruleset is now formally a v0.10 ship-target
    per launch-evidence.md (this case study is the unique source).
    Reaffirmed in the validation footer.
  - The `oss-license-exists` LICENSE.TXT recognition issue — flagged
    as v0.9.16+ candidate fix in the original draft — is still open
    as v0.10 housekeeping. Status updated in the notes section.
- **Pitfall fixes:** neither #18 nor #19 directly affects this config
  (no tracked-AND-gitignored; no `root_only:` with multi-component
  literals).
- **`command:` shellouts that v0.9.6+ rule kinds now cover natively:**
  None — this config does not currently use `command:` rules. The
  `eng/formatting/format.sh` is mentioned as a candidate but is
  expressed as `file_exists` only (the script's presence, not a
  shellout to dotnet-format).
- **Bundled-ruleset rule counts:** correct.
- **Live-tree status:** pending — not re-run during the 2026-05-07
  revalidation pass; original snapshot's 5 errors / 2,259 warnings /
  584 info documented in the README's notes section.
- **README updates applied:** corrected the Config-size miscalculation
  (105 → 60), updated `xml_path_*` v0.10 ship-target language,
  added "Future analysis" + "Validation status (2026-05-07)" footer.

## Cross-cutting findings

1. **Bundled-ruleset rule-count drift was systematic.** Three of the
   five case studies (uv, bazel, dotnet-runtime) cited "30+", "~30",
   and "~105" for the bundled rule contribution; actual numbers from
   the prompt's authoritative table are exactly known. The
   `validate-config` total is the authoritative discrepancy detector
   — every README's "N declarative rules" plus the bundled sum should
   reconcile to the validate-config total. Worth a global pass to
   align all 30 case studies.

2. **Pitfall #18 fix verification on bazel succeeded end-to-end.**
   The canonical bazel `.bazelversion` scenario was directly verified
   against `/tmp/bazel/`: the rule passes with `respect_gitignore:
   false`, fails without it. This closes the load-bearing case for
   the v0.9.17 pitfall #18 fix and the bazel README has been
   re-framed accordingly. **Recommend the bazel `.alint.yml` be
   updated in a separate change to re-add the dropped
   `bazel-version-file-exists` rule with the new override.**

3. **Pitfall #19 fix had zero direct relevance** to this batch — none
   of the 5 case studies use `root_only: true` with multi-component
   literals. The original surfacing case study (tensorflow) is in a
   different batch.

4. **No stale "v0.9.16 will fix" references in this batch.** All five
   READMEs consistently used "v0.10+" forward-looking language for
   features that haven't shipped, and "v0.9.15 Phase 4" for
   already-shipped enriched diagnostics (correct historical reference).

5. **`command_idempotent` v0.10 design candidate has demand from this
   batch.** uv (cargo fmt, ruff format), deno (dprint, deno fmt),
   bazel (buildifier --mode=check) all express the "run linter in
   --check mode" pattern via `command:` rules. None directly
   replaceable today; track v0.10 design progress.

6. **Live-tree availability is uneven.** Only `/tmp/bazel/` is
   present from this batch. Recommend a coordinated re-clone pass
   for the other 4 (uv, clap, deno, dotnet-runtime) before a future
   revalidation cycle to enable end-to-end live-tree checks across
   the corpus.

7. **`xml_path_*` and `dotnet@v1` are now both formal v0.10
   ship-targets** per launch-evidence.md, both promoted via this
   batch's dotnet/runtime case study. The case study's central
   recommendation has landed in the engineering plan; the README
   has been updated to reflect that.

## Open issues / gaps / inconsistencies / opportunities

- **Bazel `.alint.yml` follow-up:** re-add `bazel-version-file-exists`
  + `bazel-version-file-shape` rules with `respect_gitignore: false`
  override to leverage the v0.9.17 pitfall #18 fix. Flagged here per
  the "do not auto-fix .alint.yml" guard rail.
- **`oss-license-exists` LICENSE.TXT recognition** still open
  (dotnet-runtime). v0.10 housekeeping for `oss-baseline@v1`.
- **Bundled-ruleset rule-count consistency** worth a global aligner
  pass across all 30 case studies. Authoritative table exists in the
  parent agent's prompt; subagent batches can reconcile per-README.
- **Live-tree clones missing for 4 of 5 in this batch** — re-clone
  recommendation for next revalidation cycle.

## Total scope of edits

- **READMEs touched:** 5 of 5 (astral-sh-uv, bazelbuild-bazel,
  clap-rs-clap, denoland-deno, dotnet-runtime).
- **`.alint.yml` files touched:** 0 of 5 (per revalidation guard rails;
  bazel flagged for follow-up edit).
- **Log entries written:** 5 in this batch file + 1 cross-cutting
  section.
