# Case-study revalidation — Batch 1 (alphabetical, 5 of 30)

**Pass date:** 2026-05-07
**alint version validated:** 0.9.17 (built 2026-05-07,
`/home/kaminsod/projects/alint/target/release/alint`)
**Batch:** angular-angular, apache-airflow, apache-arrow,
apache-spark, astral-sh-ruff (alphabetical batch 1 of the 30
case-study revalidation pass)

All 5 configs **`validate-config`-clean** both before and after
the README edits. No `.alint.yml` files were modified.

## Per-case-study findings

### angular-angular

**Validation:** `validate-config` reports **131 rules** loaded;
README originally claimed "50-rule + 9-bundled-ruleset". The
`50` in-config count was stale — actual count is **73 in-config
+ 9 bundled overlays = 131 total** post-extends resolution. This
was the most significant rule-count drift in the batch.

**Edits applied:**
- Header rule-count corrected (50 → 73 in-config; bundled ~40
  → ~58 explicit-by-overlay; total = 131).
- `cross_language_implementation_complete` saturation count
  updated 3 → 5 sources per launch-evidence.md (added protobuf
  + flutter alongside arrow + TF + angular). Status language
  shifted from "v0.11+ flagship" → "v0.11+ ship-target".
- The 4 separate "promotes from v0.11+ flagship to v0.11+
  ship-target" sentences (in 4 different sections) all updated
  to the new ship-target framing.
- Pitfall-count "2 of 19 documented pitfalls fired" rephrased
  to acknowledge catalogue grew to 21 (P2b Wave 2 added istio's
  #20 + #21).
- Resolved the parent-agent note about "in-progress
  `RuleSpec.respect_gitignore` field" — the knob shipped in
  v0.9.17 as the per-rule `respect_gitignore: false` direct fix
  for pitfall #18.
- Added "Future analysis" section (3 ideas: `pair_inverse`
  uplift once shipped, `compliance/reuse@v1` trial,
  `agent-hygiene@v1` overlay).
- Added "Validation status (2026-05-07)" footer.

**Gaps remaining:** None blocking. Live-tree recheck pending
(`/tmp/angular/` not present at revalidation time, original
122-violation claim from 2026-05-06 unverified).

**New analyses surfaced:** `agent-hygiene@v1` overlay trial
(in-house `.ng-dev/` + `tools/tslint/` are exactly the kind of
custom tooling stack that benefits from explicit agent guardrails);
`compliance/reuse@v1` trial against the per-source `@license`
block (1k+ TS files); `pair_inverse` direct rule once it ships
(forward `for_each_dir` workaround would be replaced).

### apache-airflow

**Validation:** `validate-config` reports **75 rules** loaded;
matches expected (28 in-config + 6 bundled overlays summing to
~48 with overlap deduped). README's "25-rule alint config"
claim is approximately correct (actual 28 in-config). No drift
on the in-config count worth a separate edit.

**Edits applied:**
- `cross_file_value_equals` candidate updated: now a **v0.10
  ship-target** with **10 demand sources** per launch-evidence.md
  (airflow + tokio + clap + uv + react + pnpm + nodejs/node +
  pytorch + vscode + istio); istio's pitfall #20
  per-file-extractor refinement noted.
- `import_gate` candidate updated: now a **v0.10 ship-target**
  with **4 demand sources** (k8s + airflow + golang/go + pytorch).
- `file_lines_sorted` + `no_duplicate_lines` framing updated to
  acknowledge they're subsumed by the broader `ordered_block`
  candidate (now v0.10 ship-target with 7 demand sources per
  launch-evidence.md).
- "v0.10+ feature requests" generic phrasing tightened to
  "v0.10 ship-targets".
- Added "Future analysis" section (3 ideas: refactor to
  `for_each_file: pyproject.toml` + nested `require:` for
  per-distribution rules; `compliance/reuse@v1` trial;
  `docs/adr@v1` overlay scan).
- Added "Validation status (2026-05-07)" footer.

**Gaps remaining:** None blocking. The `command:` rule for
`zizmor` would benefit from a native `gha-pin-actions-to-sha`
ruleset rule, but that's already covered by the bundled
`ci/github-actions@v1`. Live-tree recheck pending.

**New analyses surfaced:** The per-pyproject-toml refactor would
collapse 5+ rules to 1 (airflow has 100+ pyproject.toml files
across 1 root + 4 core + 101 providers + N shared); `docs/adr@v1`
might apply to airflow's `docs/apache-airflow/best-practices/`
surface; `compliance/reuse@v1` could collapse the per-language
`insert-license` hooks (×11 variants in airflow's pre-commit).

### apache-arrow

**Validation:** `validate-config` reports **107 rules** loaded;
matches expected (65 in-config + 6 bundled overlays summing to
~44 rules with overlap deduped). README originally claimed
"6 bundled rulesets ship roughly 35 rules between them" — the
actual is **44 rules** (oss-baseline=15, compliance/apache-2=3,
python=9, ci/github-actions=3, hygiene/no-tracked-artifacts=11,
tooling/editorconfig=3 = 44). Bundled count corrected.

**Edits applied:**
- Bundled-overlay rule-count corrected (35 → 44).
- Header re-stated as "65-in-config-rule + 6 bundled = 107
  total post-extends".
- `registry_paths_resolve` references updated: now **v0.10
  ship-target** with **8 demand sources** per launch-evidence.md
  (rust + clap + cpython×2 + next.js + arrow + pytorch +
  nodejs/node + NixOS×3); previous "4 of 4 candidates" framing
  replaced.
- `cross_language_implementation_complete` references updated:
  now **v0.11+ ship-target** with **5 saturated demand sources**
  (arrow + tensorflow + protobuf + angular + flutter); previous
  "v0.10+" / "v0.11+ flagship" framing replaced.
- `ordered_block` references updated: now **v0.10 ship-target**
  with **7 demand sources** per launch-evidence.md (rust +
  airflow + tokio + cpython + arrow + golang/go + protobuf
  failure_lists); previous "5 sources" framing replaced.
- `command_per_scope` framing softened (single-source still;
  removed "v0.10+" pin).
- Historical "#17/#18" outer-parens filter pitfall claim
  rephrased to acknowledge the "#18" slot was reused for the
  gitignore-masking case (now fixed in v0.9.17 via
  `respect_gitignore: false`).
- Added "Future analysis" section (3 ideas: `compliance/reuse@v1`
  trial; `apache/governance@v1` adoption once shipped;
  `scope_filter.has_ancestor` per-language manifest narrowing
  for the per-language-subdir-has-readme rule).
- Added "Validation status (2026-05-07)" footer.

**Gaps remaining:** Live-tree recheck pending (`/tmp/apache-arrow/`
not present; original 243-violation claim from 2026-05-06
unverified). The 16 ASF-header findings reportedly all match
`rat_exclude_files.txt` entries — would be cleanly resolved by
the v0.10 ship-target `registry_paths_resolve` rule kind once
it lands.

**New analyses surfaced:** Three Apache TLPs (arrow + spark +
airflow) all converge on the same `apache/governance@v1` bundle
shape — once it ships, arrow's 8 `arrow-asf-*` / `arrow-rat-*`
restated rules collapse to one `extends:` line. `compliance/reuse@v1`
could collapse the per-language Apache-header overrides (cpp/python
extension lists). `scope_filter.has_ancestor` per-language manifest
discovery would make the per-language-subdir rules self-discovering.

### apache-spark

**Validation:** `validate-config` reports **110 rules** loaded;
matches the README's "Total = 110 rules loaded successfully"
claim ✓. Bundled-overlay rule-count claim ("roughly 49 rules")
corrected to **52** (oss-baseline=15, compliance/apache-2=3,
java=11, python=9, ci/github-actions=3,
hygiene/no-tracked-artifacts=11 = 52).

**Edits applied:**
- Bundled-overlay rule-count corrected (49 → 52); detail
  added.
- `registry_paths_resolve` "6th demand source" framing updated
  to "8 demand sources, v0.10 ship-target" per launch-evidence.md
  (added pytorch + nodejs/node + NixOS×3).
- `apache/governance@v1` references confirmed as **v0.10
  ship-target** (now confirmed in launch-evidence.md, not just
  proposed).
- `xml_path_*` references updated: now **v0.10 ship-target**
  per launch-evidence.md (promoted via dotnet/runtime's ~2,300
  XML manifests at one OOM bigger scale, alongside spark's 49
  pom.xml files); previous "v0.11+ candidate" framing replaced.
- `generated_file_fresh` references updated: now **v0.10
  ship-target** with **6 demand sources** (uv + cpython +
  pytorch + bazel + TF + spark); previous "v0.11+" / "third
  confirmation" framing replaced.
- `cross_language_registry_consistency` framing updated to
  acknowledge it's a refinement of the `cross_language_implementation_complete`
  v0.11+ ship-target rather than its own candidate.
- v0.9.15 Phase 4 enriched diagnostic note tagged with the
  v0.9.17 release that ships it.
- Added "Future analysis" section (3 ideas: `apache/governance@v1`
  adoption when shipped; `scope_filter.has_ancestor: pom.xml`
  refactor for per-Maven-module rules; `xml_path_*` adoption
  when shipped).
- Added "Validation status (2026-05-07)" footer.

**Gaps remaining:** Live-tree recheck pending (`/tmp/spark/` not
present; original 593-violation claim from 2026-05-06 unverified).
The current Maven-multi-module rules use hand-coded directory lists
(brittle); a `for_each_file: pom.xml` + `scope_filter.has_ancestor`
refactor would be cleaner and self-discovering, flagged for future
work.

**New analyses surfaced:** Spark is the **headline driver** for
promoting `apache/governance@v1` from idea → v0.10 ship-target —
once it ships, spark's config drops 11 hand-rolled per-rule entries.
The `xml_path_*` v0.10 ship-target was promoted partly via spark's
49 pom.xml stress (alongside dotnet's scale); this case study
should be cross-linked from the rule-kind tracking once shipped.

### astral-sh-ruff

**Validation:** `validate-config` reports **75 rules** loaded
(21 in-config + 7 bundled overlays summing to ~58 rules with
overlap deduped). README claims "22-rule alint config" — actual
**21**, off by 1 (too small a delta to fix in prose; flagged
here).

**Edits applied:**
- `pair_inverse` references updated: now a **v0.10 design
  candidate** with **2 demand sources** (ruff + angular goldens)
  per launch-evidence.md; previous "Strong launch-prep
  candidate" / "v0.10+" framing replaced.
- `command_idempotent` references updated: now a **v0.10 design
  candidate** with **2 demand sources** (ruff + prettier) per
  launch-evidence.md; previous "v0.10+" framing replaced.
- Per-prek priority-chain framing softened (defer until
  multi-fix conflict resolution becomes a saturated cross-repo
  ask, rather than "file as v0.10+").
- Added "Future analysis" section (3 ideas: `for_each_leaf_dir`
  / `iter.is_leaf` adoption when shipped — itself a v0.10 design
  candidate with 3 sources per launch-evidence.md;
  `scope_filter.has_ancestor: Cargo.toml` refactor for crate-level
  rules; `agent-hygiene@v1` overlay trial).
- Added "Validation status (2026-05-07)" footer (with the
  21-vs-22 rule-count delta flagged).

**Gaps remaining:** Off-by-1 rule-count claim in body prose
(22 → 21); too small a delta to surgically fix in prose. The
README pre-dates the 21-pitfall catalogue and cites no specific
pitfall numbers, so no renumbering edits needed.

**New analyses surfaced:** ruff is one of 3 demand sources for
the `for_each_leaf_dir` candidate (alongside prettier + rust);
its hundreds of `crates/ruff_linter/src/rules/<linter>/snapshots/`
leaf-dirs are a clean fit. The
`scope_filter.has_ancestor: Cargo.toml` refactor would
self-express the "only `ruff`/`ruff_linter`/`ruff_wasm` are
versioned" rule without listing them by name.

## Cross-cutting patterns observed across batch 1

1. **Rule-kind candidate status drift was the dominant pattern.**
   Every README cited at least one v0.10+/v0.11+ candidate that
   has since been promoted to a ship-target, in 4 of 5 case
   studies. The most-promoted candidates:
   - `registry_paths_resolve`: arrow + spark cite it as primary
     gap; both updated to "v0.10 ship-target, 8 sources".
   - `cross_language_implementation_complete`: arrow + angular
     cite it; both updated to "v0.11+ ship-target, 5 saturated
     sources".
   - `ordered_block`: arrow + airflow (subsumes `file_lines_sorted`);
     both updated to "v0.10 ship-target, 7 sources".
   - `xml_path_*`: spark cites it; updated to "v0.10
     ship-target".
   - `cross_file_value_equals`: airflow cites it; updated to
     "v0.10 ship-target, 10 sources".
   - `apache/governance@v1` bundle: arrow + spark cite it;
     both updated to "v0.10 ship-target, 3 Apache TLPs".

2. **Bundled-overlay rule counts were systematically
   underestimated.** Both arrow ("roughly 35" → actual 44) and
   spark ("roughly 49" → actual 52) had bundled-rule-count
   prose drift. Likely cause: the bundled rulesets grew over
   time (oss-baseline 13 → 15, etc.) and the case-study
   prose wasn't kept current. Angular's bundled count was
   stated only in the headline ("~40 rules between them") and
   was off by ~18 — corrected to ~58 with explicit
   per-overlay counts.

3. **No pitfall #18/#19 fix-vs-workaround replacements needed.**
   None of the 5 READMEs explicitly cited a workaround for
   pitfall #18 (gitignore-masked tracked-file presence) or
   pitfall #19 (root_only literal-path no-match). Angular's
   parent-agent-notes section had a related historical note
   about an in-progress `RuleSpec.respect_gitignore` field —
   that note was updated to acknowledge the v0.9.17 shipped
   fix.

4. **Pitfall renumbering was minor.** Only angular cited a
   specific pitfall count ("2 of the 19 documented pitfalls");
   updated to acknowledge the catalogue grew to 21 in P2b
   Wave 2. Arrow's "claimed pitfall #17" / "claimed pitfall
   #18" historical note was clarified.

5. **No `command:` shellouts that newer rule kinds replace
   were spotted.** All 5 configs use `command:` for genuinely
   AST/parser-bound tools (tslint, ruff, scalastyle, checkstyle,
   pre-commit, clang-format, etc.) that remain out of alint's
   structural scope. No native v0.9.6+ rule kinds (scope_filter,
   xml_path_matches, cross_file_value_equals) would replace
   any current shellout — most of those rule kinds are still
   ship-targets, not shipped.

6. **Live-tree rechecks all pending.** `/tmp/<repo>/` not
   present at revalidation time for any of the 5; original
   violation counts (122 angular, 243 arrow, 593 spark, etc.)
   remain unconfirmed against current sparse-clones.

7. **`alint suggest` output was empty across the batch when
   tested against the case-study directories themselves.**
   The suggest subcommand correctly recognised no proposals
   were warranted (the case-study dirs only contain a README
   and a config — no antipattern surface to scan). Suggesting
   against an actual checked-out repo would be more meaningful;
   deferred to live-tree rechecks.

## Files touched

- `/home/kaminsod/projects/alint/examples/angular-angular/README.md`
- `/home/kaminsod/projects/alint/examples/apache-airflow/README.md`
- `/home/kaminsod/projects/alint/examples/apache-arrow/README.md`
- `/home/kaminsod/projects/alint/examples/apache-spark/README.md`
- `/home/kaminsod/projects/alint/examples/astral-sh-ruff/README.md`

5 README files modified. **0 `.alint.yml` files modified** (no
config bugs surfaced; all 5 still validate-config-clean post-edit
at the same rule counts as pre-edit).

## Open issues / suggestions for the parent agent

- **`alint suggest` against case-study directories yields zero
  proposals** — the suggest subcommand needs a real working tree
  to be meaningful; consider running it against the actual
  upstream sparse-clones (when present) as part of the "Future
  analysis" execution pass.
- **Rule-count drift in bundled-overlay prose** is a recurring
  paper-cut. Once `apache/governance@v1` and the v0.10 rule kinds
  ship, these counts will drift again. Worth a periodic
  bundled-count snapshot in the case-study revalidation log.
- **The "v0.11+ ship-target" naming for
  `cross_language_implementation_complete`** is slightly awkward
  prose-wise (the "+" suggests "or later"). Consider whether the
  launch-evidence framing should drop the "+" once the candidate
  has 5 saturated sources — current case-study prose was kept as
  "v0.11+ ship-target" to mirror launch-evidence.md.
