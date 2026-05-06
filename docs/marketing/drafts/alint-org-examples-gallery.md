---
destination: alint.org/examples/ (new route on the site repo) + 20 sub-routes at alint.org/examples/<owner>-<repo>/
status: drafting
blocks_on: site repo content-pipeline change — ingest examples/*/README.md from asamarts/alint into the docs-bundle so each renders as a sub-page; coordinated publish with alint-org-hero.md (which links here)
last_touched: 2026-05-06
---

# alint.org/examples/ gallery — content brief for the site repo

## Why this exists

The alint.org inventory (subagent fetch 2026-05-06) confirmed: **no case
studies are linked anywhere on alint.org docs or landing.** Yet the 20
P2a case studies are alint's most marketable artifact — real repos,
working configs, live findings against the actual upstream tree. This
gap is the inventory's #1 strategic finding.

The new `/examples/` route closes the loop:

- `alint-org-hero.md` already links *"Browse the full case-study
  gallery →"* to `/examples/` — this draft delivers that page.
- The local repo's `examples/README.md` already lists all 20 case
  studies organised by the 5 positioning narratives — alint.org should
  mirror that structure for cross-surface consistency.
- Each case study's `README.md` is launch-quality long-form content that
  Starlight can render directly — the docs-bundle pipeline just needs to
  ingest `examples/*/README.md` alongside the existing `docs/` content.

## Proposed structure

Two layers:

### 1. Gallery index — `alint.org/examples/`

Single page with the 20 case studies presented as cards, organised by
positioning narrative. Hero block at the top sets context; cards below.

### 2. Per-case-study pages — `alint.org/examples/<owner>-<repo>/`

20 auto-generated pages, one per case study, each rendering the
corresponding `examples/<owner>-<repo>/README.md` from the alint repo.
The docs-bundle pipeline already syncs from `asamarts/alint` at build
time (per existing memory: alint.org docs pipeline) — extending it to
ingest `examples/*/README.md` is the smallest possible pipeline change.

## Proposed gallery page content

```markdown
---
title: Case studies
description: Working alint configurations from 20 production OSS repos.
---

# 20 case studies. 5 shapes of project.

The launch-prep validation pass took alint to 20 popular OSS
repositories — single-language workspaces, polyglot mega-monorepos,
mature script-heavy projects, tightly-curated minimal-tooling
projects. Each case study is a working `.alint.yml` you can copy as a
starting point + a writeup explaining what alint catches that the
repo's existing tooling doesn't.

The cases cluster into five distinct "shapes" of project — find the
shape closest to yours, start there.

---

## 1. Repos with verify-script sprawl

*"Replaces the structural subset of N hand-rolled validation scripts."*

Best fit: projects whose CI has accumulated a `hack/verify-*.sh` /
`scripts/check-*.py` directory over the years. alint consolidates the
declarative subset into one config; the AST/runtime-aware checks stay
where they are.

| Repo | Headline finding |
|---|---|
| [kubernetes/kubernetes](/examples/kubernetes-kubernetes/) | 50 verify scripts inventoried; alint replaces 17 declaratively. |
| [apache/airflow](/examples/apache-airflow/) | 109 pre-commit hooks; ~40 % map to alint. |
| [python/cpython](/examples/python-cpython/) | 56 surfaces consolidated; the canonical-shaped surface (`Misc/NEWS.d/next/*` filename grammar) was enforced nowhere statically before — alint encodes the full grammar in 6 lines. |

---

## 2. Repos that rely on convention without explicit checks

*"Catches the conventions your pipeline assumes but doesn't verify."*

Best fit: projects with a clean, well-curated CI that *implicitly*
depends on conventions nothing in the pipeline actually checks. alint
makes the implicit explicit.

| Repo | Headline finding |
|---|---|
| [tokio-rs/tokio](/examples/tokio-rs-tokio/) | Zero hand-rolled scripts; alint catches 15 conventions tokio's pipeline silently assumes. |
| [astral-sh/uv](/examples/astral-sh-uv/) | 67-crate workspace conventions (`[lints] workspace = true`, `edition.workspace = true`, `license.workspace = true`, README per crate) enforced **nowhere in CI today**. |
| [pnpm/pnpm](/examples/pnpm-pnpm/) | Replaces the in-tree `meta-updater` plugin's 13 cross-package field invariants with declarative rules — no per-repo plugin install needed. |
| [facebook/react](/examples/facebook-react/) | `codes.json` registry shape + `ReactVersion.js` propagated to 3 per-package version fields. Live findings against the repo: 1 wrong `repository.directory`, 19 non-canonical `bugs` URLs, 345 source files missing the canonical Meta copyright header, 39 packages without per-package LICENSE. |
| [nodejs/node](/examples/nodejs-node/) | 15-year-old conventions enforced via human review only — `test/parallel/test-*.{js,mjs,cjs}` filename grammar (a misnamed test silently drops out of test discovery), per-major `CHANGELOG_V<N>.md`. |

---

## 3. Repos with mature tooling that lacks a structural layer

*"Adds a structural floor on top of mature tooling."*

Best fit: projects whose lint/format stack is already tight (eslint +
prettier + dprint + knip / golangci-lint / etc.) but whose
*structural* validation is missing or scattered. alint sits on top.

| Repo | Headline finding |
|---|---|
| [microsoft/typescript](/examples/microsoft-typescript/) | eslint + dprint + knip already tight; alint adds structural floor (header consistency, baseline file pairing, plugin pinning, action SHA pinning). |
| [astral-sh/ruff](/examples/astral-sh-ruff/) | 900+ Python lint rules but **zero rules for ruff's own internal-crate `publish = false` discipline.** Day-one alint win. |
| [prettier/prettier](/examples/prettier-prettier/) | Mature dogfooded tooling (eslint + prettier + cspell + knip + tsc + 5 custom node scripts) — and zero on-disk enforcement of its per-language-plugin convention discipline. alint adds 5 net-new structural gates. |
| [helm/helm](/examples/helm-helm/) | Trojan-Source defence + GHA hardening on top of golangci-lint. Live findings: a U+200B zero-width char in `internal/plugin/plugin.go:80`, 5 workflows missing `permissions.contents: read`. |

---

## 4. Repos that built their own lint-orchestration tool

*"Replaces the structural subset of your custom orchestration layer."*

Best fit: projects whose orchestration needs outgrew off-the-shelf
tools and led to a custom orchestrator (lintrunner, etc.). alint slots
underneath; the orchestrator keeps the AST-aware tail.

| Repo | Headline finding |
|---|---|
| [pytorch/pytorch](/examples/pytorch-pytorch/) | ≈86 % of pytorch's 57 `lintrunner.toml` adapters are structural. alint sits beneath as the structural floor; lintrunner keeps the AST-aware tail. |

---

## 5. Tightly-curated minimal-tooling projects

*"Encodes conventions enforced only by code-review discipline."*

Best fit: projects whose maintainers enforce structure by reading
every PR, with no automated checks on top. alint encodes that
discipline into a config that runs in seconds.

| Repo | Headline finding |
|---|---|
| [golang/go](/examples/golang-go/) | Zero `.github/workflows/`, zero `Makefile`, zero `.golangci.yml`. The 31-rule alint config encodes Russ Cox & co.'s structural contract — for the first time anywhere in the project. |
| [rust-lang/rust](/examples/rust-lang-rust/) | `src/tools/tidy/` is a custom Rust binary doing exactly alint's job. ~13 of ~32 tidy checks become declarative; the rest stay on `./x test tidy`. |

---

## Polyglot wins (anticipating P2b)

These two case studies run ahead of alint's polyglot-monorepo wave —
and may end up driving the launch-marketing message.

| Repo | Headline finding |
|---|---|
| [apache/arrow](/examples/apache-arrow/) | **Flagship polyglot case.** 6 languages in one tree (C++/Java/Python/Rust/Go/JS), 21 lint hooks across 14 tool repos, 0 tools that see cross-language conventions — alint is the layer that does. Live findings: 16 source files missing the Apache header (all listed in `dev/release/rat_exclude_files.txt`). |
| [vercel/next.js](/examples/vercel-next.js/) | First hybrid pnpm + Cargo dual-workspace win. *"Drift no per-language linter catches because each linter only sees half the tree."* |

---

## Other case studies

| Repo | Notes |
|---|---|
| [denoland/deno](/examples/denoland-deno/) | Rust + JS + TS multi-language; custom validation scripts. |
| [vercel/turbo](/examples/vercel-turbo/) | Rust monorepo orchestrator; alint adds 22 gates that don't exist. |
| [clap-rs/clap](/examples/clap-rs-clap/) | Rust workspace; per-member inheritance via `for_each_dir` over family crates. |

---

## Using a case study as a starting point

Each `/examples/<owner>-<repo>/` page includes the working `.alint.yml`
inline. To bootstrap your own repo:

```sh
curl -fsSL https://raw.githubusercontent.com/asamarts/alint/main/examples/<owner>-<repo>/.alint.yml \
  > .alint.yml
alint check
```

Then trim what doesn't apply, add what's specific to your repo. The
configs are deliberately written to be readable + adaptable, not
minimal.

## Coming next: P2b

20 polyglot monorepos (bazel, vscode, angular, nx, electron,
tensorflow, spark, beam, prisma, temporal, istio, grafana,
cockroachdb, directus, supabase, nixpkgs, terraform, flutter, dotnet,
protobuf) are queued as ongoing post-launch evidence-driven content.
Watch [the blog](/blog/) for case studies as they ship.
```

## Implementation notes (for the site repo)

### Pipeline change — minimum viable

The docs-bundle workflow on `asamarts/alint` currently syncs `docs/` →
`docs-bundle` branch. Extend it to also sync `examples/*/README.md`,
mapped as:

| Source | Destination on docs-bundle |
|---|---|
| `examples/<owner>-<repo>/README.md` | `examples/<owner>-<repo>/index.md` (so Starlight renders it as `/examples/<owner>-<repo>/`) |
| `examples/<owner>-<repo>/.alint.yml` | `examples/<owner>-<repo>/alint.yml` (referenced from the README via relative link; rendered as a code block by Starlight's syntax-highlighter) |

Each case-study `README.md` already includes a "config" section that
embeds the YAML inline OR links to `.alint.yml`. The latter is cleaner
for the site (separate file you can copy with one click); the former is
denser for the GitHub view. **Recommendation**: keep both — README has
inline YAML for GitHub readers; site renders README + offers a
*"Download .alint.yml"* button on each page.

### Frontmatter for per-case-study pages

The docs-bundle sync should prepend frontmatter to each ingested README:

```yaml
---
title: <owner>/<repo> case study
description: <pulled from the README's "Headline finding" section>
sidebar:
  label: <owner>/<repo>
---
```

The sidebar grouping puts all 20 under a "Case studies" parent in
Starlight's left nav.

### Sidebar nav

Add to the site's `astro.config.mjs` (or wherever sidebar groups are
defined):

```js
{
  label: 'Case studies',
  items: [
    { label: 'Browse all 20', link: '/examples/' },
    {
      label: 'By narrative',
      items: [
        // collapsible sub-groups; one per narrative
      ],
    },
  ],
},
```

Or simpler MVP: just one link in the top-level nav to `/examples/`.

## Open questions before publish

1. **Per-case-study route shape.** `/examples/<owner>-<repo>/` (using
   the local-repo dir name) vs `/examples/<owner>/<repo>/` (using
   GitHub-style). The local repo uses the former; recommend the site
   matches for symmetry. Could redirect the latter for SEO if both
   patterns get inbound links.
2. **Live findings prose.** Some case studies have *very* compelling
   live findings (react: 345 source files missing copyright header;
   helm: U+200B zero-width char). Worth pulling those into the
   gallery cards as the headline (currently buried inside
   per-case-study pages). The brief already does this for ~half the
   cases — the other half could be lifted at publish time.
3. **OG images per case study.** Out of scope for MVP — generic OG
   image is fine. Per-case-study OG would be a P5 polish.

## Pre-publish checklist

- [ ] `alint-org-hero.md` is `ready` so both surfaces publish
      coordinated (the hero links here).
- [ ] docs-bundle workflow on `asamarts/alint` updated to sync
      `examples/*/README.md` alongside `docs/`.
- [ ] Site repo's sidebar config updated to surface the new route.
- [ ] All 20 `/examples/<owner>-<repo>/` routes verified to render.
- [ ] Each case-study README's relative links (e.g. `[`docs/rules.md`](../../docs/rules.md)`)
      verified to resolve correctly under the alint.org URL space —
      they may need rewriting from `../../docs/` to `/docs/` at
      ingestion time.
- [ ] Cloudflare Pages rebuild triggered.
- [ ] STATE.md row for `alint.org/docs/` (which currently has no
      case-study link) updated to note the new gallery exists.

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `alint-org-hero.md` | Hero links to `/examples/` — must publish coordinated. |
| `readme-hero.md` | README's "Proven on 20 real OSS repos" section uses local-repo paths (`examples/<owner>-<repo>/`); alint.org gallery uses site-relative paths (`/examples/<owner>-<repo>/`). Both correct for their surface. |
| Future `alint-org-compare.md` | The gallery's "shapes of project" framing dovetails with the comparison page's "shapes of tool" framing. Cross-link when both ship. |

## Estimated diff size on the site repo

- 1 new gallery page: ~200 lines of Starlight-ready markdown
- Sidebar config: ~10 lines
- Pipeline change: ~10-30 lines depending on whether the docs-bundle
  workflow already supports glob ingestion
- 20 per-case-study pages: zero new content (auto-rendered from
  existing examples/*/README.md)

Total net addition: ~220-240 lines on the site repo + a small
workflow change on the alint repo.
