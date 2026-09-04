# Design doc: Concepts section redesign (content + animated diagrams)

Status: Draft.
Decisions: none required (a docs content + technique change; references ADR-0005
architecture-diagrams and ADR-0007 release-aware docs). See Open questions for
whether the animated-diagram convention warrants its own ADR.
Demand evidence: maintainer review of the live `alint.org/docs/concepts/` section
(shallow, several stale errors, inconsistent depth, no animated diagrams), plus a
three-part audit (current-state critique, a code-verified concept map, and a
diagram-technology assessment) summarized in section 2.

Scope note: this is not a rule-kind design doc, so it adapts the seven-section
template rather than following it verbatim.

## 1. Problem

The `Concepts` section of the docs is the place a new user goes to build a mental
model of alint. Today it does that poorly.

- **It is misclassified.** The section is a flat list of 9 pages that mixes three
  altitudes: true mental-model concepts (`how-it-works`, `walker-and-gitignore`),
  narrow config-field references filed as concepts (`templates`, `drop-ins`,
  `content-from`, `variable-interpolation`), and command/workflow pages
  (`suggest`, `baseline`). Only 2 to 3 of the 9 are concepts everyone should
  grasp. `content-from` is the extreme case: one fix-op field elevated to a
  top-level "concept."
- **The concepts that most deserve a page have none.** There is no dedicated page
  for the `when:` / facts language, for composition and the `extends:` trust
  boundary, or for `scope_filter` -- three of alint's defining ideas. Meanwhile
  `configuration/index.md` explicitly promises the reader a `when:` language
  "explained in depth" in Concepts, and no such page exists.
- **It carries stale errors that describe shipped features as unbuilt** (full list
  in section 7). Examples: `monorepos.md` calls `--changed` and `when_iter:` "on
  the v0.5 roadmap"; `walker-and-gitignore.md` calls `git_commit_message` and
  `git_no_denied_paths` "still pending." All three ship today.
- **Depth and diagrams are uneven.** `walker-and-gitignore` and `baseline` are
  deep, table-rich teaching pages; `content-from` is a padded one-field stub;
  `index` is a cheat-sheet that re-documents the 8 output formats the reference
  already owns. Only 3 of 9 pages carry a diagram, all static LikeC4 web
  components. There is no animation anywhere, and no shared visual language for
  concept diagrams.
- **Literal IA defects** confirm the drift: two pages both declare
  `sidebar.order: 9` (tie broken alphabetically), and orders 3 and 4 are unused.

The result is a section that under-teaches the ideas that make alint distinctive
while over-indexing on field-level trivia, and that is visually flat.

## 2. What the audit found (evidence)

Three read-only audits ran against the current tree (workspace v0.16.1).

**Current-state critique.** Page-by-page: `how-it-works`, `walker-and-gitignore`,
`baseline`, `templates`, `drop-ins`, and `variable-interpolation` are actually
well-written; `content-from` and `index` are weak; `suggest` is a command page.
The core problems are misclassification, the stale-roadmap errors, uneven depth
and diagram coverage, and the three gap pages. Full error list folded into
section 7.

**Concept map (code-verified).** The user-facing concepts, classified CORE
(irreducible mental model) vs FEATURE (opt-in), with exact counts read from
`facts.json`: **94 distinct rule kinds + 11 aliases = 105 identifiers**; **13
families** (physical, one kind to one family); **13 categories** (same vocabulary
but many-to-many -- 32 kinds sit in more than one); **22 bundled rulesets**; **12
auto-fix ops**; **8 output formats**; **12 subcommands**; **6 fact predicates**;
**24 structured-query kinds** (8 formats x 3 ops). The map also surfaced three
source-doc inaccuracies the rewrite must not repeat (section 7, "source docs").

**Diagram technology.** Current diagrams are LikeC4 web components (40 views,
lazy-loaded, theme-synced) embedded as `<likec4-view>` in the synced Markdown,
plus two static d2-rendered SVG heroes; Mermaid is GitHub-only; the `amorph`
animated engine is private and unbuilt (Phase 0), and public alint is
contractually "vendored static SVG only." The recommended path for animated
concept diagrams (section 6) is hand-authored inline SVG animated with CSS, which
ships today with no build, CSP, dependency, or `.mdx` changes and survives the
verbatim docs sync.

## 3. Goals and non-goals

**Goals.**
- Make `Concepts` teach the ideas that make alint distinctive, in a deliberate
  order, at a consistent depth, with a worked example on every page.
- Give every concept one elegant animated diagram in a shared visual language.
- Reclassify: `Concepts` holds only mental-model concepts; feature and command
  pages move to `Configuration`, `Reference`, or `Guides`.
- Fix every stale error found in the audit.

**Non-goals.**
- Not rewriting `Configuration` or `Reference` beyond receiving the moved pages
  and one gap-filling cross-link.
- Not building the `amorph` runtime engine (the inline-SVG approach is
  forward-compatible with it; section 6).
- Not changing any engine, rule, or config behavior. This is docs only.

## 4. The concept inventory (what a reader must learn)

Condensed from the concept map. This is the authoritative list the new section is
built from. CORE = everyone; FEATURE = opt-in.

| # | Concept | Class | Currently documented as |
|---|---|---|---|
| 1 | The execution pipeline (parse, walk, dispatch, evaluate, report; determinism) | CORE | `how-it-works` (good) |
| 2 | The config model: one `.alint.yml`, the rule record (`id`/`kind`/`level`/`paths`/`when`/`fix`/`message`) | CORE | scattered in `index` |
| 3 | Kinds vs families vs categories | CORE | not explained; conflated |
| 4 | Severity and exit codes | CORE | one line in `index` |
| 5 | The walker + `.gitignore`; walked-tree vs git-index; `git_tracked_only` | CORE | `walker-and-gitignore` (good, overloaded) |
| 6 | Scoping: `paths:` globs, `when:` fact gates, `scope_filter:` (ancestor / changed / manifest) | CORE | gap; only in dense config prose |
| 7 | `--changed` fast path (and why cross-file rules stay whole-tree) | FEATURE | buried in `walker-and-gitignore` |
| 8 | Composition + the `extends:` trust boundary (spawn gate, SRI, confinement) | FEATURE | one paragraph in `index` |
| 9 | Bundled rulesets (fact-gated; the on-ramp) | FEATURE | scattered |
| 10 | Cross-file rules (`file_graph`, `cross_file`, `extract:`) | FEATURE | rules reference only |
| 11 | Structured-file queries (JSONPath over 8 formats into one tree) | FEATURE | rules reference only |
| 12 | Fixers (12 ops, `content_from:`, mutating vs manual) | FEATURE | `content-from` (too narrow) |
| 13 | Baseline mode (fingerprint diffing; adoption) | FEATURE | `baseline` (good) |
| 14 | Config layering: drop-ins, nested configs, interpolation timing | FEATURE | 3 separate thin pages |
| 15 | The agent surface (`agent` format, `export-agents-md`, agent rulesets) | FEATURE | not a concept page |
| 16 | Templates (reusable rule shapes) | FEATURE | `templates` (a config feature) |
| 17 | `suggest` (propose rules) | FEATURE | `suggest` (a command) |

**Deliberately not concept pages** (they are real, but they are field/command
references, not mental models; the rewrite cross-links to them rather than
absorbing them): `allow_out_of_root:` and `fix_size_limit:` (top-level config
fields, to Configuration); the `# yaml-language-server: $schema=` editor pragma and
the report JSON schemas (to Integrations / Reference); and the config-independent
`rules` catalog vs the config-scoped `list` (ADR-0009, a CLI concern, to CLI
reference).

## 5. Proposed information architecture

Redo the whole section (the maintainer's preferred option). `Concepts` becomes a
deliberately ordered, grouped set of mental-model pages; features and commands
move to their proper homes. Every concepts page follows one shape: a one-sentence
thesis, an animated diagram, a narrative, a worked `.alint.yml` example with the
resulting report line, and a "going deeper" link.

### 5.1 New `Concepts` section

Grouped into sub-sections (progressive: mental model, then targeting, then
composition, then multi-file, then adoption). The grouping is a cross-repo change,
not just page frontmatter; see 5.4 for the mechanism.

**Start here (the mental model)**
- `index.md` -- Concepts hub. Lean: the one-screen mental model + the hero
  pipeline animation + a map of the section. Delegates all reference lists.
- `how-alint-works.md` -- the execution pipeline; determinism; read-coalescing.
- `the-config-model.md` -- one `.alint.yml`; the rule record; `version: 1`.
- `kinds-families-categories.md` -- the three axes; the 94+11 / 13 / 13 counts.
- `severity-and-exit-codes.md` -- `level:` to exit code; `off`; `--fail-on-warning`.
  (Short; could merge into the config model -- see Open questions.)

**How rules target files**
- `the-walker-and-git.md` -- discovery, `.gitignore`, walked-tree vs git-index,
  `git_tracked_only`.
- `scoping.md` -- `paths:` globs, `when:` fact gates, `scope_filter:` (ancestor /
  changed / manifest-derived), and the gate order.
- `changed-mode.md` -- the `--changed` fast path and cross-file correctness.

**Composition and trust**
- `composition-and-trust.md` -- `extends:` field-merge, bundled rulesets, the
  trust boundary (spawn gate, SRI, path confinement).
- `config-layering.md` -- how one effective config is assembled from drop-ins
  (`.alint.d/`), nested configs, and the three interpolation timing layers.

**Beyond single files**
- `cross-file-rules.md` -- relational verdicts; `file_graph`; `cross_file`;
  `extract:`.
- `structured-queries.md` -- JSONPath over 8 formats into one tree; the
  stringly-typed and cardinality footguns.

**Adoption and fixing**
- `fixing.md` -- the 12 fix ops; `content_from:`; mutating vs `stages:[manual]`.
- `baseline.md` -- baseline mode; fingerprint diffing.

**Working with agents**
- `the-agent-surface.md` -- `agent` output; `export-agents-md`; agent rulesets.

### 5.2 What moves out of `Concepts`

| Current page | New home | Why |
|---|---|---|
| `content-from.md` | folded into `fixing.md` (Concepts) + fix-op reference | a single field, not a concept |
| `templates.md` | `Configuration` (reuse/composition) | a config construct |
| `drop-ins.md` | folded into `config-layering.md` (Concepts) | a layering mechanic |
| `variable-interpolation.md` | timing folded into `config-layering.md`; full reference to `Configuration` | a config feature with one conceptual hook (timing) |
| `suggest.md` | `Guides` (or CLI reference) | a command workflow |

### 5.3 Before / after at a glance

- Before: 9 flat pages, ~3 concepts + 6 misfiled, 3 diagrams, 3 hard errors,
  duplicate/gap sidebar orders.
- After: ~14 grouped concept pages (each with a worked example and one animated
  diagram), feature/command pages relocated, all errors fixed, one visual
  language.

### 5.4 Sidebar and cross-repo mechanism

The current `Concepts` sidebar is one line in the alint.org `astro.config.mjs`:
`autogenerate: { directory: 'docs/concepts' }` (a flat list ordered by each page's
frontmatter `sidebar.order`). Starlight autogenerate cannot order or label
sub-groups; the config already notes this and hand-builds the Rules group for that
reason. So the proposed grouping is not achievable from the synced Markdown alone.
Two options:

- **(recommended) subdirectories + an explicit sidebar entry.** Author the pages
  under `docs/site/concepts/<group>/...` (so the URL carries the group) and replace
  the single `autogenerate` line with an explicit `Concepts` group whose items are
  the five sub-groups, mirroring how the Rules group is hand-built. Ordering and
  proper-case labels then work. Cost: the subdir choice sets the page URLs, so it
  must ship with the redirects in section 9.
- **(alternative) flat with `sidebar.order` bands.** Keep one directory and imply
  grouping with ordered bands and a label convention. No config change, but no real
  sub-group headers.

This makes the redesign a **cross-repo change**. Page content and diagrams are
authored in `alint/docs/site/concepts/` (synced verbatim to alint.org), but three
pieces are edits in the **alint.org** repo: the sidebar grouping
(`astro.config.mjs`), any shared `@keyframes` (section 6), and the redirects
(section 9, `public/_redirects`). The "no build / CSP / dependency change" claim in
section 6 is specifically about the *diagrams*; the *grouping* is a small
`astro.config.mjs` edit.

## 6. The animated-diagram system

**Technique (recommended).** Hand-authored inline SVG animated with CSS
`@keyframes`, authored directly in the synced `.md`. Rationale (from the diagram
audit): it renders today from plain Markdown through the verbatim docs sync with
no `.mdx` conversion, no component import, no head loader, no build step, no CSP
change, and no dependency; it themes automatically via Starlight tokens
(`var(--sl-color-*)` with literal fallbacks); reduced-motion is a one-line
declarative guard; and it degrades to a static final frame. SMIL was rejected (it
ignores `prefers-reduced-motion` in CSS); JS-island and small-lib options were
rejected (they force `.mdx` or a site-side head loader). This diverges from the
existing `animated-diagrams.md` plan (vendored static SVG from amorph); that
divergence is reconciled at the end of this section. The reference template and the
shared token kit are in Appendix 14.

**Relationship to the existing LikeC4 views.** The three concept pages that carry a
LikeC4 view today (`checkFlow`, `walkerFlow`, `templateFlow`) keep it. The animated
inline SVG becomes the *primary* teaching diagram at the top of each page (it
animates one idea the reader just read); the interactive `<likec4-view>` stays as a
"Going deeper" link (the explorable full model). The two are complementary, not
competing, and the full LikeC4 gallery in `about/architecture-diagrams.md` is
untouched.

**Shared visual language.** One small kit so the diagrams read as a system:
- Structure in neutral grays (`var(--sl-color-gray-*)`); the active/flowing
  element in the accent (`var(--sl-color-accent)`); violations in the danger hue.
- One motion vocabulary: a token travels a path to mean "one pass"; a dashed wire
  animates offset to mean "flow"; a element fades/scales to mean
  "matched/suppressed."
- Every diagram: `role="img"` + `<title>` + `<desc>`; a
  `@media (prefers-reduced-motion: reduce)` block that rests at the final frame;
  `max-width:100%`.
- A copy-paste template in the doc + optional shared `@keyframes` hoisted into
  alint.org `src/styles/custom.css` (the same split the site already uses for the
  LikeC4 loader). A GitHub-faithful static SVG fallback is optional per diagram
  (GitHub strips animated SVG, exactly as it strips `<likec4-view>` today).

**The diagram set.** One animated diagram per major concept page (thirteen below),
ranked by teaching value (the concept map's "hardest concepts" ranking). Short or
derivative pages (`severity-and-exit-codes` if kept standalone, and the relocated
feature pages) carry no diagram of their own or reuse a neighbor's:

1. **Execution pipeline** (hero, on `how-alint-works` + the hub): a token flows
   config -> walk -> dispatch -> report; parallel fan-out then a
   deterministic re-sort.
2. **Walker vs git index** (`the-walker-and-git`): filesystem -> `.gitignore`
   filter -> walked index, with a `git add -f`'d file slipping past and
   `git_tracked_only` re-narrowing.
3. **Trust boundary** (`composition-and-trust`): trust tiers (top-level vs
   extended vs bundled); a malicious `command` rule from an `extends:`'d ruleset
   bouncing off the spawn gate; SRI + confinement.
4. **Baseline diff** (`baseline`): current INTERSECT baseline -> suppressed / new
   / stale; a violation moves lines yet stays suppressed while a fresh one breaks
   through.
5. **Scope gates** (`scoping`): the order `when:` -> `paths:` -> `scope_filter:`
   -> `git_tracked_only` -> evaluate; the ancestor walk; `derive_target` mapping
   build-output back to source.
6. **`--changed` correctness** (`changed-mode`): the diff set filters per-file
   rules while cross-file/existence rules stay whole-tree.
7. **`file_graph`** (`cross-file-rules`): build the reference graph, then each
   `require:` mode (cycle, forbidden edge, orphan, dangling, fresh).
8. **8 formats into one tree** (`structured-queries`): json/yaml/toml/xml/...
   coerced into one Value tree that one JSONPath walks.
9. **Kinds / families / categories** (`kinds-families-categories`): one-to-one
   family nesting vs the many-to-many category overlay (a Venn/matrix reveal).
10. **Interpolation timing** (`config-layering`): `{{env}}` at load, `{{vars}}` at
    expansion, `{{ctx}}` per-violation, on one timeline.
11. **The config model / rule record** (`the-config-model`): the rule record's
    fields lighting up as they are matched and evaluated.
12. **Fix pass** (`fixing`): parallel evaluate, then the serial single-threaded
    fix mutation.
13. **Agent single source of truth** (`the-agent-surface`): active rules flow into
    `export-agents-md` -> `AGENTS.md` -> the agent reads it at session start; plus
    the `agent` output format's per-violation `fix_command`.

**Relationship to `animated-diagrams.md` and amorph.** The existing
`docs/design/animated-diagrams.md` breadcrumb states that alint's public docs
consume amorph's output as *vendored static SVG* (no runtime dependency), because
the amorph engine is private. Two facts change the calculus for the concepts pages:
amorph is at Phase 0 (unbuilt), so there is nothing to vendor yet; and the explicit
ask is *animated* diagrams, which a static vendored SVG does not deliver. Inline
SVG + CSS satisfies both -- it is animated (the ask) and takes no runtime
dependency (amorph's "no engine in public alint" principle; CSS animates
natively). This proposal therefore **revises** the "static only" stance of
`animated-diagrams.md` for the concepts pages. Forward path when amorph ships: the
page prose is unchanged and only the `<svg>` block is regenerated by amorph (as
animated SVG, or swapped for its `<amorph-anim>` element behind an alint.org
head-loader). The bridge's cost is re-generating the hand-authored SVGs later; the
alternative (wait for amorph) is rejected because amorph is far off and the docs
need the diagrams now (decision recorded in section 11). `animated-diagrams.md`
should get a one-line pointer to this doc.

## 7. Corrections (fold into the rewrite)

**Hard errors (shipped features documented as unbuilt).**
- `docs/site/about/monorepos.md:65` -- `--changed` called "on the v0.5 roadmap."
  It ships (documented in `walker-and-gitignore.md:122`, `README.md:249`).
- `docs/site/about/monorepos.md:24,64,170` -- `when_iter:` called "planned for
  v0.5." It ships on `for_each_dir` / `for_each_file` / `every_matching_has`
  (`schemas/v1/config.json:2223,2263,1534`).
- `docs/site/concepts/walker-and-gitignore.md:120` -- `git_commit_message` and
  `git_no_denied_paths` called "still pending." Both ship (`facts.json:59,63`;
  `docs/rules.md:463,453`).

**Minor / dated.**
- `docs/site/about/architecture-diagrams.md:108` -- template syntax shown as
  `${...}`; correct is `{{vars.X}}`.
- `docs/site/concepts/variable-interpolation.md:85,89` -- narrative pinned to
  "v0.11"; the `${VAR}` form is still deprecated (v1.0 removal), so reframe as
  "still works but deprecated" without the version pin.
- `docs/site/configuration/index.md:316` -- promises a `when:` language
  "explained in depth" in Concepts; the new `scoping.md` delivers it (fix the
  link target).

**Source-doc inaccuracies the concept map surfaced (fix so the rewrite inherits
the truth).** These live in design docs, not the concepts pages, but the rewrite
must reflect the code, not these:
- `.alintignore` does not exist. The walker honors `.gitignore` / `.ignore` /
  git-exclude + the config `ignore:` list only. (Wrong in `ARCHITECTURE.md:279`.)
- Facts are evaluated sequentially (a `for` loop), not in parallel; "cached"
  refers to LSP reuse. (Overstated in `ARCHITECTURE.md` and ADR-0010.)
- The `when:` language has no `ctx.` namespace (namespaces are
  `facts.`/`vars.`/`iter.`/`env.`); `{{ctx.*}}` is valid only in message
  templates. (Wrong in `ARCHITECTURE.md` DSL listing.)

**IA defects.** Remove the duplicate `sidebar.order: 9`
(`baseline.md`/`variable-interpolation.md`) and the 3-4 gap by re-numbering the
whole section under the new grouping.

## 8. Implementation plan (phased)

1. **Errata hot-fix (independent, shippable first).** Fix the section 7 hard
   errors in place so users stop reading shipped features as unbuilt. Small,
   low-risk, no restructure. Can land before the redesign.
2. **Diagram kit.** Add the copy-paste inline-SVG template + shared `@keyframes`
   (in alint.org `custom.css`), the visual-language tokens, and the
   reduced-motion/`<title>`/`<desc>` conventions. Build the hero pipeline diagram
   first as the reference implementation.
3. **Core mental-model pages.** Rewrite `how-alint-works`; add `the-config-model`,
   `kinds-families-categories`, `severity-and-exit-codes`; each with its diagram
   and worked example.
4. **Targeting + composition pages.** Rewrite `the-walker-and-git`; add `scoping`,
   `changed-mode`, `composition-and-trust`, `config-layering`.
5. **Multi-file + adoption pages.** Add `cross-file-rules`, `structured-queries`;
   rewrite `fixing` (absorbing `content-from`) and `baseline`; add
   `the-agent-surface`.
6. **Relocations + redirects.** Move `templates` to Configuration, `suggest` to
   Guides; retire `content-from`/`drop-ins`/`variable-interpolation` as standalone
   concepts (content folded), leaving redirects (section 9). Renumber the sidebar.
7. **Source-doc corrections.** Apply the ARCHITECTURE.md / ADR errata (section 7).

Each phase is a reviewable PR; content lives in `alint/docs/site/`, so it syncs to
alint.org through the normal `docs-export` + `docs-bundle` pipeline (ADR-0007). No
release is required for doc-only pages (the docs-bundle rebuilds from a main
worktree).

## 9. Migration and redirects

Moving/retiring pages changes URLs (`/docs/concepts/templates/`,
`/content-from/`, `/drop-ins/`, `/variable-interpolation/`, `/suggest/`). Add
redirects on the alint.org side (the site already ships `public/_redirects`, served
by its Cloudflare Worker) to the new homes so external links and search results do
not 404. Update in-repo
cross-links (notably `configuration/index.md:316`). Verify with the site's
existing link/head-parity checks before deploy.

## 10. Effort and sequencing

Rough order of magnitude: ~14 concept pages (about 6 net-new, ~5 rewrites, ~3
relocations) plus ~13 animated diagrams, and one small alint.org change (the
sidebar grouping in 5.4 plus the redirects in 9). The errata hot-fix (phase 1) is an
afternoon. The diagram kit + hero (phase 2) de-risks the technique. Phases 3-5
are the bulk and can be parallelized per group. Phases 6-7 are mechanical.
Recommend landing phase 1 immediately, then phases 2-7 as a small series of PRs.

## 11. Open questions

1. **Does the animated-diagram convention warrant an ADR?** It is an additive,
   reversible docs technique (not an architectural decision), and ADR-0005 already
   covers the diagram program. Recommend: no ADR; note the technique in
   `architecture-diagrams.md` / `animated-diagrams.md` instead. Confirm.
2. **`severity-and-exit-codes`: standalone or folded into `the-config-model`?**
   It is short. Recommend folding, with exit codes as a callout, unless we want a
   linkable page for CI docs to point at.
3. **`changed-mode`: its own page or a section of `the-walker-and-git`?** It is a
   distinct idea (git diff + cross-file correctness) but small. Recommend its own
   short page for linkability; open to folding.
4. **`baseline` and `the-agent-surface`: Concepts or Guides?** Both carry a real
   mental model but are also workflows. Recommend keeping them in Concepts (they
   are differentiators) and adding a task-oriented Guide that links to them.
5. **Static-SVG fallbacks for GitHub.** Author every animated diagram with a
   committed static SVG twin (as the two landing heroes do), or accept that
   `docs/site/*.md` viewed on GitHub shows no diagram (consistent with
   `<likec4-view>` today)? Recommend: accept it for most; add a static twin only
   for the hero.
6. **Shared `@keyframes` location.** In alint.org `custom.css` (site-owned, one
   source) vs inlined per diagram (self-contained, survives to GitHub). Recommend
   a hybrid: inline per diagram for portability, with the option to hoist if
   duplication grows.
7. **Bridge now vs wait for amorph (recommend: bridge).** Hand-author inline SVG +
   CSS now and regenerate via amorph when it ships, vs wait for amorph to produce
   the diagrams. Recommend bridging: amorph is Phase 0, the docs need diagrams now,
   and the prose does not change on migration. Confirm, and add a pointer from
   `animated-diagrams.md` to this doc.
8. **Does "Bundled rulesets" deserve its own page?** It is the primary on-ramp
   (`alint init` writes the `extends:` lines) and the Repolinter-migration target,
   yet 5.1 folds it into `composition-and-trust`. Recommend a short dedicated
   `bundled-rulesets.md` concept page (the catalog, how `init` uses it, and
   fact-gating), which would make ~15 pages. Confirm.
9. **Grouping mechanism (5.4).** Subdirectories + an explicit alint.org sidebar
   entry (recommended) vs flat `sidebar.order` bands. Confirm before authoring: the
   subdir choice sets the page URLs, and therefore the redirects.

## 12. Appendix: audit provenance

Three read-only audits (2026-09-04) against workspace v0.16.1 produced the
current-state critique (section 1, 7), the code-verified concept map (section 4),
and the diagram-technology assessment (section 6). Counts verified against
`facts.json`. This doc is the synthesis; the per-page content and each diagram's
SVG are produced during implementation (phases 3-5). All claims in this doc were
re-verified against source in an adversarial review pass (counts, the three hard
errata line-citations, the three source-doc inaccuracies, the `.md`-not-`.mdx`
fact, the absence of a CSP, and the sidebar-grouping mechanism).

## 13. Appendix: the concept-page template

Every concepts page follows one shape (progressive disclosure; the diagram
reinforces the thesis; a worked example grounds it). Authors copy this skeleton:

    ---
    title: <Concept>
    description: <one sentence; straight quotes; no em-dash>
    sidebar:
      order: <n>
    ---

    <One-sentence thesis: the mental model in a line.>

    <Animated SVG diagram (Appendix 14), illustrating exactly that sentence.>

    <Narrative: two to four short paragraphs, each idea building on the last.>

    ## In practice

    <A worked `.alint.yml` snippet AND the resulting `alint check` report line.>

    ## Going deeper

    <Links to the reference/config pages and the interactive LikeC4 view.>

Worked mini-example (the pipeline page, abbreviated):

    alint reads one declarative .alint.yml, makes a single parallel pass over your
    repository, and emits one report in your pipeline's format.

    [pipeline animation -- Appendix 14]

    The walk runs once; each file's bytes are read at most once; evaluation is
    parallel, then results are re-sorted so output is byte-identical run to run.

    ## In practice
    version: 1
    rules:
      - { id: readme-exists, kind: file_exists, paths: [README.md], level: error }

    error  readme-exists  README.md is required at the repo root

## 14. Appendix: the animated-SVG reference template

The reference implementation (the pipeline hero). Pure inline SVG + inline
`<style>`: no `.mdx`, no script, no dependency; themed via Starlight tokens with
literal fallbacks; `prefers-reduced-motion` rests it at the final frame. Every
concept diagram is a variation on this skeleton reusing the token kit below.

    <svg class="alint-diagram" viewBox="0 0 640 120" role="img"
         aria-labelledby="pipe-title pipe-desc" xmlns="http://www.w3.org/2000/svg">
      <title id="pipe-title">alint execution pipeline</title>
      <desc id="pipe-desc">One token flows left to right through four stages:
        config, walk, dispatch, report.</desc>
      <style>
        .alint-diagram { max-width: 100%; height: auto;
          font: 600 14px system-ui, -apple-system, "Segoe UI", sans-serif; }
        .wire  { fill: none; stroke: var(--sl-color-gray-4, #9aa0b4); stroke-width: 2;
                 stroke-dasharray: 6 6; animation: pipe-flow 1.2s linear infinite; }
        .stage { fill: var(--sl-color-gray-6, #eef2ff);
                 stroke: var(--sl-color-accent, #4338ca); stroke-width: 1.5; }
        .label { fill: var(--sl-color-text, #1e1b4b); }
        .token { fill: var(--sl-color-accent, #4338ca);
                 animation: pipe-travel 3.6s cubic-bezier(.5,0,.5,1) infinite; }
        @keyframes pipe-flow   { to { stroke-dashoffset: -12; } }
        @keyframes pipe-travel { 0% { transform: translateX(0); opacity: 0; }
          8% { opacity: 1; } 92% { opacity: 1; }
          100% { transform: translateX(480px); opacity: 0; } }
        @media (prefers-reduced-motion: reduce) {
          .wire  { animation: none; stroke-dasharray: none; }
          .token { animation: none; transform: translateX(480px); opacity: 1; } }
      </style>
      <path class="wire" d="M 80 60 H 560" />
      <g class="label" text-anchor="middle">
        <rect class="stage" x="20"  y="40" width="120" height="40" rx="6"/><text x="80"  y="65">config</text>
        <rect class="stage" x="180" y="40" width="120" height="40" rx="6"/><text x="240" y="65">walk</text>
        <rect class="stage" x="340" y="40" width="120" height="40" rx="6"/><text x="400" y="65">dispatch</text>
        <rect class="stage" x="500" y="40" width="120" height="40" rx="6"/><text x="560" y="65">report</text>
      </g>
      <circle class="token" cx="80" cy="60" r="6"/>
    </svg>

Visual-language token kit (shared across every diagram):

- Structure: `var(--sl-color-gray-4..6)`. Active/flowing element:
  `var(--sl-color-accent)` (`#4338ca`). Violation: a danger token (red).
- Motion vocabulary: a traveling token means "one pass"; a dashed wire with
  animated offset means "flow"; fade or scale means "matched" or "suppressed."
- Accessibility on every diagram: `role="img"` + `<title>` + `<desc>`, and a
  `prefers-reduced-motion` block that rests at the final frame.
- Performance: one concept per page means one or two diagrams render per page, so
  the continuous CSS animations carry no meaningful cost; no off-screen pausing
  (which would need JS) is required.
