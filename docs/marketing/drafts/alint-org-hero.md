---
destination: alint.org/ (site repo's index page — Astro/Starlight)
status: drafting
blocks_on: coordinated publish with readme-hero.md so messaging matches across surfaces
last_touched: 2026-05-06
---

# alint.org hero refresh — content brief for the site repo

## Why

The current alint.org landing hero is *"Lint the shape of your repo."*
That's serviceable but **buries the two strongest differentiators** that
the README leads with:

1. **Repolinter-replacement positioning** — only mentioned in the "Why a
   separate tool" section halfway down the page. Repolinter was archived
   in early 2026; users actively shopping for a replacement need to see
   that signal in the hero.
2. **Agent-aware angle** — present in the value props but not in the
   headline. The bundled `agent-hygiene` and `agent-context` rulesets +
   the dedicated `agent` output format are unique among
   structural-validation tools.

Also missing from the current landing:

- **Version badge** — readers can't tell what alint version the docs
  reflect.
- **Case studies** — the 20 P2a case studies are completely invisible
  on alint.org. Direct gap.
- **Speed claim with a number** — README leads with concrete latency
  ("1.1s @ 100K, 12s @ 1M"); landing hero is silent.

This brief proposes a hero refresh that **mirrors the README's
messaging**, so a visitor reading both surfaces gets a consistent
mental model.

## Proposed hero block

Goes at the top of the alint.org landing page (above the existing
"Install" section, replacing the current single-line
"Lint the *shape* of your repo." headline).

```markdown
# alint

[![Version](https://img.shields.io/crates/v/alint.svg?label=version)](https://crates.io/crates/alint)
[![License](https://img.shields.io/crates/l/alint.svg)](https://github.com/asamarts/alint#license)

## Fast, language-agnostic linter for repository structure, files, and content.

Declare the shape your repo should have — required files, filename
conventions, content patterns, values inside `package.json` /
`Cargo.toml` / GitHub workflows, cross-file relationships — in a
single `.alint.yml`. alint enforces it.

- ⚡ **Fast at scale.** ~1.1 s on a 100K-file workspace bundle, ~12 s
  at 1M files. [Public benchmarks per release](/docs/benchmarks/).
- 🤖 **Agent-aware.** First-class `agent` output format with
  per-violation `agent_instruction` strings; bundled `agent-hygiene`
  and `agent-context` rulesets for AI-touched repos.
- 🧰 **Powerful + extensible.** 60 rule kinds across 13 families, 19
  bundled ecosystem rulesets, 12 auto-fix ops, 8 output formats,
  structured-query rules with full RFC 9535 JSONPath, cross-file
  relational rules, conditional `when:` gates, and `extends:`
  composition with SRI-pinned URLs.
- 📦 **One static Rust binary.** Any language, any repo. No plugin
  install, no Node/JVM/Python runtime needed.

> alint fills the active-maintenance gap left when
> [Repolinter](https://github.com/todogroup/repolinter) was archived in
> early 2026, with a superset of its rule catalogue plus first-class
> cross-file, conditional-rule, structured-query, and agent-aware
> primitives.
```

## New section: "Proven on real OSS repos"

Insert immediately below the hero, above the existing install snippet.
Mirrors the equivalent README section so the two surfaces tell the
same story.

```markdown
## Proven on 20 real OSS repos

alint configs covering the structural-validation surfaces of:

**Single-language workspaces** —
[kubernetes](/examples/kubernetes-kubernetes/),
[rust-lang/rust](/examples/rust-lang-rust/),
[golang/go](/examples/golang-go/),
[python/cpython](/examples/python-cpython/),
[nodejs/node](/examples/nodejs-node/),
[apache/airflow](/examples/apache-airflow/),
[denoland/deno](/examples/denoland-deno/),
[tokio-rs/tokio](/examples/tokio-rs-tokio/),
[astral-sh/uv](/examples/astral-sh-uv/),
[astral-sh/ruff](/examples/astral-sh-ruff/),
[clap-rs/clap](/examples/clap-rs-clap/),
[microsoft/typescript](/examples/microsoft-typescript/),
[facebook/react](/examples/facebook-react/),
[prettier/prettier](/examples/prettier-prettier/),
[pnpm/pnpm](/examples/pnpm-pnpm/),
[helm/helm](/examples/helm-helm/),
[pytorch/pytorch](/examples/pytorch-pytorch/),
[vercel/turbo](/examples/vercel-turbo/).

**Polyglot monorepos** —
[apache/arrow](/examples/apache-arrow/) (6 languages — C++/Java/Python/Rust/Go/JS),
[vercel/next.js](/examples/vercel-next.js/) (TypeScript + Rust hybrid).

[Browse the full case-study gallery →](/examples/)
```

(Depends on the `/examples/` route existing — that's
`alint-org-examples-gallery.md`'s deliverable. If the gallery isn't
ready at publish time, the in-section repo links can resolve to
`https://github.com/asamarts/alint/tree/main/examples/<owner>-<repo>`
as a fallback.)

## Existing CTAs — keep verbatim

The current page's CTA block (Install + Browse cookbook + inline
install snippet for brew/npm/curl/docker) is good and stays.
Quickstart and "Star on GitHub" CTAs at the bottom of the landing
also stay.

## What changes vs. current alint.org

| Element | Current | Proposed |
|---|---|---|
| Headline | "Lint the *shape* of your repo." | "Fast, language-agnostic linter for repository structure, files, and content." |
| Version badge | absent | crates.io badge |
| Speed claim | absent from hero | "~1.1 s on 100K, ~12 s on 1M" in the bullet |
| Agent-aware angle | listed as one of many bullets | promoted to a top-of-bullets header |
| Repolinter framing | "Why a separate tool" section halfway down | hero blockquote, immediately after the bullets |
| Case studies | not linked anywhere | new section with all 20 + gallery link |

## What stays the same on alint.org

- Cookbook page (excellent reference content; no churn needed)
- Per-rule pages under `/docs/rules/<family>/<kind>/` (auto-generated)
- Install snippet block (works as-is)
- Site-wide nav + theme + Astro/Starlight infrastructure

## Implementation notes (for the site repo)

The alint.org repo is a separate Astro/Starlight site that pulls docs
from the `docs-bundle` branch of `asamarts/alint` at build time. The
landing page's source likely lives at `src/pages/index.astro` or
`src/content/landing.md` (depending on how the site is structured).
Whoever applies this brief:

1. Identifies the landing-page source file.
2. Replaces the hero block (lines covering the current
   "Lint the shape of your repo." headline + the existing bullets)
   with the proposed hero block above.
3. Inserts the "Proven on 20 real OSS repos" section between the
   hero and the existing install snippet.
4. Verifies the version badge URL renders cleanly with Starlight's
   markdown image handling (Starlight is generally permissive but
   the shields.io URL has query params that may need
   markdown-escaping).
5. Tests link resolution: the `/examples/` link will 404 until
   `alint-org-examples-gallery.md` ships — see "fallback strategy"
   above.

## Open questions before publish

1. **Version badge text format.** crates.io shields ships either
   `crates.io 0.9.15` or `version 0.9.15`. The proposed
   `?label=version` produces the latter — confirm the cleaner
   rendering with the site's theme.
2. **Speed numbers.** The current numbers (1.1s @ 100K, 12s @ 1M)
   come from v0.9.13 benches. v0.9.16 won't change the engine
   shape, so the numbers stand — but confirm at publish time.
3. **`/examples/` route.** If gallery draft ships with a different
   route (e.g. `/case-studies/`), update the "Browse the full
   case-study gallery" link.

## Pre-publish checklist

- [ ] `readme-hero.md` draft is in the same `ready` state so both
      surfaces publish coordinated.
- [ ] alint.org repo identified + landing-page source file located.
- [ ] Version badge renders cleanly under Starlight's theme.
- [ ] All 20 `/examples/<owner>-<repo>/` links work (or fallback
      to GitHub links if gallery not yet shipped).
- [ ] Repolinter framing blockquote renders with the site's
      blockquote styling (Starlight default is fine; just verify).
- [ ] Cloudflare Pages rebuild triggered after the site repo
      commits.
- [ ] STATE.md row for `alint.org/` flipped from `stale` to
      `live (just refreshed)` with date + commit SHA.

## Estimated diff size on the site repo

~30 lines changed in the landing-page source file (hero replaced,
bullets restructured, blockquote added) + ~25 lines added (new
"Proven on 20 real OSS repos" section). Net +25 to +55 lines
depending on how compact the existing source is.

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `readme-hero.md` | Both surfaces must publish together so messaging is consistent. README is the source-of-truth wording; alint.org mirrors. |
| `alint-org-examples-gallery.md` (next draft) | Provides the `/examples/` route this hero links to. Could ship this hero with GitHub-link fallbacks if gallery is delayed. |
| `alint-org-compare.md` (later draft) | The "Repolinter-replacement" positioning here points users somewhere; the compare page is where they go. Don't strictly block publish, but ideal to ship together. |
