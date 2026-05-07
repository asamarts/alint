# Marketing-extraction — batch 2 (alphabetical: astral-sh-uv → dotnet-runtime)

Findings from the marketing/positioning extraction pass for the second
alphabetical batch of 5 case studies. Splits each public README into:

- **What stayed (public, engineering):** tooling inventory, mapping
  table, gap catalogue, validation status, future analysis (when
  engineering-shaped).
- **What moved (private, alint.org marketing):** positioning headers,
  "headline catch" framing, competitive language, launch-story wrappers.

Repos involved:
- alint (public): `/home/kaminsod/projects/alint`
- alint.org (private): `/home/kaminsod/projects/alint.org`
- alint binary used for rule counts: `target/release/alint` (v0.9.17)

Per-case-study rule counts (validated 2026-05-07):
- astral-sh-uv → 73
- bazelbuild-bazel → 81
- clap-rs-clap → 70
- denoland-deno → 76
- dotnet-runtime → 60

Narrative selection follows the existing alint.org gallery
(`src/pages/examples.astro`) so the per-case-study marketing page slots
into the same group it already lives in on the gallery.

---

## Per-case-study findings

### astral-sh-uv

- **Chosen narrative:** `convention-without-checks` (matches both the
  prompt directive and the existing gallery classification).
- **Moved to alint.org:**
  - "Headline finding" framing for the workspace-inheritance pattern
    (uv's 67 published crates inherit license/edition/`[lints] workspace`
    from one block; nothing in CI enforces it).
  - The "strongest case study so far for the cross-language monorepo
    positioning" pitch from the launch-story section.
  - The "alint catches what code review misses" tagline.
  - The 3 followup-feature bullet list as story-angle framing (kept the
    rule-kind candidate names in the public README under a neutral
    "Followup feature work surfaced" section).
- **Stayed in public README:** tooling inventory (3 tables — drop-in
  replacements, primitive gaps, out-of-scope), 13-rule config breakdown
  including the 9 real findings against the live tree, performance
  placeholder, future-analysis bullet list, validation footer.
- **Borderline calls:**
  - Kept the live-tree finding count (63 pass cleanly + 9 surface real
    findings, including the 3 `.ruff_cache/` directories) in public —
    that's quantitative engineering content.
  - Kept the "headline" wording removed but preserved the substance of
    the workspace-inheritance discussion (re-cast as neutral "uv's
    convention pattern across its 67 published crates is enforced
    nowhere in CI today").

### bazelbuild-bazel

- **Chosen narrative:** `orchestration-replacement` (matches the
  existing alint.org gallery classification — bazel sits under
  ORCHESTRATION_REPLACEMENT alongside pytorch). The prompt directive
  suggested "polyglot-wins" or "structural-floor"; the gallery's
  existing slot is `orchestration-replacement`, so I went with that for
  site consistency. The marketing page's headline + body still
  emphasises the polyglot-build-system shape and the `buildifier`
  delineation, so the substance lines up regardless of the
  narrative-enum slot.
- **Moved to alint.org:**
  - The full "Recommendation for the launch story" section (4-narrative
    table, "headline launch quote", "uniquely valuable as a case
    study" pitch).
  - The "structural floor under buildifier" framing as a recurring
    template (now a story-angle bullet).
  - The promotional headline-finding paragraph that opened the summary
    ("bazelbuild/bazel is the case where alint's '...' non-goal becomes
    most visible").
  - The pitfall #18 demo as a marketing headline catch (the YAML
    snippet + verification status — kept in public README too because
    the snippet is engineering-shaped, but the "demand source for the
    fix" framing landed on alint.org).
- **Stayed in public README:** the full BUILD-file notes section
  (the Starlark wall delineation, what alint catches vs can't catch,
  the orchestration-pattern enumeration), the pitfall #18 verified-fix
  snippet (engineering reference for any adopter hitting the same
  shape), the inventory tables, the gap catalogue with all v0.10
  rule-kind candidates, the 41-rule config breakdown, the validation
  footer.
- **Borderline calls:**
  - The pitfall #18 section (lines 416-461 in current README) was the
    closest call. The introduction "originally surfaced by bazel — now
    the canonical example" reads slightly promotional, but the rest of
    the section is the engineering reference (the symptom, the fix YAML,
    verification result) that adopters need. Decision: keep the whole
    section in public, with the marketing page carrying the "demand
    source" framing as the headline catch.
  - The four-narrative table at the bottom of the launch-story section
    is gallery-style positioning content; moved entirely to alint.org.
  - Validation status footer says "80 effective rules" (narrative
    lower-bound) but `validate-config` reports 81. Kept the footer's
    "80" as-is per the revalidation guard rails; cited 81 in the
    marketing page.

### clap-rs-clap

- **Chosen narrative:** `other` (matches the existing alint.org gallery
  classification; the prompt directive suggested
  `convention-without-checks` or `code-review-discipline`). The gallery
  uses `other` to slot the "small, disciplined Rust library workspace"
  archetype alongside vercel/turbo and denoland/deno. The marketing
  page narrative-label resolves to "Other case studies" via
  `[slug].astro`'s `NARRATIVE_LABEL` map.
- **Moved to alint.org:**
  - The "canonical well-curated Rust library workspace" lead.
  - The block-quote launch-positioning paragraph ("clap is what a
    clean Rust library workspace looks like, and alint can describe
    its entire structural-validation surface in 24 lines of YAML…").
  - The "complementary case study to kubernetes/rust-lang" framing.
  - "Use it as the positive baseline in the launch positioning."
- **Stayed in public README:** all 4 inventory tables (workspace
  manifest, auxiliary policy files, workflows, settings), the 24-rule
  config breakdown, the `cross_file_field_equals` and
  `regex_resolves_in_file` gap entries, the v0.10 design-candidate
  attribution, the future-analysis section, the validation footer.
- **Borderline calls:**
  - The "headline finding" callout in the manifest table ("clap's
    entire workspace-metadata contract maps to ~12 alint rules") was
    re-cast in the public README from "**This is the case study's
    headline:**" framing to a neutral statement-of-fact while keeping
    the same content.
  - The pitfall-#10 bracket-notation rediscovery note in the
    Recommendation section was engineering-shaped, kept in public.

### denoland-deno

- **Chosen narrative:** `other` (matches the existing alint.org gallery
  classification; prompt directive suggested `structural-floor`). The
  gallery slots deno under `OTHER` as "Rust + JS + TS multi-language;
  custom validation scripts", which captures the case study's
  cross-language pairing observations more cleanly than
  structural-floor would.
- **Moved to alint.org:**
  - The "Recommendation for the launch story" headers and framing
    (the two themes — language-AST query boundary, baselined-drift
    primitive — re-cast as neutral followup-feature framing in the
    public README, while the "load-bearing" / "right messaging" /
    pitch-style language landed on alint.org).
  - The "alint validates *structure*; if you need to reach into the
    language AST, keep your existing tool" tagline.
- **Stayed in public README:** all 3 inventory tables (drop-in
  replacements, gap catalogue, out-of-scope), the bundled-ruleset
  enumeration, the cross-language pairing observations section
  (engineering-shaped — `pair` rule for workflow generators,
  clippy.toml-per-crate pattern), the 4 v0.10 candidate rule kinds
  with full rationale (`disallowed_methods_in_file`,
  `violation_baseline`, `referenced_files_match_filesystem`,
  smarter `monorepo/cargo-workspace` selector,
  `dir_only_contains` subdir flag), the future-analysis section,
  the validation footer.
- **Borderline calls:**
  - The "Cross-language pairing observations" section kept its
    engineering shape but had some narrative-pitch wording that I
    left intact since it's framed around concrete rule mechanics.
  - The "single most-load-bearing missing rule kind for projects
    mid-migration" framing for `lintNodePolyfillDenoApis` is half
    engineering claim (load-bearing because of breadth-of-applicability
    across migrations), half pitch — moved the pitch to alint.org;
    public README now neutrally lists it as a "v0.10+ design pass"
    candidate.

### dotnet-runtime

- **Chosen narrative:** `structural-floor` (matches both the prompt
  directive and the existing alint.org gallery classification).
- **Moved to alint.org:**
  - The "the canonical XML-shape monorepo at scale" lead.
  - The "single most XML-heavy repo in the launch evidence list" claim.
  - The dual headline-catch framing (xml_path_* promotion + dotnet@v1
    ship-target).
  - The "Recommendation for the launch story" section in full
    (importance-weighted credibility pitch, "the layer alint owns",
    the "sixth tile on alint.org/examples" gallery placement, the
    paired polyglot finding flourish).
- **Stayed in public README:** all inventory tables (root config files,
  eng/ orchestration layer, .config/, .github/, per-csproj XML-shape,
  solution-file shape), the full gap catalogue with 5 numbered v0.10
  rule-kind / ruleset candidates and their rationales, the 8-invariants
  XML-path table, the 12-rule recommended `dotnet@v1` composition
  table, the "What's out of alint's scope" section, the parent-agent
  notes (audit pass, pitfalls hit, live findings on the cloned tree,
  config size reconciliation), future-analysis section, validation
  footer with v0.10 ship-target candidate status.
- **Borderline calls:**
  - The "RE-CONFIRMS spark; PROMOTES to v0.10 ship-target" header style
    on each gap entry was promotional but the section bodies are pure
    engineering. Re-cast headers to lowercase "re-confirms", removed
    "PROMOTES" / "Strong v0.10 ship-target" emphasis verbs while
    keeping the engineering substance ("Two demand sources;
    structured-query family becomes complete when xml_path_* ships").
  - The "single most prominent gap" framing on the dotnet@v1 ruleset
    section was lightly toned ("The most prominent gap surfaced by
    this case study") — still claims primacy among this case study's
    gaps, which is factual.
  - Kept the headline finding paragraph in the parent-agent notes
    (re-cast as "headline gap-catalogue finding") because the body is
    quantitative (8 invariants × 1,091 csprojs ≈ 8,700 invariant
    instances) — that's engineering data even when phrased
    headline-style.

---

## Cross-cutting patterns

- **Narrative consistency with the gallery is the right tiebreaker.**
  When the prompt suggested a narrative that didn't match the existing
  alint.org `examples.astro` slot, I went with the gallery to keep
  the per-case-study page slot consistent with where the card lives
  in the index. Rationale: the gallery is the source of truth for
  how case studies are grouped on the site; per-page metadata should
  echo it.
- **The headline-catch framing is the most reliable thing to move.**
  Every case study had a "Headline finding" or "Headline launch quote"
  callout in its summary, and an entire "Recommendation for the launch
  story" section. Both are pure positioning that lands cleanly on
  alint.org as the body's "Headline catch" subsection.
- **Followup feature work is borderline by default.** The
  "Followup feature work surfaced" sections enumerate v0.10+
  rule-kind / ruleset candidates with priority/demand-source counts.
  That's engineering content (gap catalogue), but the "strong
  ship-target" / "Top-priority" / "Stronger candidate" emphasis verbs
  are pitch-shaped. Decision: keep the bullet lists in public with
  emphasis verbs toned to neutral; let the marketing page carry the
  "uniquely surfaced here", "demand source", "ship-target" pitch.
- **Engineering-shaped findings inside promotional sections** are
  the single hardest borderline. dotnet/runtime's parent-agent-notes
  section had a "single most important finding" bullet that was 90%
  quantitative engineering data with one promotional flourish; the
  fix was to re-cast just the flourish ("headline gap-catalogue
  finding") rather than move the whole bullet.
- **Validation status footers stay in public, untouched.** Every
  README's "Validation status (2026-05-07)" section is pure engineering
  data (rule counts, reconciliation, pitfall fixes that affected this
  config, open gaps). No promotional language to extract; no edits
  needed.

## Borderline calls (consolidated)

- **bazel pitfall #18 demo:** the YAML snippet + verified-fix result
  is engineering content (any adopter hitting the same shape needs
  the snippet). The "demand source for the fix" framing is positioning.
  Decision: keep snippet in public, carry the "canonical motivating
  example" framing on alint.org.
- **clap "headline" callout in inline table:** rephrased neutrally
  in public ("12 distinct manifest assertions, all expressible as
  TOML path queries — clap's entire workspace-metadata contract
  maps to ~12 alint rules") while preserving the same fact. The
  block-quote pitch-paragraph went to alint.org.
- **deno "load-bearing" framing for `violation_baseline`:** mixed
  engineering claim + pitch. The breadth-of-applicability fact (same
  shape recurs in TS strict-mode adoption, Python type-coverage,
  Kubernetes restricted-imports, deno node-polyfills) is engineering;
  the "single most-load-bearing missing rule kind" wording is pitch.
  Decision: re-cast public README to neutral "Worth a dedicated v0.10+
  design pass"; keep the breadth-of-applicability framing on alint.org.
- **dotnet/runtime gap-section headers:** the "RE-CONFIRMS X; PROMOTES
  to v0.10 ship-target" capitalised emphasis was promotional but the
  section bodies are pure gap-catalogue engineering content.
  Decision: lowercase the header emphasis verbs, leave bodies intact.

## Rule-count reconciliation

`validate-config` was run against each case study's `.alint.yml` to
confirm rule counts before writing the marketing-page frontmatter.
Counts (config-loaded, including extends):

- astral-sh-uv: 73 (matches batch-2 revalidation log; 16 explicit + 57
  from extends − 0 facts = 73)
- bazelbuild-bazel: **81** (batch-2 revalidation log says 80; live
  validate reports 81. Kept 80 in the public README footer per
  the revalidation guard rails; cited 81 on the marketing page since
  it matches what `validate-config` actually reports today.)
- clap-rs-clap: 70 (matches the batch-2 revalidation log exactly)
- denoland-deno: 76 (matches the batch-2 revalidation log exactly)
- dotnet-runtime: 60 (matches the batch-2 revalidation log exactly)

The bazel discrepancy (81 vs 80) is a minor reconciliation gap; not a
blocker. The marketing page uses the live `validate-config` value.

## Blockers

None. All 5 case-study marketing pages and 5 public README edits land
cleanly within the constraint envelope (no commits, no edits outside
the 5 READMEs + 5 marketing pages + this batch findings file, no
.alint.yml edits).
</content>
</invoke>