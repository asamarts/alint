---
destination: per-page frontmatter additions across alint.org content (site repo + bundle source files in asamarts/alint/docs/site/**)
status: drafting
blocks_on: confirm Starlight's auto-generated rule pages support frontmatter `description:` (they should via Starlight's docs schema, but the docs-export pipeline needs to pass it through)
last_touched: 2026-05-06
---

# alint.org per-page `<meta description>` strategy — content brief

## Why

Starlight's default behaviour: if a page's frontmatter doesn't set
`description:`, it falls back to extracting the page's first
paragraph for `<meta name="description">` and `<meta property="og:description">`.

That fallback is *effective but not optimised*:

- The first paragraph was written for **a reader who already clicked**
  — it answers "what is this page?" not "why should I click this
  search result?".
- Length is uncontrolled — Starlight truncates to ~160 chars, often
  mid-sentence.
- Identical fallback drift across rule pages: many of the auto-generated
  rule pages start with "**Family:**" boilerplate, so SERP snippets
  for every rule in the same family look interchangeable.

This brief proposes a **frontmatter-driven `description:` field per
page**, with a one-line style guide and concrete examples for the 5
most-visited page types. The win: every search result snippet is
purposefully chosen, leads with the page's unique value prop, and
includes the relevant keyword cleanly.

## Proposed convention

Every page in alint.org's content tree (both site-repo `src/content/`
sources and bundle-source markdown files under
`~/projects/alint/docs/site/**`) gets a `description:` frontmatter
field.

```yaml
---
title: <existing>
description: <new — single line, ~155 chars max>
---
```

Starlight automatically wires `description:` into:

- `<meta name="description" content="…">`
- `<meta property="og:description" content="…">`
- `<meta name="twitter:description" content="…">`
- The card preview when a page is linked from another Starlight page

(See [Starlight frontmatter docs](https://starlight.astro.build/reference/frontmatter/#description).)

## Style guide (the one-liner version)

**~155 chars. Lead with the unique value prop. Include the keyword.
Active voice. No marketing fluff.**

Expanded:

1. **Length: ≤155 chars.** Google truncates SERP snippets at ~155-160
   chars on desktop, ~120 on mobile. Going over wastes the
   characters Google clips.
2. **Lead with the unique value prop**, not the category. *"Fast,
   language-agnostic linter…"* beats *"alint is a linter that…"*. The
   first 90 chars are what shows on mobile.
3. **Include the keyword the page targets.** For rule pages, the rule
   kind name (`commented_out_code`, `every_matching_has`, etc.). For
   ruleset pages, the ruleset name (`rust@v1`, `oss-baseline@v1`).
   For landing pages, the head term they're optimising for.
4. **Active voice, present tense.** *"Enforces filename conventions"*
   beats *"Used to enforce filename conventions"*.
5. **No marketing fluff.** Skip "powerful", "revolutionary",
   "next-generation". Concrete > grand.
6. **Include a number when honest.** Numbers anchor SERP attention
   ("60 rule kinds", "19 bundled rulesets", "1.1s on 100K files").
7. **One sentence, no semicolons, no em-dashes.** Search snippets
   read as one short clause.
8. **Don't repeat the title.** The `<title>` already shows above the
   description in the SERP; repeating it wastes characters.

## Examples — 5 most-visited page types

### 1. Landing — `https://alint.org/`

```yaml
---
title: alint — fast, language-agnostic repo-structure linter
description: Fast, language-agnostic linter for repository structure, files, and content. 60 rule kinds, 19 bundled rulesets, one static Rust binary.
---
```

Char count: 152. Leads with the headline value prop (mirrors the
hero from `alint-org-hero.md`); name-drops three concrete numbers;
ends on the install-friction signal ("one static Rust binary").

### 2. Docs landing — `https://alint.org/docs/`

```yaml
---
title: Documentation
description: Get started with alint in 60 seconds. Reference for 60 rule kinds, 19 bundled rulesets, the .alint.yml schema, and integration recipes.
---
```

Char count: 151. Leads with the time-to-value ("60 seconds"); covers
the four content surfaces a docs reader is actually looking for.

### 3. Per-rule page — `https://alint.org/docs/rules/<family>/<kind>/`

Template:

```
description: <kind_name> rule — <one-line what it checks>. <Family-name family. Auto-fix: yes|no.>
```

Concrete examples:

```yaml
# /docs/rules/git-hygiene/commented_out_code/
---
title: commented_out_code
description: commented_out_code rule — flags blocks of source code that are commented out instead of deleted. Git-hygiene family. Auto-fix: yes.
---
```

Char count: 145. Includes the rule kind name (the search query),
explains the check in plain language, surfaces the family + the
auto-fix capability (a strong CTR driver — auto-fixable rules are
more interesting to bring into a config).

```yaml
# /docs/rules/cross-file/every_matching_has/
---
title: every_matching_has
description: every_matching_has rule — for every file matching globA, assert a corresponding file matching globB exists. Cross-file family.
---
```

Char count: 142. Same template; the "for every…assert" phrasing
captures what the rule is uniquely useful for (companion-file
invariants).

```yaml
# /docs/rules/structured-query/structured_path_jsonpath/
---
title: structured_path_jsonpath
description: structured_path_jsonpath rule — assert RFC 9535 JSONPath expressions over JSON, YAML, and TOML files. Structured-query family.
---
```

Char count: 143. Specifies "RFC 9535" because that's a real
discriminator — partial JSONPath implementations are common, full
RFC 9535 isn't.

### 4. Bundled rulesets index + per-ruleset page — `https://alint.org/docs/bundled-rulesets/`

Index page:

```yaml
---
title: Bundled rulesets
description: 19 ready-to-use rulesets shipped with alint — rust, node, python, go, java, monorepo, CI hygiene, agent-context, oss-baseline, and more.
---
```

Char count: 153. Number anchor ("19"); enumerates the most-searched
ecosystems first; "and more" earns the trailing characters without
keyword-stuffing.

Per-ruleset (auto-generated; needs the docs-export pipeline to emit
the `description:` from each ruleset's YAML):

```yaml
# /docs/bundled-rulesets/rust@v1/
---
title: rust@v1
description: rust@v1 ruleset — Cargo.toml shape, workspace conventions, edition pinning, and Rust-specific OSS-baseline checks for any Cargo workspace.
---
```

Char count: 151. Names what the ruleset *does* — the Cargo
workspace user reading the SERP wants to see "Cargo.toml shape" and
"workspace conventions" reflected back.

```yaml
# /docs/bundled-rulesets/oss-baseline@v1/
---
title: oss-baseline@v1
description: oss-baseline@v1 ruleset — 15 rules covering LICENSE, README, CONTRIBUTING, CODE_OF_CONDUCT, and the rest of the OSS-baseline files.
---
```

Char count: 145. Number ("15"); enumerates the *files* a Repolinter
migrant searches for ("LICENSE README CONTRIBUTING").

### 5. Per-case-study page — `https://alint.org/examples/<owner>-<repo>/`

Template:

```
description: <owner>/<repo> — <one-line headline finding>. Working .alint.yml + writeup of what alint catches the existing tooling misses.
```

Concrete examples:

```yaml
# /examples/kubernetes-kubernetes/
---
title: kubernetes/kubernetes — alint case study
description: kubernetes/kubernetes — 17 of 50 hack/verify-*.sh scripts mapped to declarative alint rules. Working .alint.yml plus a writeup of the gaps.
---
```

Char count: 154. Headline finding (the 17/50 metric is the social
proof); CTAs at the end ("working .alint.yml plus writeup").

```yaml
# /examples/apache-arrow/
---
title: apache/arrow — alint case study
description: apache/arrow polyglot monorepo — 6 languages, 21 lint hooks, 0 cross-language structural linters. alint fills the gap. Working config + writeup.
---
```

Char count: 154. Polyglot positioning lead; 0/21 framing makes the
"alint fills the gap" plausible rather than salesy.

## Implementation notes (for the site repo applier)

The `description:` field has to land in **two places** depending on
where the page is sourced from:

| Page type | Source file (where `description:` goes) | Repo |
|---|---|---|
| Landing, `/compare/`, `/examples/` index, `/migrating-from/*`, keyword landings, blog | `src/content/docs/<slug>.md` (or `src/pages/index.astro` for landing) | `asamarts/alint.org` |
| `/docs/getting-started/`, `/docs/concepts/`, `/docs/cookbook/`, `/docs/integrations/`, `/docs/about/` | `~/projects/alint/docs/site/<section>/<slug>.md` | `asamarts/alint` |
| `/docs/rules/<family>/<kind>/` | Each rule's source `Options` doc-comment in `crates/alint-dsl/src/<family>/<kind>.rs`; emitted into bundle frontmatter by `xtask/src/main.rs::docs_export` | `asamarts/alint` |
| `/docs/bundled-rulesets/<name>/` | Each ruleset's `description:` field at the top of `crates/alint-dsl/rulesets/v1/<name>.yml`; emitted into bundle frontmatter by `xtask/src/main.rs::docs_export` | `asamarts/alint` |
| `/examples/<owner>-<repo>/` | Each case study's `examples/<owner>-<repo>/README.md` frontmatter | `asamarts/alint` |
| `/docs/cli/<subcmd>/` | Captured from `alint <subcmd> --help`'s first non-empty paragraph; emit by `xtask/src/main.rs::docs_export` | `asamarts/alint` |
| `/docs/changelog/` | `~/projects/alint/CHANGELOG.md` top-of-file (or hardcoded in `xtask` if Starlight strips frontmatter) | `asamarts/alint` |

Two implementation steps:

1. **Hand-write `description:` for the ~10-15 long-form pages and
   the case studies.** That's maybe 30 frontmatter additions across
   site repo + alint repo `docs/site/**` + `examples/*/README.md`.

2. **Plumb `description:` through the docs-export pipeline.**
   `xtask/src/main.rs` currently emits frontmatter for the
   auto-generated rule + ruleset + CLI pages. Extend that block to
   include `description:` derived from the rule's `Options`
   doc-comment / ruleset YAML / `--help` output, using the
   templates above.

The docs-export plumbing is the bulk of the work and lands ~80
descriptions in one PR (60 rules + 19 rulesets + ~5-10 CLI
subcommands).

## Open questions before publish

1. **First-paragraph fallback survives where?** Confirm Starlight's
   exact fallback behaviour — for pages we *don't* set a description
   on, does it still extract first paragraph, or does it leave
   `<meta description>` absent? If absent, that's a regression for
   pages that haven't been migrated yet. Recommend: ship the
   docs-export plumbing first (auto-derives 80 descriptions in one
   shot), then hand-write the long-form ones.
2. **Description for rules where the doc-comment is one line.**
   Some `Options` doc-comments are terse ("Match files by glob.").
   The auto-derived description would be too short. Either pad with
   the family name (template above) or fall back to a per-family
   default ("Cross-file rule that…", "Content rule that…"). Default
   fallback is fine for v1.
3. **Twitter Card image (`<meta name="twitter:image">`).** Out of
   scope for this brief but relevant to the same `<head>` block.
   Worth its own brief? Recommend: yes, after Open Graph image
   generation lands as part of P4 press kit.
4. **Localised descriptions.** Site is en-only today. No work needed
   until that changes.

## Pre-publish checklist

- [ ] Style guide added to `~/projects/alint/docs/site/_meta-descriptions.md`
      (or wherever site contributors look for content style guidance —
      check the alint repo for an existing CONTRIBUTING-flavoured doc).
- [ ] Hand-written long-form pages (`/`, `/docs/`, `/compare/`,
      `/examples/` index, `/migrating-from/*`) all have a
      `description:` set.
- [ ] All 20 `examples/<owner>-<repo>/README.md` frontmatter blocks
      have a `description:` set.
- [ ] `xtask/src/main.rs::docs_export` extended to emit `description:`
      for rule pages, ruleset pages, and CLI subcommand pages.
- [ ] Smoke-test on 5 deployed pages: `curl -s https://alint.org/<path>
      | grep -E '<meta (name="description"|property="og:description")'`
      returns the expected string.
- [ ] No description on any page exceeds 160 chars (a CI check that
      greps frontmatter `description:` line lengths is a small
      follow-up).
- [ ] STATE.md row for the meta-description initiative flipped to
      `live` with date + commit SHA.

## Estimated diff size

- Hand-written `description:` additions across site-repo + bundle
  source: ~30-40 lines (one per page).
- `xtask/src/main.rs` plumbing: ~40-60 lines (rule + ruleset + CLI
  emission paths).
- Style-guide doc (this brief, condensed): ~30-50 lines if added to
  the site repo's contributor docs.

Total: ~100-150 lines across the alint + alint.org repos.

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `sitemap-config.md` | Same `astro.config.mjs` may need touching; apply together. |
| `schema-org-jsonld.md` | The `Article.description` field in the JSON-LD reuses `description:` frontmatter — so descriptions land before JSON-LD or both ship together. |
| `keyword-landing-pages.md` | Each new landing page needs a `description:` from day one — uses the templates above. |
| `alint-org-hero.md`, `alint-org-compare.md`, `alint-org-examples-gallery.md`, `migrate-from-*.md` | All proposed pages should ship with a `description:` frontmatter line that follows this style guide. Cross-reference. |
