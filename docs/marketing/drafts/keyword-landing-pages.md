---
destination: alint.org site repo — 5 new top-level routes (/repolinter-alternative/, /monorepo-linter/, /agent-friendly-linter/, /language-agnostic-linter/, /repository-structure-linter/)
status: drafting
blocks_on: alint-org-compare.md (each landing page links to relevant compare-page sections); alint-org-examples-gallery.md (case-study links must resolve); meta-descriptions.md + schema-org-jsonld.md (each landing page needs a description and Article JSON-LD from day one)
last_touched: 2026-05-06
---

# alint.org keyword-targeted landing pages — content brief

## Why

Per `launch-prep.md` (P3.2 SEO § "Keyword strategy"), 5 high-intent
search queries map cleanly to alint's positioning but currently have
no targeted landing page on alint.org:

1. **"repolinter alternative" / "repolinter replacement"** — highest
   intent. Repolinter was archived 2026-02; users are *actively*
   shopping for a replacement.
2. **"monorepo linter" / "monorepo conventions enforcement"** —
   medium-high volume; alint is one of the only language-agnostic
   answers.
3. **"agent-friendly linter" / "AI-aware repository linter"** —
   emerging category; low current volume but rapidly growing as
   coding-agent adoption climbs.
4. **"language-agnostic linter" / "polyglot linter"** — steady
   evergreen volume.
5. **"repository structure linter" / "filesystem linter"** — the
   exact category alint occupies.

Without dedicated landing pages, queries for these terms either bounce
to the alint.org landing (which is too broad — visitors don't see "yes,
this tool answers your specific question") or land on the `/compare/`
page (better but still a step removed from "this is the answer to
your search").

A dedicated landing per keyword does three things:

1. **Tighter SERP CTR** — the page's `<title>` + `<meta description>`
   match the query verbatim.
2. **Lower bounce rate** — the H1 confirms "you found the right
   page", the body answers the query in the first 200 words.
3. **Stronger internal-link graph** — each landing page links into the
   compare page, the closest case study, and the most relevant
   bundled-ruleset / rule pages. Topic-cluster authority compounds
   over time.

Per launch-prep, "high intent + low competition" is the framing.
None of these queries have a dominant SEO-optimised competitor, and
the queries themselves are commercial-intent ("alternative",
"replacement", "linter for X").

## Page-by-page brief

Each landing page follows the same shape:

- **Frontmatter:** title (≤60 chars, includes the head term),
  description (155 chars, value-prop-first, includes the head term),
  conformant with `meta-descriptions.md`.
- **H1:** includes the head term verbatim.
- **Body:** 300-500 words. First 200 answer the searcher's question;
  the rest provides supporting context + CTAs.
- **Primary internal links:** alint.org landing (back-link), the
  most-relevant `/compare/` section, the most-relevant case study,
  the most-relevant docs page or ruleset.
- **CTA block:** install snippet + GitHub link + docs link.

---

### 1. `/repolinter-alternative/` — highest priority

**Target query:** "repolinter alternative", "repolinter replacement",
"alternative to repolinter", "what to use instead of repolinter".

**Volume signal:** Repolinter has 1.4k GitHub stars + a TODO Group
(Linux Foundation) endorsement. The archive notice in February 2026
generated meaningful chatter on HN, dev.to, and r/devops.
"repolinter alternative" became a measurable head term post-archive.
**Estimate: 100-300 monthly searches** (low absolute volume, very high
intent).

**Frontmatter:**

```yaml
---
title: Repolinter alternative — alint
description: Repolinter was archived in February 2026. alint is the language-agnostic, actively-maintained replacement — a strict superset of Repolinter's rule catalogue.
---
```

**Page outline:**

```markdown
# Looking for a Repolinter alternative? alint is it.

[Repolinter](https://github.com/todogroup/repolinter) was archived
in February 2026. The TODO Group's tool for OSS-baseline checks
(LICENSE present, README has the right sections, CONTRIBUTING.md
exists, etc.) hadn't shipped a release since 2024 even before the
archive — and now there's no upstream at all.

**alint is the language-agnostic, actively-maintained replacement.**

## What you get out of the box

The bundled `oss-baseline@v1` ruleset (15 rules) maps Repolinter's
file-presence and content-shape axioms 1:1. Drop into your
`.alint.yml`:

\`\`\`yaml
extends:
  - alint:oss-baseline@v1
\`\`\`

…and you're at parity with Repolinter's default config. Most users
were running roughly that anyway.

## What you gain by switching

- **Active maintenance.** alint shipped 14 releases in the last 6
  months. Repolinter's last release was 2024.
- **Cross-file rules.** Repolinter checks one file at a time. alint's
  `pair`, `for_each_dir`, `for_each_file`, `dir_contains`, `unique_by`,
  `every_matching_has` rules cover invariants Repolinter can't express.
- **Structured-query rules.** Validate fields *inside* JSON / YAML /
  TOML with full RFC 9535 JSONPath. (Repolinter has partial
  jsonpath-plus support.)
- **18 more bundled rulesets.** rust, node, python, go, java,
  monorepo, CI, hygiene, agent-context — Repolinter ships only
  oss-baseline.
- **Auto-fix.** 12 mechanically-safe fix ops (trim whitespace,
  normalise line endings, strip BOM/bidi, prepend/append, rename).
  Repolinter has partial fix support for file presence only.
- **Performance.** Sub-second on 100K-file workspaces vs Repolinter's
  Node startup + per-rule JS execution.
- **Agent-aware output.** First-class `agent` output format with
  per-violation `agent_instruction` strings.

## How to migrate

The [migration guide](/migrating-from/repolinter/) covers all 24
Repolinter axioms with side-by-side YAML — most map directly, a few
need a small `.alint.yml` rewrite.

[Read the full alint vs Repolinter comparison →](/compare/#vs-repolinter)

[Get started in 60 seconds →](/docs/getting-started/)
```

**Word count:** ~340.

**Primary internal links:**

- `/` (back-link in nav)
- `/compare/#vs-repolinter` (deep-dive comparison)
- `/migrating-from/repolinter/` (migration guide)
- `/docs/bundled-rulesets/oss-baseline@v1/` (the drop-in ruleset)
- `/docs/getting-started/` (CTA)

**Don't:**

- Don't repeat "Repolinter alternative" more than 3 times in body
  copy. Once in H1, once in the lead paragraph, once in the CTA is
  natural; more is keyword-stuffing.
- Don't disparage Repolinter's quality. The framing is "Repolinter
  was good for what it was; it's archived now." Respect the prior
  art.
- Don't write thin content (< 300 words). Google quality raters
  flag commercial-intent pages with thin content as "low".

---

### 2. `/monorepo-linter/`

**Target query:** "monorepo linter", "linter for monorepos",
"monorepo conventions enforcement", "lint monorepo structure".

**Volume signal:** monorepo tooling is a popular topic — bazel,
turborepo, nx, pnpm workspaces all generate steady search volume.
"monorepo linter" specifically is narrower; existing
results are dominated by per-language-linter-in-a-monorepo articles
rather than dedicated tools. **Estimate: 200-500 monthly searches**
(steady evergreen).

**Frontmatter:**

```yaml
---
title: Monorepo linter — alint enforces structural conventions across packages
description: alint enforces filesystem and content conventions across all packages in your monorepo with one .alint.yml. 67-crate Cargo workspaces, 6-language polyglots, all of it.
---
```

**Page outline:**

```markdown
# A linter for monorepo conventions

Per-package linters (eslint, clippy, ruff, golangci-lint) check the
code *inside* each package. None of them check that **your packages
agree with each other** — that every crate has a `README.md`, that no
two packages claim the same npm name, that every workspace `Cargo.toml`
sets `publish = false` consistently, that the Rust toolchain pinned in
`rust-toolchain.toml` matches what CI uses.

**alint is the linter for those conventions.**

## What alint catches that per-package linters miss

- **Package-level convention drift.** Every workspace member has a
  `README.md`; every `package.json` shares the same `engines` field;
  every `Cargo.toml` declares `edition = "2021"`. One config across
  all packages.
- **Cross-package invariants.** No two `package.json` files share the
  same `name`. Every Rust crate marked `publish = false` has a
  `# Internal: do not publish` comment in its README.
- **Polyglot cross-language conventions.** In a monorepo with C++,
  Java, Python, Rust, Go, and JavaScript, no per-language linter sees
  the cross-language shape — the `LICENSE` headers, the SPDX
  identifiers, the `.editorconfig` compliance, the
  `CODEOWNERS`-vs-actual-directory-structure consistency.
- **Workspace-tier checks.** The Cargo workspace's `Cargo.lock` is
  committed; every TS path-alias in `tsconfig.json` resolves to a
  real `packages/<name>/` directory.

## Real monorepos using alint

- [astral-sh/uv](/examples/astral-sh-uv/) — 67 Rust crates, alint
  enforces workspace conventions that are otherwise unenforced.
- [apache/arrow](/examples/apache-arrow/) — 6 languages
  (C++/Java/Python/Rust/Go/JS), 21 lint hooks across 14 tool repos,
  zero of them see cross-language structural shape — alint fills
  that gap.
- [vercel/turbo](/examples/vercel-turbo/) — TypeScript + Rust
  hybrid; alint enforces the package-level conventions Turborepo
  itself doesn't lint.

[Browse all 20 case studies →](/examples/)

[Read the full alint vs alternatives comparison →](/compare/)

## Get started

\`\`\`bash
cargo install alint
alint init   # generates a starter .alint.yml
alint check
\`\`\`

[Full quickstart →](/docs/getting-started/)
```

**Word count:** ~365.

**Primary internal links:**

- `/`
- `/compare/` (broader comparison — monorepo angle isn't a single
  comparator)
- `/examples/astral-sh-uv/`, `/examples/apache-arrow/`,
  `/examples/vercel-turbo/` (3 monorepo case studies)
- `/examples/` (gallery)
- `/docs/getting-started/`
- `/docs/bundled-rulesets/monorepo@v1/` (if exists; if not, the
  rulesets index)

**Don't:**

- Don't claim alint replaces Bazel / Nx / Turborepo. It doesn't —
  those are build orchestrators, alint is a structure linter. They
  coexist.
- Don't oversell on the polyglot angle if the visitor's monorepo is
  single-language. The page should still be useful for a Cargo-only
  workspace user.

---

### 3. `/agent-friendly-linter/`

**Target query:** "agent-friendly linter", "AI-aware linter",
"linter for AI-generated code", "linter for AI agents", "Claude Code
linter", "Cursor linter", "linter that gives AI hints".

**Volume signal:** emerging category. Search volume was near-zero in
2024, climbing through 2025-26 as Claude Code, Cursor, Aider, GitHub
Copilot Chat, etc. drove agent-touched-codebase awareness.
**Estimate: 50-200 monthly searches today, growing**.

**Frontmatter:**

```yaml
---
title: Agent-friendly linter — alint emits structured hints for AI agents
description: alint's agent output format ships per-violation agent_instruction strings, plus bundled agent-hygiene and agent-context rulesets for AI-touched repos.
---
```

**Page outline:**

```markdown
# A linter built for the AI-agent workflow

Coding agents (Claude Code, Cursor, Aider, Copilot Chat, Codex) write
real code in real repos every day. They make structural mistakes per-
agent-typical patterns: scattering `.bak` files, leaving `console.log`
debugging in committed code, generating files outside the project's
conventional locations, drifting from the `.editorconfig`.

Generic linters tell humans about these mistakes. **alint also tells
agents.**

## Three things alint does for agent workflows

### 1. The `agent` output format

Every alint violation can carry an `agent_instruction` string —
written for an agent to read, not a human. Run:

\`\`\`bash
alint check --output agent
\`\`\`

…and you get a JSON document where every violation includes a
deterministic, machine-readable instruction the agent can act on
without further parsing.

\`\`\`json
{
  "violations": [
    {
      "rule": "filename_case",
      "file": "src/myComponent.tsx",
      "expected": "kebab-case (per .alint.yml)",
      "agent_instruction": "Rename src/myComponent.tsx to src/my-component.tsx and update all imports."
    }
  ]
}
\`\`\`

### 2. Bundled `agent-hygiene@v1` ruleset

A drop-in ruleset of the most common agent-typical mistakes:

\`\`\`yaml
extends:
  - alint:agent-hygiene@v1
\`\`\`

Catches: `.bak` / `.orig` files left in commits, `console.log` /
`print()` debugging in committed code, accidentally-committed
`node_modules/` or `target/`, `TODO(agent):` comments without an
issue ref, files outside conventional directories.

### 3. Bundled `agent-context@v1` ruleset

The companion ruleset that ensures your repo *teaches the agent the
conventions* it should follow:

\`\`\`yaml
extends:
  - alint:agent-context@v1
\`\`\`

Asserts: `.cursor/` or `.claude/` agent-config directory exists,
`AGENTS.md` (or `CLAUDE.md`) lives at repo root, the agent-config
directory contains the conventions the rest of the repo expects.

## Why this matters

If you've adopted coding agents in production, the lint loop becomes
agent-driven: the agent makes a change → CI runs alint → alint emits
agent-format JSON → the agent reads its own violations and self-fixes.
The `agent_instruction` strings make that loop deterministic instead
of having the agent re-derive what to do from natural-language error
messages.

[Read the alint vs alternatives comparison →](/compare/)

[Browse the rule catalogue →](/docs/rules/)

[Get started →](/docs/getting-started/)
```

**Word count:** ~395.

**Primary internal links:**

- `/`
- `/compare/` (mention `agent` row in the feature matrix)
- `/docs/bundled-rulesets/agent-hygiene@v1/`,
  `/docs/bundled-rulesets/agent-context@v1/`
- `/docs/cli/check/` (for the `--output agent` reference)
- `/docs/getting-started/`

**Don't:**

- Don't position alint as "the linter that protects you from AI
  agents". The framing is collaborative — agents are users; alint
  serves them.
- Don't oversell — `agent_instruction` is a String field on each
  rule, not a ML-powered fix generator. Be honest about what it is.

---

### 4. `/language-agnostic-linter/`

**Target query:** "language-agnostic linter", "polyglot linter",
"linter for any language", "language-independent code linter".

**Volume signal:** evergreen. The phrase "language-agnostic" is
specific developer-tool jargon; people searching for it know what
they want. Existing top results: Megalinter, ls-lint, EditorConfig.
None lead with the structural-validation angle. **Estimate: 200-400
monthly searches**.

**Frontmatter:**

```yaml
---
title: Language-agnostic linter — alint runs on any repo, any language
description: alint enforces filesystem, content, and cross-file conventions independent of programming language. One static binary, 60 rule kinds, 19 bundled rulesets.
---
```

**Page outline:**

```markdown
# A truly language-agnostic linter

alint operates on **bytes and structure**, not parsed code. It doesn't
care whether your repo is Python, Rust, Go, Java, TypeScript,
Kotlin, Swift, C++, or all six in one monorepo — the rule engine
treats files as files.

## What language-agnostic actually means here

- **No language parser.** alint doesn't have an AST for any language.
  The 60 rule kinds work on filenames, file content (regex / literal),
  structured documents (JSON / YAML / TOML via RFC 9535 JSONPath),
  and cross-file relationships. None of them require a language-
  specific compiler or parser.
- **One static binary.** ~10 MB Rust binary, no runtime dependencies.
  Drop into CI on any OS, run, done. No `npm install`, no JVM, no
  Python interpreter.
- **Per-language *opinions* are opt-in via bundled rulesets.** The 19
  bundled rulesets (`rust@v1`, `node@v1`, `python@v1`, `go@v1`,
  `java@v1`, etc.) encode the *conventions* of each ecosystem — but
  they're built on the same language-agnostic rule kinds. You opt in
  to whichever ecosystems your repo touches.
- **Polyglot monorepos work without a config story per language.**
  See [apache/arrow](/examples/apache-arrow/) (C++/Java/Python/Rust/
  Go/JS in one repo, 1 `.alint.yml`) and
  [vercel/next.js](/examples/vercel-next.js/) (TypeScript + Rust
  hybrid).

## What language-agnostic doesn't mean

alint is **not** trying to replace your per-language code linter.
ESLint, Clippy, ruff, golangci-lint, etc. all do AST analysis that
alint deliberately doesn't. alint is the **structural floor**;
those are the **semantic surface**. The two coexist.

## Real polyglot repos using alint

- [apache/arrow](/examples/apache-arrow/) — 6 languages, 1 config.
- [vercel/next.js](/examples/vercel-next.js/) — TS + Rust hybrid.
- [microsoft/typescript](/examples/microsoft-typescript/) — TS-heavy
  but cross-cuts compiler-binary management, fixture conventions, etc.

[Read the alint vs alternatives comparison →](/compare/)

[Browse all 20 case studies →](/examples/)

[Get started in 60 seconds →](/docs/getting-started/)
```

**Word count:** ~370.

**Primary internal links:**

- `/`
- `/compare/#vs-megalinter`, `/compare/#vs-editorconfig`
- `/examples/apache-arrow/`, `/examples/vercel-next.js/`,
  `/examples/microsoft-typescript/`
- `/examples/`
- `/docs/getting-started/`

**Don't:**

- Don't claim alint replaces per-language linters. It doesn't.
  Hammer the "structural floor + semantic surface" framing.
- Don't define "language-agnostic" loosely. Be specific about what
  alint does and doesn't see.

---

### 5. `/repository-structure-linter/`

**Target query:** "repository structure linter", "filesystem linter",
"directory structure linter", "lint repo layout".

**Volume signal:** the most literal head term for what alint is. Less
trafficked than "monorepo linter" or "language-agnostic linter" but
the highest commercial intent — searchers who type this know exactly
what they're shopping for. **Estimate: 50-150 monthly searches**.

**Frontmatter:**

```yaml
---
title: Repository structure linter — alint
description: alint lints the structure of your repository — required files, naming conventions, directory layout, and content patterns. Declarative .alint.yml, sub-second on 100K files.
---
```

**Page outline:**

```markdown
# A linter for the structure of your repository

Most linters check the **code** inside files. alint checks **the
files themselves** — what files exist, where they live, what they're
named, and what's inside them at the structural level (manifest
fields, content patterns, format compliance).

## What "structure" means in practice

- **Required files exist.** `LICENSE`, `README.md`, `CONTRIBUTING.md`,
  `.editorconfig`, `CODEOWNERS`, language-specific manifests
  (`Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`).
- **Filename conventions hold.** All `.tsx` files in `src/components/`
  are PascalCase; all `.test.ts` files live next to their `.ts`
  counterpart; all docs files are kebab-case.
- **Directory layouts hold.** Every `packages/<name>/` has a
  `README.md` and a `package.json`; every Rust workspace member is
  declared in the workspace `Cargo.toml`'s `members` list.
- **Manifest field shape is correct.** Every `package.json` has a
  `license` field; every `Cargo.toml` sets `edition = "2021"`; every
  GitHub workflow uses `actions/checkout@v4` not `@v3`.
- **Content patterns hold.** No `console.log` in committed code; no
  `# TODO` without an issue reference; every Markdown file ends with
  a newline; no Unicode bidirectional control characters slipping in.
- **Cross-file invariants hold.** Every `*.proto` has a `*.rs`
  generated alongside it; every TS path-alias in `tsconfig.json`
  resolves to a real package; no two `package.json` files share the
  same `name`.

## How it's expressed

One `.alint.yml`, declarative, with a [JSON Schema](https://alint.org/_alint/configuration/schema.json)
for editor autocomplete:

\`\`\`yaml
extends:
  - alint:rust@v1
  - alint:oss-baseline@v1

rules:
  - kind: filename_case
    paths: ["src/**/*.rs"]
    case: snake
\`\`\`

## Performance

Sub-second on a 100K-file workspace bundle. ~12 seconds on a 1M-file
mega-monorepo. Single static Rust binary, no runtime dependencies.

[Browse all 60 rule kinds →](/docs/rules/)

[Read the alint vs alternatives comparison →](/compare/)

[Get started →](/docs/getting-started/)
```

**Word count:** ~370.

**Primary internal links:**

- `/`
- `/compare/`
- `/docs/rules/`
- `/docs/bundled-rulesets/`
- `/docs/getting-started/`

**Don't:**

- Don't enumerate every rule kind here. The role of this page is
  *category landing* — link to `/docs/rules/` for the full catalogue.
- Don't dwell on auto-fix. It's a feature, not the head-term value
  prop. Mention once if at all.

---

## Universal "don'ts" across all 5 pages

- **No keyword stuffing.** Each head term appears 3-4 times maximum
  (H1, lead paragraph, sub-heading, one CTA). More signals "this
  is a doorway page" to Google quality raters.
- **No thin content.** Each page is 300-500 words. Below 300 risks
  Google's "thin content" classification; above 500 risks losing
  the focused-landing-page signal.
- **No duplicated boilerplate across the 5 pages.** Each page has its
  own opening framing, its own examples, its own internal links.
  Templates are templates, not copy-paste.
- **No competing CTAs.** Each page picks a primary CTA (the one
  matching the searcher's intent — for "alternative" it's the
  migration guide; for "monorepo" it's the gallery; for "agent" it's
  the agent rulesets). Secondary CTAs link out cleanly.
- **No promises alint doesn't deliver.** Every claim on these pages
  must hold against the actual rule catalogue + bundled rulesets +
  measured benchmarks.

## Implementation notes (for the site repo applier)

1. **5 new top-level routes** in `asamarts/alint.org`:
   - `src/content/docs/repolinter-alternative.md` (or
     `src/pages/repolinter-alternative.astro`, depending on
     Starlight's content vs. pages convention).
   - Same for the other 4.

2. **Top-level nav.** Don't surface these in the main nav. They're
   SEO landing pages, not user-facing routes — visitors who arrive
   from search land directly. Adding them to nav would clutter the
   header for everyone else and dilute the landing's "first-time
   visitor" focus.

3. **Each landing page MUST have:**
   - `description:` frontmatter (per `meta-descriptions.md`).
   - `Article` + `BreadcrumbList` JSON-LD (per
     `schema-org-jsonld.md`). The breadcrumb is short:
     `alint > <Page Title>` — minimal but valid.
   - A `<title>` ≤60 chars including the head term.

4. **Internal-link audit before publish.** Each landing page links
   to the `/compare/` page, ≥1 case study, and ≥1 docs page. Verify
   all links resolve at publish time (some `/examples/` pages may
   not exist yet if `alint-org-examples-gallery.md` hasn't shipped).

5. **Cross-link from the landing page once.** A single line near the
   bottom of `https://alint.org/` ("See also: [Repolinter alternative](/repolinter-alternative/)
   • [Monorepo linter](/monorepo-linter/) • [Agent-friendly linter](/agent-friendly-linter/)
   • [Language-agnostic linter](/language-agnostic-linter/)
   • [Repository structure linter](/repository-structure-linter/)")
   gives Googlebot the link signal without dragging the landing's
   focus.

6. **Sitemap inclusion.** All 5 land in the sitemap automatically
   (per `sitemap-config.md`'s site-repo-commit-date branch).
   Resubmit sitemap to Search Console after publish so the new URLs
   index quickly.

## Open questions before publish

1. **Dedicated landing for "ls-lint alternative"?** launch-prep lists
   "ls-lint alternative" as a sixth keyword. Recommend NOT building
   it as a dedicated landing — ls-lint is actively maintained,
   competing for that head term reads as opportunistic. Instead,
   the `/migrating-from/ls-lint/` migration guide already covers
   that intent.
2. **Page order priority.** Recommend ship order:
   1. `/repolinter-alternative/` (highest intent, highest CTR
      potential).
   2. `/monorepo-linter/` (highest steady volume).
   3. `/language-agnostic-linter/` (steady evergreen).
   4. `/repository-structure-linter/` (head term but lower volume).
   5. `/agent-friendly-linter/` (emerging; low current volume but
      lowest competition — invest now, harvest as the category
      grows).
3. **Translations.** Site is en-only today. Future translation work
   should start with these landing pages — `/ja/repolinter-alternative/`
   etc. — because they're short, focused, and high-leverage.
4. **A/B testing.** Cloudflare Pages doesn't ship A/B tooling
   natively. Recommend ship straight to production; iterate on
   copy based on Search Console CTR + position data after 4 weeks.
5. **Do we need an explicit `<link rel="canonical">` per page?**
   Starlight emits canonical tags by default. Verify on a deployed
   page before publish.

## Pre-publish checklist

- [ ] All 5 landing pages drafted into the site repo (`.md` or
      `.astro` per Starlight convention).
- [ ] Each page has `title:` (≤60 chars) + `description:` (~155
      chars) frontmatter.
- [ ] Each page emits `Article` + `BreadcrumbList` JSON-LD.
- [ ] Each page's H1 matches the target head term.
- [ ] Each page is 300-500 words (verify with `wc -w`).
- [ ] Each page links to `/`, `/compare/<deep-link>`, ≥1
      `/examples/<repo>/`, and ≥1 `/docs/<...>/`.
- [ ] Each page's primary CTA matches its searcher intent.
- [ ] Landing page (`/`) gets the bottom-of-page "See also" link
      strip.
- [ ] All linked `/examples/<repo>/` and `/migrating-from/<source>/`
      routes resolve at publish time.
- [ ] Sitemap regenerated and resubmitted to Search Console + Bing.
- [ ] Google Rich Results Test passes for each landing page
      (`Article` + `BreadcrumbList`).
- [ ] STATE.md row for `keyword-landing-pages.md` flipped to
      `live` with date + commit SHA + per-page slug list.

## Estimated diff size on the site repo

5 new markdown / astro files, each ~80-120 lines (frontmatter +
body + CTAs):

- `/repolinter-alternative/`: ~90 lines.
- `/monorepo-linter/`: ~110 lines.
- `/agent-friendly-linter/`: ~120 lines.
- `/language-agnostic-linter/`: ~100 lines.
- `/repository-structure-linter/`: ~100 lines.

Plus:

- 1 line on `astro.config.mjs` (no nav addition; just ensure routes
  are picked up — Starlight auto-discovers).
- ~6 lines added to landing (`/`) for the bottom-of-page "See also"
  link strip.

Total: **~530 lines** on the site repo. Largest of the P3.2 drafts
by line count, but each page is independent — can ship in 5 PRs
serialised (or one monolithic PR — trade-off is review velocity
vs. coordinated SEO publish).

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `meta-descriptions.md` | Each landing page MUST have a `description:` from day one. This brief uses the templates from there. |
| `schema-org-jsonld.md` | Each landing page emits `Article` + `BreadcrumbList`. Brief depends on JSON-LD plumbing being in place. |
| `sitemap-config.md` | Each new page picks up site-repo-commit-date `lastmod` automatically. |
| `alint-org-compare.md` | Each landing page deep-links into `/compare/` sections. Depends on compare page existing. |
| `alint-org-examples-gallery.md` | Each landing page links to ≥1 case study. Depends on at least the cited case studies resolving. |
| `migrate-from-repolinter.md` | `/repolinter-alternative/` links to the migration guide. Depends on it shipping. |
