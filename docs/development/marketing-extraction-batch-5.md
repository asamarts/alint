# Marketing extraction — batch 5

5 case studies (alphabetical batch 5): pnpm-pnpm, prettier-prettier,
protocolbuffers-protobuf, python-cpython, pytorch-pytorch.

Engineering content stays in `examples/<owner>-<repo>/README.md`
(public alint repo). Marketing/positioning/headline-catch framing
moved to `~/projects/alint.org/src/content/case-studies/<owner>-<repo>.md`
(private alint.org repo).

## Per-case-study findings

### pnpm-pnpm

- Narrative: `convention-without-checks`.
- Public README: marketing-pointer added; "Headline finding" recast
  to "Cross-cutting finding" (kept the factual 13 invariants
  measurement, dropped the launch-story framing); "Recommendation
  for the launch story" section deleted (kept follow-up feature
  work as "Followup feature work"); "Headline rules:" sub-heading
  renamed "Notable rules:" to avoid promotional ambiguity.
- Engineering preserved: 24 structural-validation surfaces, 17/3/4
  mapping breakdown, 13 meta-updater invariants table, husky chain,
  changeset shape, GHA workflows, pnpmfile / cspell / commitlint /
  per-package layout, the "needs new alint primitives" 3-pattern
  catalogue (`cross_file_value_equals` + `registry_paths_resolve` +
  `json_key_sort_order`), validation status (112 rules).
- Promotional moved to alint.org: "16M weekly downloads" credibility
  framing, "fourth tile" positioning, "the 'hand-rolled cross-package
  field-sync plugin' data point" framing, agent-hygiene narrative
  pairing, downstream pnpm-workspace adoption pitch.

### prettier-prettier

- Narrative: `structural-floor`.
- Public README: marketing-pointer added; "Headline finding" recast
  to "Cross-cutting finding" (kept factual measurement of 5 net-new
  gates); the inline "The headline finding." sentence in the
  per-language-plugin convention section deleted; "Three concrete
  launch-prep proposals" relabelled "Three concrete v0.10+ rule-kind
  proposals"; "Recommendation for the launch story" section deleted
  (kept follow-up feature work).
- Engineering preserved: 8-script lint cluster table, 16-step
  workflow inventory, 5 custom node scripts breakdown
  (lint-changelog.js, check-deps.js, format-test-lint.js,
  ensure-no-files-changed.js, clean-cspell.js), per-language-plugin
  convention non-enforcement finding, 4 needs-new-primitive entries
  (`json_key_value_forbidden`, `for_each_leaf_dir`,
  `command_idempotent`, `unique_by` cross-dir), validation status
  (68 rules).
- Promotional moved to alint.org: "cleanest 'structural floor on
  top' win" framing, "two distinct angles" launch-recommendation
  table, plugin-architecture launch-tile generalisation
  (webpack/rollup/vite/babel/postcss).

### protocolbuffers-protobuf

- Narrative: `polyglot-wins`.
- Public README: marketing-pointer added; "the densest polyglot
  binding repo in the OSS evidence catalogue" recast to "polyglot
  binding repo"; "Headline finding for the v0.11+ design phase"
  recast to "Cross-cutting finding"; "the densest cross-language
  parity discipline in the OSS catalogue and the strongest test
  case" recast to drop superlative framing while keeping the
  saturation count; per-binding-conformance-discipline subsection
  heading dropped "(the densest parity surface)" parenthetical;
  "the canonical demand-driver, and the densest of 5 sources"
  recast to "the canonical shape in this repo and one of 5 sources";
  "with protobuf as the densest source" recast to keep the
  quantitative shape (10 bindings × 4-5 parity surfaces = ~45
  cross-language assertions) without the densest-source framing;
  "Recommendation for the launch story" section deleted (kept
  follow-up feature work); Notes-for-parent-agent "densest
  single-repo source" sentence recast to keep the quantitative
  shape factual.
- Engineering preserved (per brief): factual "10 in-tree language
  bindings, ~45 cross-language assertions" measurement, all
  per-language-binding tables, 19 failure_lists / 8 text_format /
  11 test workflows / 6 in-tree runners / 22 GHA workflows / 137
  BUILD.bazel / 117 *.bzl Starlark / 13 editions goldens, the
  3-pattern needs-new-primitive catalogue, validation status (108
  rules).
- Promotional moved to alint.org: "densest single-repo source"
  framing, audience targeting (Google's protobuf team), arrow vs.
  protobuf differentiator narrative, Bazel-angle pitch.

### python-cpython

- Narrative: `script-sprawl`.
- Public README: marketing-pointer added; "Misc/NEWS.d/next/ — the
  headline finding" sub-heading recast to "Misc/NEWS.d/next/ —
  convention enforced only by tool-write-time grace" (factual
  description); "Recommendation for the launch story" section
  deleted (kept follow-up feature work); Performance-comparison
  "alint pitch here is" paragraphs recast as factual operational
  notes about config legibility and 38% coverage.
- Engineering preserved: 12 validation surfaces inventory,
  Makefile.pre.in 122-target table, 35 pre-commit hook breakdown,
  7 Tools/build/* scripts, .gitattributes 4747-byte breakdown,
  Misc/stable_abi.toml inventory, 25 GHA workflows table, NEWS.d
  filename grammar regex, 9-gap needs-new-primitive catalogue
  including the NEW `column_alignment` candidate, validation status
  (72 rules).
- Promotional moved to alint.org: "third positioning narrative
  crystallised in P2a-Wave 2" framing, the three-narrative table
  with cpython at the intersection, "we sit beneath your existing
  linters as the structural-orchestration layer" pitch, triple-
  narrative launch-tile angle.

### pytorch-pytorch

- Narrative: `orchestration-replacement`.
- Public README: marketing-pointer added; "This is the launch-pitch
  story for alint on pytorch:" intro paragraph + the "alint isn't
  trying to replace lintrunner" blockquote deleted; "So the headline
  is:" sentence-opener trimmed; "Recommendation for the launch story"
  section deleted (kept follow-up feature work with the factual 86%
  measurement preserved as a numerical note); "Already covered by
  other linters pytorch uses" — the lintrunner-row "alint sits BENEATH"
  phrasing recast as a neutral division-of-labour note;
  Performance-comparison "alint pitch here is inventory legibility
  AND fail-fast latency" framing recast as "two operational
  characteristics distinguish alint from lintrunner here".
- Engineering preserved (per brief): factual "86% of 57 lintrunner
  adapters" measurement; full 57-adapter table with structural /
  command / AST-aware breakdown; 144 GHA workflow inventory;
  needs-new-primitive catalogue (`cross_file_value_equals` 10
  sources, `registry_paths_resolve` 8 sources, `import_gate` 4
  sources, `generated_file_fresh` 6 sources, plus 3 NEW
  single-source candidates: `line_spacing`, `not_executable`,
  `directory_hash`); validation status (87 rules).
- Promotional moved to alint.org: "alint sits beneath as the
  structural floor" framing, "fourth positioning narrative" framing,
  "alint is what you would have built instead of lintrunner if
  lintrunner had existed" pitch, custom-orchestrator launch-tile
  angle (pytorch + bazel + tensorflow).

## Cross-cutting patterns

1. **All 5 case studies originally ended with a "Recommendation for
   the launch story" section.** This was the cleanest extraction
   target — every one had explicit "Position it as the Nth tile",
   "Headline launch quote", or "fourth positioning narrative"
   framing that mapped 1:1 to a marketing case-study page. Section
   was deleted in all 5 READMEs and replaced with a brief
   "Followup feature work" intro that points to the alint.org
   marketing writeup.

2. **The `Headline finding:` paragraph in Summary** appeared in 3
   of the 5 (pnpm, prettier, protobuf — though protobuf called it
   "Headline finding for the v0.11+ design phase"). Recast to
   "Cross-cutting finding" in all three so the factual measurement
   stays but the "headline" framing moves.

3. **"alint pitch" / "alint sits beneath" prose** — appeared in 3 of
   the 5 (cpython, pytorch, partially in pnpm). All recast as factual
   operational notes about config legibility, fail-fast latency,
   division of labour. Performance-comparison sections in particular
   needed careful editing because they conflate engineering speed
   characteristics with launch-pitch framing.

4. **Density / superlative language ("densest", "strongest test
   case", "canonical demand-driver"** — concentrated in protobuf
   (per the polyglot-wins narrative). Recast to drop superlatives
   while preserving quantitative measurements (per the brief's
   guidance for protobuf specifically).

5. **Schema collection alignment**: alint.org/src/content.config.ts
   defines a `caseStudies` collection with required fields
   `title / repo / headline / narrative (enum) / rules (positive int)
   / lastValidated (ISO date)`. All 5 frontmatters validate against
   this schema. Narrative enum values used: `convention-without-checks`
   (pnpm), `structural-floor` (prettier), `polyglot-wins` (protobuf),
   `script-sprawl` (cpython), `orchestration-replacement` (pytorch)
   — uses 5 of the 7 allowed values; covers the breadth of the
   alphabetical batch.

## Borderline calls

1. **pnpm "Headline rules:" sub-heading** — under "Starter alint
   config (drop-in)". Could read as a promotional call-out or as
   "the most notable rules in the config" engineering listing.
   Renamed "Notable rules:" to remove ambiguity. Defensible either
   way; renamed to be safe.

2. **pytorch "Already covered by other linters" lintrunner row** —
   the original "alint sits BENEATH; CI runs both" is partially
   factual (CI does run both) and partially positioning ("BENEATH"
   echoes the structural-floor narrative). Recast as an explicit
   "division of labour" note tied to the per-adapter mapping above
   it. Same factual content, no positioning framing.

3. **protobuf factual measurements** — the brief explicitly
   instructs to keep "10 in-tree language bindings, ~45
   cross-language assertions" as engineering and move only the
   "densest single-repo source" framing. The "5 sources past
   saturation" count is intermediate (it's a quantitative count but
   used promotionally). Decision: kept the count as factual in the
   README (a saturation-count is a roadmap fact) but dropped the
   "densest of 5 sources" superlative ranking. The
   `cross_language_implementation_complete` v0.11+ ship-target
   claim is similarly factual roadmap status; kept.

4. **prettier per-language-plugin convention** — the
   "(NOT enforced anywhere on disk)" sub-heading is technically
   factual (it IS the case) but reads like a marketing call-out.
   Kept it because it directly describes the engineering finding
   that motivates the 5 net-new alint rules; the inline "The
   headline finding." sentence right after it was the genuinely
   promotional phrasing and that was deleted.

5. **cpython "convention enforced only by tool-write-time grace"
   sub-heading** — the recast description of the original "the
   headline finding" sub-heading. Reads slightly evocative but
   the phrase is descriptively accurate (blurb generates correct
   names; nothing checks them). Kept as the factual description.

## Blockers

None. All 5 case studies extracted cleanly. Schema validation,
narrative enum membership, file-path conventions, and frontmatter
shape all match the existing angular-angular reference case study.

## Counts

- 5 alint.org case-study files created (under
  `~/projects/alint.org/src/content/case-studies/`).
- 5 public alint READMEs edited in place.
- 1 batch findings file (this file).
- 0 .alint.yml edits.
- 0 commits, 0 pushes (per task constraints).

Rule counts (matching the public README validation status footers):

| Case study | Rule count |
|---|---|
| pnpm-pnpm | 112 |
| prettier-prettier | 68 |
| protocolbuffers-protobuf | 108 |
| python-cpython | 72 |
| pytorch-pytorch | 87 |
