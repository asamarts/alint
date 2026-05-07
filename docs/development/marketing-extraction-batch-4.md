# Marketing-extraction batch 4

Per-case-study findings from extracting marketing/optics/positioning
content out of the public `examples/<repo>/README.md` files into the
private `alint.org` case-study collection.

The principle: **engineering findings stay in the public README;
positioning + headline-catch framing moves to alint.org**. Same fact,
different language — factual stays, promotional moves.

Batch scope (alphabetical batch 4, 5 case studies):

- `examples/kubernetes-kubernetes/`
- `examples/microsoft-typescript/`
- `examples/microsoft-vscode/`
- `examples/nixos-nixpkgs/`
- `examples/nodejs-node/`

---

## kubernetes-kubernetes

- **Narrative:** script-sprawl (the canonical "replaces N hand-rolled
  scripts" example)
- **Rules:** 49 (12 custom + 4 bundled rulesets)
- **alint.org page:** `src/content/case-studies/kubernetes-kubernetes.md`
- **README line count:** 194 -> 192 (-2)
- **README marketing-pointer:** added under top H1
- **Recasts performed:**
  - The Summary's "headline win" sentence ("Replacing those 20 shell
    scripts with one declarative config + one `alint check` invocation
    in CI is the headline win — fewer moving parts, one place to look
    when CI breaks, ~5x faster than running 20 shell scripts in
    sequence") was rewritten to a neutral factual statement:
    "Combined with `command:` shell-outs to shellcheck, spelling,
    gofmt, golangci-lint, and govulncheck, **17 of the 50 verify
    scripts** move into the declarative config. Rules dispatch in
    parallel against a single filesystem walk." The headline-win
    framing + the 5x speedup positioning moved to the alint.org page
- **Sections removed:**
  - "## Recommendation for the launch story" entirely — the
    "strongest single piece of evidence", "Use it as the headline
    example on alint.org/examples and in the HN/Reddit launch posts"
    pitch. Replaced with a plain "## Followup feature work surfaced"
    heading directly above the rule-kind candidate list (the
    rule-kind list itself is engineering content and stays)
- **Sections preserved:**
  - Summary (rule counts, mapping breakdown - engineering)
  - Existing tooling inventory (all tables: maps-directly,
    needs-new-primitive, out-of-scope, already-covered)
  - Starter alint config block (rule-by-rule walkthrough)
  - Performance comparison (placeholder)
  - Future analysis
  - Validation status (2026-05-07) footer
- **Marketing material moved to alint.org:**
  - The "headline win — fewer moving parts" framing
  - The "~5x faster than running 20 shell scripts in sequence" pitch
    (the factual "rules dispatch in parallel" remains in README)
  - The "strongest single piece of evidence for the launch
    positioning" sentence
  - The "Use it as the headline example on alint.org/examples" line
- **Borderline calls:**
  - The "12 scripts - direct replacements" / "17 of 50 scripts" /
    "Net: 17 of 50 scripts can move to one declarative file"
    counts are FACTUAL coverage stats and stay in the README. The
    "Replacing those 20 shell scripts with one declarative config +
    one alint check invocation in CI is the headline win" sentence
    surrounding the same numbers was promotional and moved out

---

## microsoft-typescript

- **Narrative:** structural-floor (eslint + dprint + knip already
  tight; alint adds the structural floor)
- **Rules:** 68 (~22 custom + 6 bundled rulesets)
- **alint.org page:** `src/content/case-studies/microsoft-typescript.md`
- **README line count:** 352 -> 330 (-22)
- **README marketing-pointer:** added under top H1
- **Recasts performed:**
  - The Summary's "exactly the kind of target the launch-prep
    validation pass wants to lint against" + "The headline outcome is
    *not* 'alint replaces N shell scripts' ... The headline is **alint
    adds structural checks TypeScript doesn't enforce today** ... For
    the launch story, this is the 'stable, famously meticulous repo'
    data point" — rewritten to a neutral "The existing
    structural-validation surface is therefore a *frozen snapshot*"
    plus a factual rule-count + new-gates summary. The "headline is"
    framing + "stable, famously meticulous repo" data-point pitch
    moved to alint.org as the Headline catch
- **Sections removed:**
  - "## Recommendation for the launch story" entirely — "TypeScript
    is the most-watched JS-tooling repo on GitHub", "Position it as
    the third tile on alint.org/examples", and the angle quote ("for
    repos that already have their lint house in order, alint adds
    the structural floor under their existing tools") all moved out
- **Sections preserved:**
  - Summary (rule counts, mapping breakdown - engineering)
  - Existing tooling inventory (Hereby tasks, scripts/*.mjs,
    workflows, eslint custom rules, package.json, tsconfig,
    baselines, CONTRIBUTING/AGENTS - all tables)
  - Starter alint config (rule-by-rule walkthrough)
  - What needs new alint primitives (gap catalogue with
    `pair_count` + `bundled_size_diff`)
  - What's out of alint's scope (categorised list)
  - Performance comparison (placeholder, lightly trimmed of
    "Where alint shines on TS specifically" framing tail)
  - Followup feature work surfaced (consolidated) - heading
    simplified, content preserved
  - Future analysis
  - Validation status (2026-05-07) footer
- **Marketing material moved to alint.org:**
  - "Famous, frozen, meticulously curated" data-point framing
  - "TypeScript is the most-watched JS-tooling repo on GitHub.
    Naming it as a target gives alint instant credibility with the
    JS audience" credibility framing
  - "Easy 'alint caught X the existing tooling missed' anecdote"
    line
  - The "Position it as the third tile" pitch with the
    structural-floor angle quote
- **Borderline calls:**
  - The performance section's "Where alint shines on TS
    specifically: the **baseline file-size guard** runs against 53k
    files in tens of milliseconds" was reframed without the "shines"
    verb but kept the factual measurement. The "cross-cutting
    structural checks pay back the most" tail was retained as a
    factual cost statement, not promotional
  - The "frozen snapshot — exactly the kind of target the launch-prep
    validation pass wants to lint against" sentence had a factual
    half (frozen snapshot) and a positioning half (launch-prep
    target). Kept the factual half

---

## microsoft-vscode

- **Narrative:** script-sprawl (apples-to-apples vs build/hygiene.ts)
- **Rules:** 67 (~37 custom + 6 bundled rulesets)
- **alint.org page:** `src/content/case-studies/microsoft-vscode.md`
- **README line count:** 752 -> 692 (-60)
- **README marketing-pointer:** added under top H1
- **Recasts performed:**
  - The Summary's opening framing ("the canonical 'every developer's
    editor' repo - **~160k stars, top-watched OSS desktop application
    on GitHub**, and the alint case study with the **highest direct
    apples-to-apples comparison surface of any P2 study to date**") was
    rewritten to a neutral statement: "ships a custom hygiene-check
    script (`build/hygiene.ts`) that does structurally what alint is
    designed to do - making this the case study with the most direct
    apples-to-apples comparison surface in the catalogue."
  - The "## Headline finding" pull-quote (the load-bearing "alint is
    what `build/hygiene.ts` would look like as a tool, not a per-repo
    script" launch claim) was renamed to "## Headline coverage" and
    rewritten to be a measurement-only statement: "**`build/hygiene.ts`
    coverage: 6 of 8 hygiene-pipeline stages (75%)** covered
    declaratively...". The full launch claim moved to the alint.org
    page as the Headline catch
  - The hygiene-script analysis tail's "**vscode is uniquely
    positioned for the launch pitch because the target it competes
    against is one well-defined script that any reader can audit in 5
    minutes**" was rewritten to a neutral catalogue-comparison
    statement
  - The eslint-plugin-local analysis tail ("This is the cleanest
    example in any P2 study", "**strengthens** the launch story's
    'complementary, not competing' framing", "second-most-watched OSS
    TS repo on GitHub") was tightened to a factual "every in-tree
    rule is correctly placed in eslint rather than alint" plus a
    factual statement that vscode maintains 45 in-tree eslint rules
    alongside the hygiene script
- **Sections removed:**
  - "## Recommendation for the launch story" (~75 lines) — the
    flagship-visibility framing, "if alint can replace 75% of what
    the most-watched developer-tools repo on GitHub maintains as a
    335-line custom hygiene script - it can replace most of yours
    too" pitch quote, the "Position it as the **flagship tile on
    alint.org/examples**" line, AND the entire 5-narrative
    positioning matrix (Replaces a custom in-tree hygiene script /
    Replaces N hand-rolled / Catches conventions / Adds a
    structural floor / Maturity is the hard test) — that matrix
    is positioning content, not engineering
  - The "alint pitch here is **not** speed - it's **legibility +
    LSP-driven adoption**" paragraph and the "adopt alint to express
    the hygiene-script invariants declaratively" angle quote in the
    Performance section
- **Sections preserved:**
  - Summary (rule counts, 34 surfaces inventoried, mapping
    breakdown - engineering)
  - `build/hygiene.ts` analysis (the 8-stage table with line
    references and coverage status - engineering apples-to-apples
    comparison)
  - `.eslint-plugin-local/` custom rules analysis (the 45-rule
    sample table)
  - Existing tooling inventory (the long tables across hygiene.ts,
    filters.ts, eslint-plugin-local, workflows, package.json,
    tsconfig matrix, Component Governance triple, tsfmt,
    .editorconfig, .gitattributes, vscode-dts/)
  - Maps to alint (~17 surfaces) table - engineering
  - Needs new alint primitive (gap catalogue with the canonical
    `cross_file_value_equals` motivating example)
  - Out of alint's scope
  - Already covered by other linters
  - Performance comparison (placeholder, lightly trimmed of pitch
    framing)
  - Followup feature work surfaced (priority order) - heading and
    content preserved
  - No NEW schema/language pitfalls hit
  - Future analysis
  - Validation status (2026-05-07) footer
- **Marketing material moved to alint.org:**
  - The "alint is what `build/hygiene.ts` would look like as a tool,
    not a per-repo script" launch claim - this is now the canonical
    Headline catch on the alint.org page
  - The "if alint can replace 75% ... it can replace most of yours
    too" pitch quote
  - The "flagship-visibility data point" framing
  - The "highest direct apples-to-apples comparison surface of any
    P2 study" optic (the FACTUAL "most direct apples-to-apples
    comparison surface in the catalogue" was preserved as a
    catalogue-relative descriptive line in the README - this was a
    borderline call, see below)
  - The 5-narrative positioning matrix
  - The "second-most-watched OSS TS repo on GitHub" credibility line
- **Borderline calls:**
  - The Validation status footer's "Apples-to-apples target" bullet
    had a tail clause ("This is the strongest 'alint replaces a
    hand-rolled script' data point in the case-study catalogue.")
    which was promotional. **Stripped that tail** and kept just
    the measurement (6 of 8 stages, 75%, the 2 deferred stages
    listed by name)
  - The Validation status footer's "Open gaps" line about
    `cross_file_value_equals` had a "vscode is the
    flagship-visibility consumer of the 10" framing - rewritten to
    "vscode's `checkCopilotEnginesVersion` is one of the 10"
    (factual)
  - The Summary's reframed line "the case study with the most direct
    apples-to-apples comparison surface in the catalogue" is a
    catalogue-relative claim that was borderline - left in because
    it factually orients the reader to the engineering content of
    the case study (this README's whole point is the apples-to-apples
    comparison) and matches the reframing in batch 3's flutter
    where catalogue-relative descriptive lines were retained when
    they preface engineering content
  - The 75% (6 of 8) hygiene.ts coverage measurement is engineering
    and stays in the README per task instructions; the "alint is
    what build/hygiene.ts would look like as a tool" quote moved to
    alint.org per task instructions

---

## nixos-nixpkgs

- **Narrative:** polyglot-wins (the scale-validation flagship)
- **Rules:** 79 (~46 custom + 4 bundled rulesets)
- **alint.org page:** `src/content/case-studies/nixos-nixpkgs.md`
- **README line count:** 709 -> 658 (-51)
- **README marketing-pointer:** added under top H1
- **Recasts performed:**
  - The Summary's opening "NixOS/nixpkgs is **the SCALE-STRESS data
    point** in alint's case-study catalogue" was reframed to a
    neutral "the **scale-stress data point** in the catalogue" with
    the "answers two launch-relevant questions" line tightened to
    "exists in this catalogue to answer two questions". The two
    questions themselves are engineering scope and stay
  - The "**Headline finding:** ... confirming alint scales gracefully
    to the largest non-trivial OSS monorepo on GitHub. nixpkgs is
    **the case where alint's 'any size repo' pitch becomes
    defensible by measurement**: the `for_each_dir` primitive is
    not the bottleneck adopters at this scale need to fear" paragraph
    was rewritten to "**Headline measurement:** at 39 101 files...
    273 ms wall-clock - *under half the wall-clock budget of a
    single Nix evaluation*. The `for_each_dir` primitive is not the
    bottleneck at this scale." (factual measurement only)
  - The "Scale notes" preamble ("This is the section the entire P2b
    SCALE-STRESS exercise exists to populate") was tightened to a
    factual "Each candidate concern from the SCALE-STRESS exercise
    was tested empirically:"
  - The "Notes for the parent agent" benchmark bullet's "This is the
    load-bearing data point for the launch-pitch 'alint scales to
    any size repo' claim" tail was stripped, keeping just the
    measurement
  - The Validation status "Live-tree headline (load-bearing)" bullet
    with its "This is the empirical anchor for alint's 'any size
    repo' pitch" tail was rewritten to "Live-tree measurement:" with
    the factual measurement only
- **Sections removed:**
  - "## Recommendation for the launch story" (~50 lines) — the
    "launch-pitch's 'scales to any size repo' anchor" framing, the
    "Position it as the **scale-stress tile** on alint.org/examples"
    pitch with its angle quote (*"NixOS/nixpkgs has 20 678 by-name
    package directories...without per-repo perf tuning."*), the
    "instant credibility as a tool that handles scale" sentence,
    the "any size repo" pitch validation language, and the by-name
    findings pitch ("The pitch lands harder when paired with the
    by-name finding..."). Replaced with a plain "## Followup
    feature work surfaced (consolidated, sorted by strength of
    demand across P2a + P2b)" heading directly above the rule-kind
    candidate list (engineering content)
- **Sections preserved:**
  - Summary (rule counts, file/dir counts, mapping breakdown -
    engineering)
  - Scale notes (all 7 sub-sections - empirical findings on
    `for_each_dir`, `for_each_file`, scope_filter discipline,
    paths "**/*" content rules, JSON Schema editor-LSP, speculative
    concerns, real concerns flagged for v0.10 LSP-server design)
  - Existing tooling inventory (all surfaces: top-level orchestration,
    `ci/`, treefmt umbrella, maintainer + license + team registries,
    `lib/` + `lib/tests/`, `pkgs/by-name/<2-letter>/<pkg>/`,
    `.github/`, hygiene)
  - What maps to existing alint rules (rule-count breakdown)
  - What needs new alint primitives (gap catalogue)
  - What's out of alint's scope
  - Already covered by other linters
  - Performance comparison (the table with the alint 0.273s vs
    nix-build parse vs treefmt vs nixpkgs-vet wall-clocks - this
    is engineering measurement and stays; the "**~100x faster**"
    framing was already factual against the table data so was
    retained)
  - Notes for the parent agent (audit pass status, run-against-cloned-
    tree results, by-name silently-passes finding)
  - Future analysis
  - Validation status (2026-05-07) footer
- **Marketing material moved to alint.org:**
  - The "any size repo" pitch in all forms - both as the explicit
    "alint scales to any size repo" pitch claim and as the
    surrounding "load-bearing" / "empirical anchor" framing
  - The "scale-stress tile" pitch with its angle quote
  - The "instant credibility as a tool that handles scale" line
  - "alint complements rather than replaces nixpkgs's existing
    tooling" framing (the FACTUAL boundary description stays as the
    "What's out of alint's scope" + "Already covered by other
    linters" sections; the headline framing moved out)
  - The "by-name finding pitch lands harder when paired with..."
    paragraph
- **Borderline calls:**
  - **Per task instructions** - the FACTUAL "39,101 files /
    20,678 by-name dirs / 273 ms wall-clock" measurement was kept
    in the validation status footer (engineering); the
    "scale-validation flagship - 'any size repo' empirically
    defensible" framing moved (positioning). Done as instructed
  - The Validation status "Live-tree headline (load-bearing)"
    bullet was renamed to "Live-tree measurement" — the
    "(load-bearing)" parenthetical was promotional metadata and
    was stripped
  - The Performance comparison's "Key observation: alint's 0.273 s
    is **~100x faster** than the fastest..." was retained as a
    factual quantitative comparison against the table data (the
    headline pitch claim "alint is the fastest fail signal" was
    softened to a measurement comparison). This is the same call
    pattern as in nodejs/node and the prior batches: factual
    speedup statements stay; pitch framing around them moves
  - The two pitfalls flagged for the v0.10 LSP-server design
    (per-keystroke re-evaluation cost, result-cache invalidation)
    are engineering forward-design notes and stay in the README

---

## nodejs-node

- **Narrative:** convention-without-checks (15-year-old conventions
  enforced via human review only); secondary "maturity is the hard
  test" framing
- **Rules:** 86 (~40 custom + 5 bundled rulesets)
- **alint.org page:** `src/content/case-studies/nodejs-node.md`
- **README line count:** 569 -> 524 (-45)
- **README marketing-pointer:** added under top H1
- **Recasts performed:**
  - The Summary's opening "the canonical mature C++/JS hybrid mega-
    repo - **~15 years of accumulated convention discipline**, with
    structural validation scattered across the broadest surface of
    any P2a-Wave 3 case study" was reframed to "a mature C++/JS
    hybrid repo with ~15 years of accumulated convention discipline
    and structural validation scattered across the broadest surface
    in the catalogue." The "canonical" framing softened to factual;
    the P2a-Wave 3 internal-categorisation reference removed
  - The Summary's "The 43% that *do* fit translate to the **40-rule
    alint config** ... The two single most alint-shaped surfaces
    are:" preamble was rewritten to "Two surfaces fit alint
    particularly cleanly because they are enforced statically
    nowhere today:" (neutral, factual)
  - The two-headline-finding bullets ("**Enforced nowhere
    statically today**...") had their `**Enforced nowhere
    statically today**` boldface framing softened to plain factual
    statements; the "alint encodes the grammar as a 6-line
    `filename_regex` rule" measurements stayed
- **Sections removed:**
  - "## Recommendation for the launch story" (~70 lines) — the
    "Headline launch quote", the "**fourth positioning narrative**
    crystallised in P2a-Wave 3" framing, the "doubles down on the
    **maturity** angle that complements typescript and cpython"
    pitch, the entire 4-narrative positioning matrix (Replaces N
    hand-rolled / Catches conventions / Adds a structural floor /
    Maturity is the hard test), the "node sits at the **intersection
    of all four**" paragraph, and the "we sit beneath your existing
    linters as the structural-orchestration layer..." pitch quote.
    Replaced with a plain "## Followup feature work surfaced
    (priority order)" heading directly above the rule-kind candidate
    list (engineering content)
  - The Performance comparison's "alint pitch here is **not** speed
    - it's **inventory legibility**" paragraph and the "adopt alint
    to consolidate the orchestration layer so contributors can read
    the structural contract in one file" pitch quote
- **Sections preserved:**
  - Summary (rule counts, 44 surfaces inventoried, mapping breakdown,
    the two filename_regex headline findings - engineering)
  - Existing tooling inventory (Makefile targets,
    `tools/eslint-rules/` 27 rules, `tools/lint-*.mjs`,
    `tools/find-inactive-*.mjs`, `tools/test.py`, `.gitattributes`,
    `.editorconfig`, `.cpplint`, `pyproject.toml`, workflows,
    `eslint.config.mjs` partials, `src/node_version.h`,
    `tools/lint-md/package.json`,
    `lib/internal/per_context/primordials.js`,
    `doc/changelogs/`, `doc/contributing/` - all tables)
  - What needs new alint primitives (gap catalogue including the
    `cross_file_value_equals` + `registry_paths_resolve` v0.10
    ship-targets and the new `file_header_consistency` candidate)
  - Out of alint's scope
  - Already covered by other linters
  - Starter alint config (rule-by-rule walkthrough)
  - Performance comparison (placeholder, lightly trimmed of pitch
    framing)
  - Followup feature work surfaced (priority order) - heading and
    content preserved
  - No NEW schema/language pitfalls hit
  - Future analysis
  - Validation status (2026-05-07) footer
- **Marketing material moved to alint.org:**
  - The "Headline launch quote" with its full surface-count brag
    sentence
  - The "fourth positioning narrative crystallised in P2a-Wave 3"
    internal categorisation
  - The "doubles down on the **maturity** angle" pitch
  - The 4-narrative positioning matrix (factual narrative names +
    use-cases moved to alint.org's narrative-tagging metadata; the
    matrix table itself moved out)
  - The "node sits at the **intersection of all four**" pitch
    paragraph and its full pitch quote ("we sit beneath your
    existing linters...")
  - The "alint pitch here is **not** speed - it's **inventory
    legibility**" framing
- **Borderline calls:**
  - The two "Enforced nowhere statically today" headline-finding
    bullets in the Summary contain a FACTUAL claim (the convention
    really is enforced statically nowhere) plus a load-bearing
    boldface that signals positioning weight. Kept the factual
    claim, dropped the boldface emphasis. The full
    "headline-launch-quote" treatment of the two findings moved to
    alint.org as the Headline catch
  - The "Maturity is the hard test" narrative tag is the headline
    framing for the alint.org page (and matches the brief's hint
    about node's narrative being "convention-without-checks" with a
    maturity overlay). Both tags surfaced in the original README; the
    primary alint.org narrative chosen was "convention-without-checks"
    (matching the brief), with maturity surfaced in the Why-this-
    matters / Where-alint-earns-its-keep prose
  - The factual "44 distinct structural-validation surfaces" count
    stays in the README (engineering); the "broadest surface alint
    has linted in any case study" framing was a catalogue-relative
    claim that was retained as a descriptive orient-the-reader line
    (same call as flutter / vscode in prior batches)

---

## Cross-cutting patterns observed

- **The "## Recommendation for the launch story" section was
  uniformly removable across all 5 READMEs.** Every one was a
  positioning pitch + (in 3 of 5: vscode, nodejs-node, microsoft-
  typescript) a tile-positioning sentence + (in 2 of 5: vscode,
  nodejs-node) a multi-narrative comparison matrix. None of these
  were engineering content; all moved to alint.org cleanly. Same
  pattern as batches 2 + 3
- **The "Headline finding" pull-quote pattern** — both vscode and
  nodejs-node carried a load-bearing pull-quote at the top of the
  Summary that was 50% factual + 50% positioning. Resolution:
  rewrite to factual measurement only, lift the positioning quote
  to alint.org as the Headline catch (this is the canonical
  recasting pattern surfaced across batches 2 + 3 + 4)
- **The Validation status footer is the trickiest section to
  audit** — it carries factual measurements (which stay) but also
  picks up promotional adjectives ("load-bearing", "headline",
  "empirical anchor", "flagship-visibility consumer", "strongest
  data point in the catalogue") that creep in across drafts. All 5
  READMEs needed light footer copy-edits to strip these adjectives
  while preserving the underlying numbers
- **Catalogue-relative descriptive lines** ("the case study with
  the most direct apples-to-apples comparison surface in the
  catalogue", "the broadest surface alint has linted") sit on the
  borderline. The call applied here (consistent with batch 3): when
  the line factually orients the reader to engineering content the
  README is about, leave it. When it's pitch framing for a launch
  quote, move it
- **`for_each_dir` / `for_each_file` data points are engineering
  scale measurements** even when they read pitch-y in context (e.g.
  nixpkgs's 273 ms / 20,678 directories). The measurements stay;
  only the surrounding "any size repo" pitch language moves
- **"alint complements rather than replaces existing tooling"
  framing** is a positioning frame. The factual boundary
  ("What's out of alint's scope" + "Already covered by other
  linters" sections) is engineering. Same pattern across batches:
  the boundary tables stay; the framing-headline moves
- **Apples-to-apples comparison surfaces (vscode's
  `build/hygiene.ts`, kubernetes' `hack/verify-*.sh`, nodejs/node's
  Makefile + custom eslint rules)** are the case studies where the
  alint.org "Headline catch" carries the most weight. The README's
  job is the engineering inventory; the alint.org page's job is the
  one-sentence pitch a reader can quote

## Borderline calls (consolidated)

- **vscode's "alint is what build/hygiene.ts would look like as a
  tool, not a per-repo script" claim** — this is the canonical
  "load-bearing positioning quote" of batch 4. Per task brief,
  moved to alint.org. The factual 75% (6 of 8) coverage measurement
  stayed in the README per brief. Done as instructed
- **nixpkgs's "scale-validation flagship — 'any size repo'
  empirically defensible" framing** — moved per brief. The 39,101
  files / 20,678 by-name dirs / 273 ms wall-clock measurement
  stayed per brief. Done as instructed
- **node's "Maturity is the hard test" tag** — surfaced as the
  fourth positioning narrative on the alint.org page tagged under
  the primary "convention-without-checks" narrative (matching the
  brief's hint). The matrix table itself moved to alint.org;
  the factual "intersection of all four narratives" observation
  was reframed in alint.org's prose, not as a matrix
- **Catalogue-relative orient-the-reader lines** — kept when they
  factually orient the reader to engineering content (vscode's
  "most direct apples-to-apples comparison surface in the
  catalogue", node's "broadest surface in the catalogue"). Moved
  out when they were pitch framing for a launch quote (vscode's
  "**flagship-visibility data point**" — different shape, moved)

## Blockers

None. All 5 READMEs revalidate cleanly; all 5 alint.org case-study
pages parse against the `caseStudies` schema (`title`, `repo`,
`headline`, `narrative` from the 7-value enum, `rules` integer,
`lastValidated: 2026-05-07`). No schema or content-config changes
required. No commits or pushes performed per brief.
