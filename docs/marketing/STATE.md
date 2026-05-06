# alint Marketing — State of the World

Single source of truth for **what marketing exists, where it lives, and
what's a draft vs. published**. Maintained alongside engineering work in
`docs/launch-prep.md`. Update this file whenever a draft moves status,
a surface gets refreshed, or a new surface joins the inventory.

## How this doc works

Each marketing surface is one row. Status uses the small vocabulary:

| Status | Meaning |
|---|---|
| **live** | Currently published / deployed; what the public sees today |
| **stale** | Live but known out-of-date (specific drift documented) |
| **drafting** | Active draft under `docs/marketing/drafts/`; not yet published |
| **ready** | Draft complete + reviewed; awaiting publish trigger |
| **planned** | On the roadmap; no draft yet |

Drafts live under `docs/marketing/drafts/<surface>.md` with frontmatter
documenting status + intended destination + dependencies (e.g., "blocks
on v0.9.15 release for the JSON-Schema link to resolve").

## Current marketing surfaces

> *Initial inventory — fetched 2026-05-06; will be refined by the
> alint.org subagent inventory landing alongside this doc.*

### Live surfaces

| Surface | Status | Last refresh | Owner | Notes |
|---|---|---|---|---|
| `README.md` (top-level alint repo) | **stale** | P1 hygiene pass (2026-05-05; commit `52e7494f`) | self | Hero rewritten in P1 with 4 value props + 60-second quickstart; version refs at v0.9.14. Needs v0.9.15 + 20-case-study + 5-narrative refresh in P3. |
| `https://alint.org/` (landing) | **stale** | last alint.org commit `0cb0779` per existing memory ("60 rule kinds across thirteen families" intro for v0.9.6) | self | Per existing memory: still uses pre-v0.9.6 framing; needs hero rewrite + comparison table + examples gallery + v0.9.6 → v0.9.15 docs roll. Subagent inventory in progress. |
| `https://alint.org/docs/` (docs gallery) | **live** | rolled per docs-bundle workflow | site repo | Auto-synced from `docs-bundle` branch via Cloudflare Pages deploy hook (per existing memory: alint.org docs pipeline). Manifest version-stamped at the docs-bundle workflow's last successful run. |
| `https://alint.org/docs/rules/` | **live** | per docs-bundle | site repo | Per-rule documentation pages. Auto-generated. |
| `https://alint.org/docs/rules/<family>/<kind>/` | **live** | per docs-bundle | site repo | Per-rule pages. e.g. `git-hygiene/commented_out_code/`. |
| GitHub repo About (description / homepage / topics) | **live** | P1 hygiene (2026-05-05) | self | All three set in P1; no immediate refresh needed. |

### Pending inventory

The alint.org subagent inventory is in flight — it will refresh the
"live" rows above with concrete claims (hero text, value-prop list,
version refs, sitemap presence, robots.txt content, llms.txt presence,
`.well-known/security.txt` presence) so drafts target the actual
baseline rather than memory.

## Drafts roadmap

Drafts under `docs/marketing/drafts/` correspond to P3 phases.

### P3.1 — Hero + content (drafts)

| Draft | Destination | Status | Notes |
|---|---|---|---|
| `readme-hero.md` | top-level `README.md` | **planned** | 5-line punch + quickstart + 5-narrative reference + 20-case-study link. |
| `alint-org-hero.md` | `alint.org/` (site repo `index.md` or equivalent) | **planned** | Match README messaging; CLI demo asciinema/GIF. |
| `alint-org-compare.md` | `alint.org/compare/` | **planned** | Direct table: alint vs Repolinter (archived 2026-02), ls-lint, Megalinter, EditorConfig, custom shell. |
| `alint-org-examples-gallery.md` | `alint.org/examples/` | **planned** | Gallery of the 20 P2a case studies, organised by the 5 positioning narratives (mirrors `examples/README.md`). |
| `alint-org-benchmarks.md` | `alint.org/benchmarks/` | **planned** | Public-facing version of `docs/benchmarks/HISTORY.md`. |
| `migrate-from-repolinter.md` | `alint.org/migrating-from/repolinter/` | **planned** | Step-by-step. Repolinter archived 2026-02 — high-intent search target. |
| `migrate-from-ls-lint.md` | `alint.org/migrating-from/ls-lint/` | **planned** | Step-by-step. |
| `migrate-from-custom-bash.md` | `alint.org/migrating-from/custom-bash-scripts/` | **planned** | Step-by-step; this is the kubernetes "50→17" story generalised. |

### P3.2 — SEO infrastructure (drafts)

| Draft | Destination | Status | Notes |
|---|---|---|---|
| `sitemap-config.md` | `alint.org` build-time config | **planned** | Auto-generation strategy. |
| `robots-txt.md` | `alint.org/robots.txt` | **planned** | Explicit AI-crawler rules (allow GPTBot, ClaudeBot, anthropic-ai, CCBot, Google-Extended, PerplexityBot, Applebot-Extended, meta-externalagent). |
| `meta-descriptions.md` | per-page frontmatter additions | **planned** | Per-page `<title>` + `<meta description>` strategy. |
| `schema-org-jsonld.md` | site-wide template | **planned** | `SoftwareApplication`, `Article`, `BreadcrumbList` JSON-LD. |
| `keyword-landing-pages.md` | `alint.org/<keyword>/` per-page drafts | **planned** | `/repolinter-alternative`, `/monorepo-linter`, `/agent-friendly-linter`, `/language-agnostic-linter`, `/repository-structure-linter`. |

### P3.3 — AI/LLM discovery (drafts)

| Draft | Destination | Status | Notes |
|---|---|---|---|
| `llms-txt.md` | `alint.org/llms.txt` | **planned** | llmstxt.org standard. H1 + summary + H2-section bullet-list links. |
| `llms-full-txt.md` | `alint.org/llms-full.txt` | **planned** | Companion: same content with all linked docs inlined. |
| `well-known-security-txt.md` | `alint.org/.well-known/security.txt` | **planned** | RFC 9116. |
| `well-known-ai-txt.md` | `alint.org/.well-known/ai.txt` | **planned** | We opt in. |
| `releases-atom.md` | `alint.org/releases.atom` | **planned** | RSS/Atom for releases. |
| `api-endpoints.md` | `alint.org/api/{rules,rulesets,versions}.json` | **planned** | Programmatic catalogue discovery. |

### Press kit (P4 dependency)

| Draft | Destination | Status | Notes |
|---|---|---|---|
| `branding/` | top-level `branding/` dir in alint repo | **planned** | Logo SVGs, screenshots, GIFs, OG images. |

### Launch posts (P4 dependency)

| Draft | Destination | Status | Notes |
|---|---|---|---|
| `launch-post-hn.md` | "Show HN" submission | **planned** | "Show HN: alint, a fast linter for repo structure" — leads with the kubernetes/arrow/golang-go data. |
| `launch-post-r-rust.md` | r/rust | **planned** | Tailored angle. |
| `launch-post-lobsters.md` | Lobsters | **planned** | Tailored angle. |
| `launch-post-dev-to.md` | dev.to | **planned** | "How we replaced 50 verify scripts with one .alint.yml" — uses kubernetes case study as the hook. |

## Update protocol

When you update this file:

1. Move the row's status (e.g. `planned` → `drafting` when the draft file
   is created).
2. Note the draft filename in the row.
3. If a draft moves to `ready`, link the PR or commit that's blocking on
   publish.
4. If a draft moves to `live`, capture the publish commit/SHA + date.
5. If a `live` row drifts (new feature shipped, version bump), flip it to
   `stale` and open a row for the refreshing draft.

Keep this doc terse — one row per surface, link out for detail. Long-form
drafts go in `docs/marketing/drafts/<name>.md`.
