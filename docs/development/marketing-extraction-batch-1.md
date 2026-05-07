# Marketing extraction batch 1

Per-batch findings file for the marketing/optics/positioning extraction
pass across the alphabetical batch 1 case studies (angular-angular,
apache-airflow, apache-arrow, apache-spark, astral-sh-ruff).

Engineering content (tooling inventory, mapping, gap catalogue, validation
status) stays in the public alint repo at `examples/<repo>/README.md`.
Marketing content (5-narrative wrappers, headline catches, strategic
positioning, "where alint shines" framings, launch-tile recommendations)
moved to alint.org at `src/content/case-studies/<owner>-<repo>.md`.

The line is LANGUAGE not CONTENT — same fact can be expressed neutrally
(engineering) or promotionally (marketing). Re-cast promotional framing
to neutral in the public README; preserve the promotional version on
alint.org.

---

### angular-angular

- **Marketing content moved**: 3 sections (Headline finding paragraph
  promotional framing, "Recommendation for the launch story" full section
  with 5 sub-bullets and "fifth tile" positioning, "Where alint shines on
  angular specifically" promotional bullets in Performance comparison)
- **Engineering content preserved**: Existing tooling inventory (full
  table), mapping table for 17 of 27 → existing alint rules, 6
  hand-rolled rule highlights re-cast neutrally, gap catalogue (3
  primitives:
  cross_language_implementation_complete/cross_file_value_equals/pair_inverse),
  pitfall #16 reference, validation status footer, future analysis (3
  engineering-shaped ideas — pair_inverse trial, compliance/reuse@v1
  trial, agent-hygiene@v1 overlay)
- **Headline catch text**: "alint catches 6 license-header drifts
  including a UTF-8 BOM byte that no other tool in angular's pipeline
  looks at — and 5 packages without API goldens that pnpm public-api:check
  silently no-ops on."
- **Chosen narrative**: `structural-floor` (the public README enforces
  what tslint + prettier + pnpm public-api:check can't see — license
  headers, golden parity, version-placeholder discipline)
- **Notes on borderline content**: The "headline rules" subsection inside
  Starter alint config was promotional ("Surfaces 6 real drifts including
  a leading UTF-8 BOM that prettier doesn't catch") but contains
  load-bearing engineering content (which rules + what they catch). I
  re-cast to neutral language ("Selected rules:", "Surfaces 6 drifts in
  the live tree including a leading UTF-8 BOM byte") rather than moving
  the section. The "5 saturated demand sources" framing in the gap
  section was demand-saturation accounting (engineering) but the
  emphasis-bold ("**Strong v0.11+ signal**") was promotional — re-cast
  the rhetoric, kept the count.

---

### apache-airflow

- **Marketing content moved**: 3 sections ("most alint-shaped surface in
  the entire airflow tree" Summary tail, "Recommendation for the launch
  story" full section, "headline win" + "headline perf number" promotional
  framings)
- **Engineering content preserved**: 109-hook breakdown table, mapping
  table (~30 hooks → existing alint rules), gap catalogue (cross-file
  value sync, import gates, file-content sortedness with v0.10 ship-target
  status), Out of scope catalogue, perf comparison (50× speedup re-cast
  factually), validation status, future analysis (3 engineering-shaped
  ideas)
- **Headline catch text**: "alint expresses Airflow's
  101-provider-package layout invariants in 25 lines of YAML — the same
  invariants that today live in 1085 lines of Python that has to spin up
  a docker container."
- **Chosen narrative**: `script-sprawl` (109 pre-commit hooks across 14
  repo blocks, with ~80 being `repo: local` shell-outs to Python scripts
  — the canonical script-sprawl shape)
- **Notes on borderline content**: The "headline win" framing in the
  Summary was a clear marketing wrapper ("Replacing those ~30 hooks with
  one declarative file ... is the headline win"). I re-cast to factual
  ("consolidates the question to one place"). Similarly the "headline
  perf number" claim in Performance: re-cast to "~50× speedup on this
  subset" with the factual numbers preserved.

---

### apache-arrow

- **Marketing content moved**: 4 sections ("Headline finding" paragraph
  with "flagship pitch" framing, "Recommendation for the launch story"
  full section, "this is where alint earns its keep" sub-header, "most
  distinctive structural feature" + "canonical alint-shaped surface"
  framings)
- **Engineering content preserved**: 34-surface breakdown,
  full inventory tables (root config files, dev/release/, .github/workflows/,
  per-language subtree, per-Ruby-gem layout, format/, pre-commit hooks),
  mapping table (65 in-config rules + 6 bundled rulesets), gap catalogue
  (registry_paths_resolve, cross_language_implementation_complete,
  ordered_block, pre-commit fan-out), filter-expression pitfall reference
  (CONFIG-AUTHORING.md § 10), validation status, future analysis (3
  engineering-shaped ideas — compliance/reuse@v1 trial,
  apache/governance@v1 adoption, scope_filter ancestor-manifest narrowing)
- **Headline catch text**: "6 languages in one tree, 21 lint hooks
  across 14 tool repos, and 0 tools that see the cross-language
  conventions — alint is the layer that does."
- **Chosen narrative**: `polyglot-wins` (the canonical multi-language
  polyglot monorepo flagship — clang-format only sees C++, rubocop only
  sees Ruby, etc.)
- **Notes on borderline content**: The "demand-saturation" language ("8
  demand sources for `registry_paths_resolve`", "5 saturated demand
  sources for `cross_language_implementation_complete`") is on the line —
  it's factual accounting that doubles as promotional positioning. I kept
  the counts and removed the emphasis ("highest-leverage gap in P2a",
  "Strong v0.11+ signal") and the marketing wrapper ("the canonical
  alint-shaped surface for this repo"). The "16 source files missing the
  Apache header — ALL legitimate" finding is structural truth, kept.

---

### apache-spark

- **Marketing content moved**: 4 sections ("Headline finding" paragraph
  with "promoting from v0.10+ idea to v0.10 ship-target" framing,
  "Recommendation for the launch story" full section, "this is where
  alint earns its keep on apache/spark" sub-header, "the canonical
  big-data engine" + "Strong v0.11+ signal" + "Strong v0.10 signal for
  apache/governance@v1" promotional framings)
- **Engineering content preserved**: 38-surface breakdown, full inventory
  tables (root config files, dev/, .github/workflows/, per-language
  MODULE, Apache governance artefacts), mapping table (61-rule config +
  6 bundled rulesets = 110 total), 5 gap candidates
  (registry_paths_resolve, Maven `<modules>` registry, cross-language
  registry consistency, LICENSE-binary cross-reference, xml_path_*),
  Apache governance discipline notes (the convergence-across-3-TLPs
  table), Maven multi-module findings (3 architectural findings),
  pitfall #10 reference (the v0.9.15 Phase 4 enriched diagnostic that
  caught spark's draft error), validation status, future analysis (3
  engineering-shaped ideas)
- **Headline catch text**: "4 languages, 49 Maven modules, 3 PySpark
  packaging variants, 72 GHA workflows, and 6 per-language lint scripts
  — alint sees the structural shape no per-language linter does."
- **Chosen narrative**: `polyglot-wins` (4-language Apache TLP polyglot
  monorepo — same family as arrow but with a per-language-MODULE
  mandate shape rather than parity-mandate)
- **Notes on borderline content**: The Apache governance discipline
  section is the densest borderline — it's the launch-shaped argument
  ("3 TLPs converge → ship apache/governance@v1") expressed as
  engineering accounting (12-artefact convergence table). I kept the
  full section in the public README (the table is engineering analysis)
  but re-cast the section header from "now a v0.10 ship-target" to "v0.10
  ship-target" framing. Marketing version on alint.org gets the same
  table re-framed as the headline catch.

---

### astral-sh-ruff

- **Marketing content moved**: 3 sections ("Headline finding for the
  launch story" subhead in `crates/ruff_dev/`, "Recommendation for the
  launch story" full section with "two distinct angles", "This is a
  direct alint opportunity" framing)
- **Engineering content preserved**: 16-hook prek inventory, 22-job CI
  inventory, mapping table (15 of 16 prek hooks → alint replacements),
  the ruff_dev codegen-not-tidy distinction (re-cast neutrally), gap
  catalogue (pair_inverse, command_idempotent, validate-pyproject deep
  schema, ecosystem regression diff), Out of alint's scope, validation
  status, future analysis (3 engineering-shaped ideas — for_each_leaf_dir,
  scope_filter has_ancestor, agent-hygiene@v1)
- **Headline catch text**: "ruff has 900+ rules for Python, but zero
  rules for its own per-crate manifest discipline. alint is the missing
  piece."
- **Chosen narrative**: `convention-without-checks` (the conventions
  exist but enforcement is entirely social — every internal crate is
  `version = "0.0.0", publish = false` but nothing checks it)
- **Notes on borderline content**: The "ruff is a linter that can't lint
  its own structure" framing is the punchiest single line in this case
  study. It's both engineering observation (factually true: ruff_dev is
  codegen, not tidy) and marketing punchline. I moved the punchline
  framing to alint.org (as the second angle), kept the factual ruff_dev
  description in the public README (re-cast from "Headline finding for
  the launch story" to neutral). The "two distinct angles" framing
  itself was clearly marketing, fully moved.

---

## Cross-cutting patterns observed

1. **"Headline finding" / "headline catch" / "headline X" was the
   single most common marketing tell.** All 5 case studies opened the
   Summary with a "**Headline finding:**" paragraph. The factual
   content (live-tree finding counts, what no existing tool catches)
   stayed in the public README; the promotional framing ("THE canonical
   example", "the strongest piece of evidence", "the flagship X for
   alint") moved to alint.org.

2. **"Recommendation for the launch story" was 100% marketing.** Every
   single case study had this section, every one was fully removed from
   the public README and folded into alint.org's "Why this case study
   matters" + "Future story angles" sections. None of them belonged in
   engineering reference material.

3. **"Where alint shines on X specifically" / "this is where alint
   earns its keep on X" was 100% marketing**, scattered through both
   inventory and Performance sections. Always re-cast to neutral
   ("Coverage on X specifically:") in the public README.

4. **Demand-saturation accounting was on the line.** Counts ("5
   demand sources", "8 demand sources") are factual; emphasis
   ("strongest demand signal in P2a", "highest-leverage gap")
   is promotional. Removed the emphasis; kept the counts.

5. **The "fifth tile on alint.org/examples" prescriptive positioning
   appeared 4 times.** All 4 instances were at the bottom of
   "Recommendation for the launch story" sections. All moved to
   alint.org under "Future story angles → Launch tile candidate".

6. **Validation status footer + future analysis (when engineering-shaped)
   stayed put** — these were the targeted engineering enrichment from
   the recent revalidation pass, not marketing wrappers. All 5 case
   studies' future-analysis sections are engineering-shaped (concrete
   bundled-ruleset trials + scope_filter refactors + rule-kind
   adoption-when-shipped).

## Borderline calls

The most ambiguous calls in this batch were:

- **angular's "headline rules" sub-section** — the 6-rule highlight
  list inside Starter alint config opens with the marketing tell
  ("**Surfaces 6 real drifts including a leading UTF-8 BOM that
  prettier doesn't catch.**"). The list itself is engineering (which
  rules and what they catch). I re-cast the section header from
  "headline rules:" to "Selected rules:" and toned down the emphasis
  while keeping all rule descriptions.

- **arrow's "this is where alint earns its keep on apache/arrow"
  sub-headers** appeared twice (under Per-language subtree and
  Performance comparison). Both removed — they're orphaned marketing
  pointers without standalone content.

- **spark's "headline finding" paragraph** is unique in this batch
  because it's NOT a single-finding catch — it's a
  3-Apache-TLP-convergence accounting that's structurally engineering
  but tonally promotional. I re-cast to neutral ("Cross-TLP
  convergence (factual):") and kept the 12-artefact table in place.
  The marketing-shaped version of the same fact landed on alint.org.

- **ruff's "Alint is exactly the missing piece"** is a marketing
  punchline embedded in an engineering summary. I removed the
  marketing-line ("**Alint is exactly the missing piece**") and the
  "This is a direct alint opportunity:" subhead while keeping the
  factual ruff_dev-is-codegen-not-tidy observation. The punchline
  reappears on alint.org as the headline catch.
