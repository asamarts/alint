# Marketing-extraction batch 3

Per-case-study findings from extracting marketing/optics/positioning
content out of the public `examples/<repo>/README.md` files into the
private `alint.org` case-study collection.

The principle: **engineering findings stay in the public README;
positioning + headline-catch framing moves to alint.org**. Same fact,
different language — factual stays, promotional moves.

Batch scope (alphabetical batch 3, 5 case studies):

- `examples/facebook-react/`
- `examples/flutter-flutter/`
- `examples/golang-go/`
- `examples/helm-helm/`
- `examples/istio-istio/`

---

## facebook-react

- **Narrative:** convention-without-checks
- **Rules:** 87 (54 from 8 bundled rulesets + 33 react-specific)
- **alint.org page:** `src/content/case-studies/facebook-react.md`
- **README marketing-pointer:** added under top H1
- **Recasts performed:**
  - "**Headline finding:** ... ~600-entry, append-only JSON registry
    ... **The position: 'alint replaces the structural floor under
    react's well-evolved tooling, surfacing real drift ...'**"
    paragraph in the Summary (lines ~67–82) → tightened to a factual
    "**Key finding:**" paragraph that names codes.json + ReactVersion.js
    as a registry/single-source-of-truth pair and points at the two
    rule-kind candidates by status; the embedded "**The position:**"
    pull-quote moved to the alint.org page as the Headline catch
- **Sections removed:**
  - "## Recommendation for the launch story" — the entire
    most-watched-on-GitHub framing, the kubernetes/turbo contrast,
    and the "evolved JS monorepo data point" paragraph. Replaced
    with a plain "## Followup feature work surfaced (consolidated)"
    heading directly above the rule-kind candidate list (the
    rule-kind list itself is engineering content and stays)
- **Sections preserved:**
  - Summary (rule counts, mapping breakdown — engineering)
  - Existing tooling inventory (5 surfaces, all tables)
  - Findings against the live tree (the actionable drift list —
    engineering result)
  - Starter alint config (rule-by-rule walkthrough)
  - What needs new alint primitives (gap catalogue)
  - Performance comparison
  - Pitfalls / Suggested CONFIG-AUTHORING.md addition
  - Validation status (2026-05-07) footer
  - Future analysis
- **Marketing material moved to alint.org:**
  - The "real and actionable drift" framing (1 wrong
    `repository.directory`, 19 bugs URLs, 345 source files
    missing copyright) — engineering finding stays in the public
    Findings table; the "drift exists even at the most evolved end
    of the JS monorepo spectrum" reframe lives on the alint.org
    case-study page only
  - "Use as evidence on alint.org/examples that ..." sentence
    moved entirely to the alint.org page
  - "react is the most-watched JS UI library on GitHub (>240k
    stars, ~1k contributors). Naming it as a target gives alint
    instant credibility" — moved out
  - The kubernetes/turbo contrast as positioning — moved out
- **Borderline calls:**
  - The "Headline finding:" pull-quote in Summary mixes a factual
    claim (codes.json + ReactVersion.js shape) with a positioning
    claim ("**The position: '...'**"). Split: kept the factual
    claim as "Key finding"; moved the bracketed Position quote to
    alint.org as the Headline catch
  - The followup feature-work list IS engineering (rule-kind demand
    counts, candidate status) so it stays in the README; only the
    section heading was simplified

---

## flutter-flutter

- **Narrative:** polyglot-wins (platform-driven variant)
- **Rules:** 68 (39 flutter-specific + 29 from 3 bundled rulesets)
- **alint.org page:** `src/content/case-studies/flutter-flutter.md`
- **README marketing-pointer:** added under top H1
- **Recasts performed:**
  - "**Headline finding:** flutter/flutter is **the** flagship
    'platform-driven polyglot monorepo' pitch for alint ..." in
    Summary (lines ~184–195) → tightened to "**Key finding:** the
    canonical platform-driven polyglot monorepo ... fifth independent
    demand signal for `cross_language_implementation_complete`". The
    "no per-language linter sees this cross-platform consistency"
    promotional list (Android Studio / Xcode / MSVC / clang-format)
    moved to the alint.org page
- **Sections removed:**
  - "## Recommendation for the launch story" entirely — the
    "second-most-starred Google OSS project on GitHub" framing, the
    Apple framework "cleanest demo" framing, the Wave-2 polyglot
    tile pitch quote, and the three-distinct-variants
    cross_language_implementation_complete pitch. Replaced with a
    plain "## Followup feature work surfaced" heading
- **Sections preserved:**
  - Summary (rule counts, mapping breakdown — engineering)
  - Existing tooling inventory (all tables)
  - What maps to existing alint rules (rule-by-rule walkthrough)
  - What needs new alint primitives (gap catalogue + sketch)
  - What's out of alint's scope
  - Already covered by other linters
  - Performance comparison
  - Notes for the parent agent — including the **5 errors on
    `oss-no-bidi-controls`** live-tree finding (CVE-2021-42574 hits
    in `docs/about/Values.md` and 4 archived release-notes files
    under `docs/releases/archive/`); this stays as a factual
    live-tree result in the engineering README
  - Validation status (2026-05-07) footer
  - Future analysis
- **Marketing material moved to alint.org:**
  - The "strongest 'alint catches things other tools miss' data
    point in the corpus" framing of the 5 Trojan-Source errors
    (the FACT stays in the README; the MARKETING PUNCH is now on
    the alint.org page as the Headline catch)
  - "no per-language IDE / linter sees the cross-platform
    conventions" pitch
  - "second-most-starred Google OSS project on GitHub (~170k
    stars)" credibility line
  - The three-distinct-variants comparison framing (arrow data-
    format-driven, tensorflow data-format-driven-at-API-scale,
    flutter platform-driven)
  - The Wave-2 polyglot tile positioning quote
- **Borderline calls:**
  - The 5 CVE-2021-42574 findings: per the brief, **kept the
    factual finding** (where, how many, which files) in the
    engineering README's live-tree results bullet, and **moved
    the marketing punch** ("strongest in the corpus", "single
    rule, 5 real CVE hits") to the alint.org Headline catch.
    The factual bullet in the README does not call these
    "headline" or "strongest"
  - Lightly trimmed one tail bullet under "Notes for the parent
    agent" that called the BSD-header rule "the cleanest single-
    rule polyglot demo in the case-study catalogue" — that was
    pure positioning. Replaced with a factual statement of file
    coverage (~9,000 source files in scope)

---

## golang-go

- **Narrative:** code-review-discipline
- **Rules:** 64 (31 golang/go-specific + 33 from 3 bundled rulesets,
  one rule deduplicated)
- **alint.org page:** `src/content/case-studies/golang-go.md`
- **README marketing-pointer:** added under top H1
- **Recasts performed:**
  - The "**The headline is NOT** ... it's **'alint encodes the
    unwritten Go conventions enforceable for the first time.'**"
    promotional paragraph in the Summary (lines ~61–70) → "**Key
    finding:**" with the same zero-tooling list, neutralised tone.
    The "the headline is" framing moved to the alint.org page
- **Sections removed:**
  - "## Recommendation for the launch story" entirely — the
    block-quote pitch ("kubernetes has 50 ... tokio ... **golang/go
    has effectively zero ...**"), the four-narrative positioning
    table, and the closing "alint pitch lands as ..." paragraph.
    Replaced with a plain "## Followup feature work surfaced"
    heading directly above the existing rule-kind list. The
    four-narrative positioning table moved to the alint.org page
- **Sections preserved:**
  - Summary (zero-everything inventory + percentage breakdown —
    engineering)
  - Existing tooling inventory (4 in-tree validation surfaces, 10
    config files, 4-go.mod canonical layout, FIPS 140 registry,
    `doc/next/` release-notes structure — all engineering)
  - Maps to existing alint rules
  - Drop-in replacements (none) note
  - Conventions encoded for the first time table
  - Defensive shellouts table
  - What needs new alint primitives
  - Out of alint's scope
  - Already covered
  - Starter alint config
  - Performance comparison
  - Methodology notes (sparse-checkout strategy, license-header
    regex iteration — engineering)
  - Validation status (2026-05-07)
  - Future analysis
- **Marketing material moved to alint.org:**
  - The "fourth distinct narrative" positioning + table
  - The full multi-line block-quote pitch comparing kubernetes,
    tokio, golang/go
  - "The alint pitch lands as ..." closing line
  - The generalisation list (Linux kernel, plan9, suckless tools,
    house-style internal codebases) — moved to the "Future story
    angles" section of the alint.org page
- **Borderline calls:**
  - The four-narrative table itself is positioning content but it
    was the cleanest summary of where golang/go fits in the
    catalogue. Moved entirely to alint.org; the engineering README
    no longer references the positioning narratives directly
  - "31 conventions, 0 scripts, 0 workflows" is technically a
    positioning slogan, but the FACTS (zero scripts, zero workflows)
    are stated in the Summary's bullet list as engineering
    inventory; the slogan formulation is on the alint.org page

---

## helm-helm

- **Narrative:** structural-floor
- **Rules:** 58 (24 helm-specific + 34 from 4 bundled rulesets, with
  rule-id deduplication across overlapping rulesets)
- **alint.org page:** `src/content/case-studies/helm-helm.md`
- **README marketing-pointer:** added under top H1
- **Recasts performed:** none in the Summary — helm's Summary
  already reads as engineering content (the percentage breakdown
  + "what 70% maps cleanly" framing) and didn't carry a "headline
  finding" pull-quote
- **Sections removed:**
  - "## Recommendation for the launch story" — the "realistic
    adoption target" framing, the kubernetes/rust/clap anchoring
    list, the multi-line block-quote pitch, and the
    "complementary case study to kubernetes/golang-go" closer.
    Replaced with a plain "## Followup feature work surfaced"
    heading directly above the rule-kind candidate list
- **Sections preserved:**
  - Summary (rule counts + percentage breakdown — engineering)
  - Existing tooling inventory (Makefile, validate-license.sh,
    `.golangci.yml`, GitHub Actions, `.github/env`, OWNERS,
    `.goreleaser.yaml`, top-level files — all engineering)
  - Maps to existing alint rules (rule-by-rule walkthrough)
  - **Real findings against the live tree (2026-05-06 snapshot)**
    — the zero-width / 5-workflow-permissions / trailing-whitespace
    findings stay in the engineering README as factual results
  - Needs new alint primitive (gap catalogue)
  - Out of alint's scope
  - Already covered
  - Performance comparison
  - Pitfalls catalogued during config authoring
  - Validation status (2026-05-07)
  - Future analysis
- **Marketing material moved to alint.org:**
  - The "complementary case study to kubernetes/golang-go" framing
  - The "typical CNCF-shape" pitch quote
  - The "realistic adoption target" + "population alint needs to
    convert" framing
  - The bolded "**In the live snapshot, alint surfaced one zero-
    width-char comment in plugin.go and 5 workflows missing
    `permissions.contents: read` — net-new structural findings
    the existing pipeline doesn't catch.**" pull-quote (the
    factual findings stay; the marketing punch moves)
- **Borderline calls:**
  - The "Real findings against the live tree" section's framing
    "Net-new structural finding alint catches that no existing
    tool in helm's pipeline does" is borderline — it's both a
    fact AND a positioning claim. Kept in the engineering README
    because the facts (zero-width-char in `plugin.go:80`,
    5 workflows by name) are the actionable engineering content;
    the positioning formulation also lives on the alint.org page
    as the Headline catch

---

## istio-istio

- **Narrative:** polyglot-wins (CNCF service-mesh + Helm-chart
  discipline)
- **Rules:** 65 (28 istio-specific + 37 from 4 bundled rulesets)
- **alint.org page:** `src/content/case-studies/istio-istio.md`
- **README marketing-pointer:** added under top H1
- **Recasts performed:**
  - "**Headline finding for the launch story.**" tagline at the end
    of finding #1 (cobra-cli placeholder in precheck.go) → removed.
    The factual engineering finding stays (the bash script accepts
    the placeholder leak; alint's regex-anchored rule catches it);
    the "Headline finding" billing moves to the alint.org page
- **Sections removed:**
  - "## Recommendation for the launch story" entirely — the
    "largest CNCF / service-mesh OSS adoption target" framing, the
    three-anchor positioning list (kubernetes/helm/istio), the
    multi-line block-quote pitch, and the "complementary case
    study to kubernetes + helm" closer. Replaced with a plain
    "## Followup feature work surfaced" heading directly above
    the rule-kind candidate list
- **Sections preserved:**
  - Summary (rule counts + percentage breakdown — engineering)
  - Existing tooling inventory (Makefiles, lint_copyright_banner.sh,
    `.golangci.yml`, helm-chart structural surface, CRDs, release-
    note schema, CODEOWNERS, `prow/`, `istio.deps`, `common/`,
    top-level files — all engineering)
  - Maps to existing alint rules
  - **Real findings against the live tree (2026-05-06 snapshot)**
    — all 9 findings (cobra placeholder, gRPC-Authors header,
    HTTP-not-HTTPS regression, piVersion typo, kind-enum drift,
    23 hygiene findings, missing CoC) stay as factual engineering
    results
  - Needs new alint primitive (gap catalogue) — including the
    full pitfall #20 + #21 design-candidate analysis
  - Out of alint's scope
  - Already covered
  - Performance comparison
  - **Pitfalls catalogued during config authoring** (the full
    pitfall #20 + #21 surfacing analysis with `value_extractor:`
    and `multi_doc_mode:` knob design — engineering content,
    stays)
  - Validation status (2026-05-07)
  - Future analysis
- **Marketing material moved to alint.org:**
  - The "largest CNCF / service-mesh OSS adoption target" framing
  - The three-anchor positioning list
  - The full multi-line block-quote pitch enumerating the live-
    snapshot findings (the findings themselves stay; the pitch
    formulation moves)
  - The "complementary case study to kubernetes + helm" framing
  - The "Helm-chart structural-discipline axis no earlier case
    study has covered" closer
- **Borderline calls:**
  - The pitfall #20 + #21 sections are LARGELY engineering content
    (design analysis, workaround documentation, fix-target version)
    so they stay in the README. The alint.org page summarises them
    as the Headline catch but does not duplicate the design detail
  - The "named source" attribution for the per-file
    `value_extractor:` refinement and the `multi_doc_mode:` knob
    is engineering provenance (it tells the rule-kind designer
    where to test), not positioning, so it stays in the README

