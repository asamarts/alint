---
destination: alint.org site repo (Starlight base layout `<head>` + per-page templates)
status: drafting
blocks_on: meta-descriptions.md (the JSON-LD `description` fields reuse the same frontmatter); confirm Starlight's component-override path for injecting per-page `<head>` content
last_touched: 2026-05-06
---

# alint.org Schema.org JSON-LD — content brief

## Why

Schema.org JSON-LD is the structured-data format Google + Bing + most
AI ingestion crawlers parse to populate Rich Results (info boxes,
breadcrumb trails, software install panels) and to disambiguate page
intent. Per `launch-prep.md` (P3.2 + P3.3), three types are in scope:

| Type | What it expresses | Pages it appears on |
|---|---|---|
| [`SoftwareApplication`](https://schema.org/SoftwareApplication) | "alint is a software product" — name, version, license, OS, install URL | Landing only |
| [`Article`](https://schema.org/Article) | "this page is a long-form article" — headline, author, dates | Per-rule pages, per-ruleset pages, blog posts (when blog ships), case-study pages, migration guides |
| [`BreadcrumbList`](https://schema.org/BreadcrumbList) | "here's where this page sits in the nav hierarchy" | Every page two or more levels deep |

Without JSON-LD:

- We're invisible to the SoftwareApplication Rich Result panel (the
  one that shows version + license + install on a search result for
  the brand name).
- Crawler intent classification falls back to heuristics — the rule
  pages get treated as generic docs rather than as canonical
  references for each rule kind, which dilutes per-rule SERP
  positioning.
- Breadcrumb display in SERPs uses the URL path heuristically; a
  `BreadcrumbList` lets us control labels (e.g.,
  "alint > Documentation > Rules > Cross-file > every_matching_has"
  rather than the URL slug).

JSON-LD is the lowest-friction structured-data format (single
`<script>` tag in `<head>`) and the best-supported by both Google
and AI ingestion.

## Proposed JSON-LD snippets

All snippets below are ready to drop into the site repo's templates.
They use Starlight's component-override pattern — Starlight allows
overriding `Head.astro` (or injecting via `head:` frontmatter) per
page and globally.

### 1. `SoftwareApplication` — landing page only

Embed in the `<head>` of `https://alint.org/`:

```html
<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  "name": "alint",
  "alternateName": "alint repo linter",
  "description": "Fast, language-agnostic linter for repository structure, files, and content.",
  "applicationCategory": "DeveloperApplication",
  "applicationSubCategory": "Linter",
  "operatingSystem": "Linux, macOS, Windows",
  "downloadUrl": "https://github.com/asamarts/alint/releases/latest",
  "softwareVersion": "0.9.6",
  "softwareRequirements": "None (single static binary)",
  "url": "https://alint.org/",
  "sameAs": [
    "https://github.com/asamarts/alint",
    "https://crates.io/crates/alint"
  ],
  "license": "https://github.com/asamarts/alint/blob/main/LICENSE",
  "offers": {
    "@type": "Offer",
    "price": "0",
    "priceCurrency": "USD"
  },
  "author": {
    "@type": "Person",
    "name": "Aliaksandr Samartsau",
    "url": "https://github.com/asamarts"
  },
  "codeRepository": "https://github.com/asamarts/alint",
  "programmingLanguage": "Rust"
}
</script>
```

**Implementation:** the `softwareVersion` value should be templated
from a build-time variable that reads `~/projects/alint/Cargo.toml`'s
workspace version (the same one the README's version badge points
at). Site repo applier:

```js
// astro.config.mjs (or in a shared helper)
import { execSync } from 'node:child_process';

const ALINT_VERSION = (() => {
  try {
    // Read from the docs-bundle's version sidecar (recommended:
    // have docs-bundle workflow write `.alint-version` from
    // `cargo metadata --format-version 1 | jq -r '.workspace_members[0]'`)
    return readFileSync('src/content/docs/docs/.alint-version', 'utf8').trim();
  } catch {
    return '0.9.6'; // fallback: latest known
  }
})();
```

Then interpolate `${ALINT_VERSION}` into the JSON-LD on the landing
page.

### 2. `Article` — rule pages, ruleset pages, case studies, migration guides, blog posts

Template (per-page, with frontmatter substitution):

```html
<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "Article",
  "headline": "{{ title }}",
  "description": "{{ description }}",
  "url": "https://alint.org{{ path }}",
  "datePublished": "{{ date_published }}",
  "dateModified": "{{ lastmod }}",
  "author": {
    "@type": "Person",
    "name": "Aliaksandr Samartsau",
    "url": "https://github.com/asamarts"
  },
  "publisher": {
    "@type": "Organization",
    "name": "alint",
    "url": "https://alint.org/",
    "logo": {
      "@type": "ImageObject",
      "url": "https://alint.org/favicon.svg"
    }
  },
  "mainEntityOfPage": {
    "@type": "WebPage",
    "@id": "https://alint.org{{ path }}"
  }
}
</script>
```

**Per-page wiring:** a Starlight `Head.astro` override (in the site
repo's `src/components/`) reads the current page's frontmatter +
URL + last-mod, fills in the template, and emits the `<script>`.
Sketch:

```astro
---
// src/components/Head.astro (overrides Starlight's default)
import Default from '@astrojs/starlight/components/Head.astro';

const { entry } = Astro.props;
const path = new URL(Astro.url).pathname;
const isArticle =
  path.startsWith('/docs/rules/') ||
  path.startsWith('/docs/bundled-rulesets/') ||
  path.startsWith('/examples/') ||
  path.startsWith('/migrating-from/') ||
  path.startsWith('/blog/');

const articleJsonLd = isArticle ? {
  "@context": "https://schema.org",
  "@type": "Article",
  "headline": entry.data.title,
  "description": entry.data.description,
  "url": `https://alint.org${path}`,
  "datePublished": entry.data.datePublished ?? entry.data.lastmod,
  "dateModified": entry.data.lastmod,
  "author": {
    "@type": "Person",
    "name": "Aliaksandr Samartsau",
    "url": "https://github.com/asamarts"
  },
  "publisher": {
    "@type": "Organization",
    "name": "alint",
    "url": "https://alint.org/",
    "logo": { "@type": "ImageObject", "url": "https://alint.org/favicon.svg" }
  },
  "mainEntityOfPage": { "@type": "WebPage", "@id": `https://alint.org${path}` }
} : null;
---

<Default {...Astro.props}>
  <slot />
  {articleJsonLd && (
    <script type="application/ld+json" set:html={JSON.stringify(articleJsonLd)} />
  )}
</Default>
```

(Starlight's `Head.astro` override path is documented at
[Starlight component overrides](https://starlight.astro.build/guides/overriding-components/).)

### 3. `BreadcrumbList` — every page two or more levels deep

Template:

```html
<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "BreadcrumbList",
  "itemListElement": [
    { "@type": "ListItem", "position": 1, "name": "alint", "item": "https://alint.org/" },
    { "@type": "ListItem", "position": 2, "name": "Documentation", "item": "https://alint.org/docs/" },
    { "@type": "ListItem", "position": 3, "name": "Rules", "item": "https://alint.org/docs/rules/" },
    { "@type": "ListItem", "position": 4, "name": "Cross-file", "item": "https://alint.org/docs/rules/cross-file/" },
    { "@type": "ListItem", "position": 5, "name": "every_matching_has", "item": "https://alint.org/docs/rules/cross-file/every_matching_has/" }
  ]
}
</script>
```

**Per-page wiring:** derive from URL pathname + Starlight's sidebar
config (the sidebar already knows the section labels). Sketch:

```astro
// in the same Head.astro override
const segments = path.split('/').filter(Boolean);
const breadcrumbs = [
  { name: 'alint', url: '/' },
  ...segments.map((seg, i) => ({
    name: humanise(seg),  // 'getting-started' → 'Getting started'
    url: '/' + segments.slice(0, i + 1).join('/') + '/',
  })),
];

const breadcrumbJsonLd = breadcrumbs.length > 1 ? {
  "@context": "https://schema.org",
  "@type": "BreadcrumbList",
  "itemListElement": breadcrumbs.map((b, i) => ({
    "@type": "ListItem",
    "position": i + 1,
    "name": b.name,
    "item": `https://alint.org${b.url}`
  }))
} : null;

function humanise(slug) {
  return slug
    .replace(/-/g, ' ')
    .replace(/^./, c => c.toUpperCase());
}
```

For sections with friendlier labels than the auto-derived ones (e.g.,
`bundled-rulesets` → "Bundled rulesets" not "Bundled rulesets" — that
one's fine; but `cross-file` → "Cross-file" works too), the
auto-derive is enough. If we want to override a label, a small
slug-to-label map at the top of `Head.astro` covers the exceptions.

## Page-type emission matrix

| Page type | `SoftwareApplication` | `Article` | `BreadcrumbList` |
|---|---|---|---|
| Landing (`/`) | yes | no | no (top-level) |
| `/docs/` (docs index) | no | no | yes |
| `/docs/getting-started/`, `/docs/concepts/`, `/docs/cookbook/`, `/docs/integrations/`, `/docs/about/` | no | no (docs are reference, not articles) | yes |
| `/docs/rules/` (index) | no | no | yes |
| `/docs/rules/<family>/` (family index) | no | no | yes |
| `/docs/rules/<family>/<kind>/` | no | yes | yes |
| `/docs/bundled-rulesets/` (index) | no | no | yes |
| `/docs/bundled-rulesets/<name>/` | no | yes | yes |
| `/docs/cli/<subcmd>/` | no | no | yes |
| `/docs/changelog/` | no | no | yes |
| `/compare/` | no | yes | yes |
| `/examples/` (gallery index) | no | no | yes |
| `/examples/<owner>-<repo>/` | no | yes | yes |
| `/migrating-from/<source>/` | no | yes | yes |
| `/repolinter-alternative/`, `/monorepo-linter/`, `/agent-friendly-linter/`, `/language-agnostic-linter/`, `/repository-structure-linter/` | no | yes | yes (1-deep, so "alint > Repolinter alternative" — minimal but valid) |
| `/blog/<slug>/` (when blog ships) | no | yes | yes |

## Google Rich Results validator checklist

Use [Google Rich Results Test](https://search.google.com/test/rich-results)
+ [Schema Markup Validator](https://validator.schema.org/) post-deploy:

- [ ] Landing page: `SoftwareApplication` validates (no warnings).
      Confirm `softwareVersion` matches the live alint version.
- [ ] Landing page: `BreadcrumbList` is correctly absent (or
      explicitly suppressed for top-level routes).
- [ ] One rule page (e.g.,
      `/docs/rules/cross-file/every_matching_has/`):
      `Article` + `BreadcrumbList` both validate.
- [ ] One case study (e.g., `/examples/kubernetes-kubernetes/`):
      `Article` + `BreadcrumbList` both validate.
- [ ] One ruleset page (e.g., `/docs/bundled-rulesets/rust@v1/`):
      `Article` + `BreadcrumbList` both validate.
- [ ] No "Missing field" warnings on any of the above. Each
      `Article` has `headline`, `description`, `datePublished`,
      `dateModified`, `author`, `publisher`, `mainEntityOfPage`.
- [ ] `BreadcrumbList` items all use canonical absolute URLs (no
      `//` accidents).
- [ ] No duplicate JSON-LD scripts on any page (one
      `SoftwareApplication` on landing, one `Article` + one
      `BreadcrumbList` on each Article-eligible page).
- [ ] Schema.org validator flags zero "Severe error" findings on a
      sample of 5 pages (1 landing + 1 rule + 1 ruleset + 1 case
      study + 1 migration guide).

## Implementation notes (for the site repo applier)

1. **Confirm Starlight component override path.** Starlight v0.x
   exposes `Head.astro` for override; verify the version on the
   site repo and the matching override entry-point. Docs:
   <https://starlight.astro.build/guides/overriding-components/>.

2. **`SoftwareApplication.softwareVersion` source.** Single
   source of truth: `~/projects/alint/Cargo.toml`'s
   `[workspace.package].version`. Pipeline:
   - Add to docs-bundle workflow:
     `cargo metadata --format-version 1 --no-deps | jq -r '.packages[0].version' > target/docs-bundle/.alint-version`
   - Site repo's `astro.config.mjs` reads
     `src/content/docs/docs/.alint-version` at build time.
   - Fallback: hard-coded `0.9.6` (current latest).

3. **`Article.datePublished` vs `Article.dateModified`.**
   - `datePublished`: the page's first commit. For rule pages, the
     commit that introduced the rule (`git log --reverse --format=%cI
     -- crates/alint-dsl/src/<family>/<kind>.rs | head -1`). For
     case studies, the commit that introduced
     `examples/<owner>-<repo>/README.md`.
   - `dateModified`: the page's most recent commit (= the same
     `lastmod` value used by the sitemap brief).
   - For v1, both can = `lastmod` (i.e. the docs-bundle commit
     date). Wrong but harmless. For v2, plumb true `datePublished`
     through the bundle.

4. **No JSON-LD for short-form / generic pages.** The matrix above
   skips `Article` for `/docs/concepts/`, `/docs/cookbook/`, etc.
   Those pages are reference material, not articles. Emitting
   `Article` for them invites Google to demote the auto-eligible
   pages.

5. **CSP considerations.** `<script type="application/ld+json">` is
   allowed by default CSP. If the site repo eventually adds a
   strict CSP, ensure `script-src` permits inline JSON-LD (the
   `'unsafe-inline'` keyword OR a hash-based allow per script).

## Open questions before publish

1. **`Person` vs `Organization` for `author`.** Currently
   "Aliaksandr Samartsau" as `Person`. Once a `funding.yml`
   "alint org" identity exists (P5), might want to swap to
   `Organization`. v1: `Person` is honest. Don't need to
   pre-emptively switch.
2. **Add `FAQPage` schema for the migration guides?** The guides
   have a Q&A-ish structure ("How does X map to Y?"). FAQPage
   schema is eligible for FAQ rich results in SERPs. Worth a
   follow-up brief; OUT of MVP.
3. **`HowTo` schema for the cookbook entries?** Cookbook recipes
   have a step-by-step structure that maps to `HowTo`. Same
   answer as FAQPage — worth a follow-up; OUT of MVP.
4. **`Article` vs `TechArticle` for rule pages.** `TechArticle` is
   a valid subtype with fields like `proficiencyLevel` and
   `dependencies`. Probably overengineered for rule pages.
   Recommend stay on `Article`.
5. **Where to author `Person` URL.** Currently
   `https://github.com/asamarts`. If a personal site lands later,
   update once.

## Pre-publish checklist

- [ ] `Head.astro` override implemented in the site repo (with
      both `Article` and `BreadcrumbList` emission logic) and the
      auto-derive helper for breadcrumb labels covers all current
      sections.
- [ ] Landing page emits `SoftwareApplication` with the correct
      `softwareVersion` (sourced from the bundle's `.alint-version`
      sidecar or hard-coded fallback).
- [ ] Rule + ruleset + case-study + migration-guide pages emit
      `Article`.
- [ ] Every page two or more levels deep emits `BreadcrumbList`.
- [ ] Google Rich Results Test passes for: landing, 1 rule page,
      1 ruleset page, 1 case study, 1 migration guide.
- [ ] Schema.org Validator: zero severe errors across same 5
      sample pages.
- [ ] `description:` frontmatter exists on every page that emits
      `Article` (depends on `meta-descriptions.md`).
- [ ] STATE.md row for the JSON-LD initiative flipped to
      `live` with date + commit SHA.

## Estimated diff size on the site repo

- `src/components/Head.astro` (override): ~80-120 lines (the
  three JSON-LD blocks + the breadcrumb derive helper).
- `astro.config.mjs`: ~10 lines (the `ALINT_VERSION` lookup +
  any glue).
- (Optional) `src/lib/breadcrumb-labels.ts`: ~10 lines for the
  slug-to-label override map if the auto-derive isn't sufficient.

Total: ~100-140 lines on the site repo.

Plus ~3 lines on the alint repo's docs-bundle workflow to write
`.alint-version` sidecar (optional but recommended).

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `meta-descriptions.md` | `Article.description` reuses the same `description:` frontmatter. Don't ship JSON-LD before descriptions or `Article` blocks emit `null` descriptions for not-yet-migrated pages. |
| `sitemap-config.md` | `Article.dateModified` reuses the same `lastmod` source. Don't ship JSON-LD before sitemap, for the same reason. |
| `keyword-landing-pages.md` | Each new landing page emits `Article` + `BreadcrumbList` per the matrix. Brief depends on this one having shipped first. |
| `alint-org-hero.md` | The landing page's JSON-LD lands as part of this brief; the hero refresh in `alint-org-hero.md` must coexist. The two PRs touch the same `index` source file but different sections (one touches body, one touches `<head>`). |
