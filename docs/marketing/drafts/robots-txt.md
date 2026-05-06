---
destination: alint.org/robots.txt (site-repo public root) + Cloudflare configuration override
status: drafting
blocks_on: Cloudflare-managed robots.txt override mechanism (page rule OR worker OR Bot Management config — see "Cloudflare coordination" section); coordinated publish with `well-known-ai-txt.md` so the two AI-posture surfaces match
last_touched: 2026-05-06
---

# alint.org/robots.txt — content brief for the site repo

## Why

The current `https://alint.org/robots.txt` is **Cloudflare-managed**
and reflects the platform-default AI-blocking posture:

```text
# Current state — Cloudflare-managed, AI-blocking
User-Agent: *
Allow: /

User-Agent: ClaudeBot
Disallow: /
User-Agent: GPTBot
Disallow: /
User-Agent: CCBot
Disallow: /
User-Agent: Bytespider
Disallow: /
User-Agent: Google-Extended
Disallow: /
User-Agent: meta-externalagent
Disallow: /
User-Agent: Applebot-Extended
Disallow: /
User-Agent: Amazonbot
Disallow: /
User-Agent: CloudflareBrowserRenderingCrawler
Disallow: /

# Header (Cloudflare-injected):
content-signal: search=yes,ai-train=no
```

This was Cloudflare Pages' default — opt-out of AI training, allow
human-search-engine indexing.

**The AI-training posture decision logged 2026-05-06 was: opt IN.**
Rationale (recapped from `reference_alint-marketing-tracking.md` and
the marketing-tracking memory):

1. **alint's positioning is agent-aware.** The bundled
   `agent-hygiene` and `agent-context` rulesets + the `agent` output
   format are unique differentiators. Being LLM-discoverable
   advances the launch mission, not vice versa.
2. **Discoverability matters more than copyright control.** The
   alint codebase is Apache-2.0 OR MIT licensed; the docs are
   permissively reusable. Blocking AI training doesn't protect
   anything we're protecting; allowing it puts alint into the
   training corpora of next-generation models, which is high-
   leverage long-term marketing.
3. **The runtime fetch concern is separate.** Allowing AI training
   doesn't mean allowing unbounded crawler load — that's bot-
   management's job (via Cloudflare's "verified bot" system + rate
   limiting), not robots.txt's. Crawler etiquette is preserved.
4. **`/llms.txt` and `/.well-known/ai.txt` complete the posture.**
   The three files together (robots.txt, llms.txt, ai.txt) make
   the opt-in machine-readable across three different
   conventions.

This brief implements the **opt-in flip** with the concrete
robots.txt body + the Cloudflare-coordination notes for the
applier.

## Proposed `/robots.txt` body

```text
# alint.org robots.txt
# Last updated: 2026-05-06
# Posture: opt IN to AI training and crawler runtime access.
#
# Rationale: alint's positioning includes agent-aware tooling
# (see /docs/output-formats/#agent and the bundled agent-* rulesets);
# being LLM-discoverable advances that mission. License is
# Apache-2.0 OR MIT — content is permissively reusable.
#
# See also:
#   /.well-known/ai.txt (per-use AI permissions, Spawning AI standard)
#   /llms.txt          (LLM context-discovery, llmstxt.org standard)
#   /sitemap-index.xml (URLs available for indexing)

User-Agent: *
Allow: /
Sitemap: https://alint.org/sitemap-index.xml

# Explicit allow for known AI-related crawlers — overrides any
# platform-default block that would otherwise apply. Listed in
# alphabetical order for maintenance.

User-Agent: Amazonbot
Allow: /

User-Agent: anthropic-ai
Allow: /

User-Agent: Applebot-Extended
Allow: /

User-Agent: Bytespider
Allow: /

User-Agent: CCBot
Allow: /

User-Agent: ClaudeBot
Allow: /

User-Agent: CloudflareBrowserRenderingCrawler
Allow: /

User-Agent: Google-Extended
Allow: /

User-Agent: GPTBot
Allow: /

User-Agent: meta-externalagent
Allow: /

User-Agent: PerplexityBot
Allow: /

# (No Disallow lines anywhere. The opt-in posture is broad.)
```

### What changed vs. current state

| Element | Current | Proposed | Why |
|---|---|---|---|
| `content-signal` header | `search=yes,ai-train=no` | `search=yes,ai-train=yes` | Implements the 2026-05-06 opt-in decision at the header layer (Cloudflare's content-signal system) |
| `User-Agent: ClaudeBot Disallow: /` | present | replaced with `Allow: /` | Opt-in flip |
| `User-Agent: GPTBot Disallow: /` | present | replaced with `Allow: /` | Opt-in flip |
| `User-Agent: CCBot Disallow: /` | present | replaced with `Allow: /` | Opt-in flip |
| `User-Agent: Bytespider Disallow: /` | present | replaced with `Allow: /` | Opt-in flip |
| `User-Agent: Google-Extended Disallow: /` | present | replaced with `Allow: /` | Opt-in flip |
| `User-Agent: meta-externalagent Disallow: /` | present | replaced with `Allow: /` | Opt-in flip |
| `User-Agent: Applebot-Extended Disallow: /` | present | replaced with `Allow: /` | Opt-in flip |
| `User-Agent: Amazonbot Disallow: /` | present | replaced with `Allow: /` | Opt-in flip |
| `User-Agent: CloudflareBrowserRenderingCrawler Disallow: /` | present | replaced with `Allow: /` | Opt-in flip |
| `User-Agent: PerplexityBot` | absent | added with `Allow: /` | Inventory gap noted in STATE.md — Perplexity actively crawls + cites; we want to be cited |
| `User-Agent: anthropic-ai` | absent | added with `Allow: /` | Inventory gap noted in STATE.md — legacy UA, still seen in some Anthropic infrastructure; explicit allow for completeness |
| `Sitemap:` directive | absent | added | Standard signal pointing crawlers at `/sitemap-index.xml`; no functional change, just makes discoverability explicit |
| Comment block | absent | added | Documents the posture for future maintainers + AI/security researchers reading the file |

## Cloudflare coordination — the override mechanism

**The hard part of this draft.** The current robots.txt is *Cloudflare-
managed*, which means our `public/robots.txt` in the site repo is
overridden at the edge unless we explicitly disable the platform-
default. Three options, in increasing complexity:

### Option A — Cloudflare dashboard: "Manage Robots.txt"

Cloudflare's Bot Management dashboard has a "Manage Robots.txt" toggle
under **Security → Bots**. Disable the platform-managed robots.txt
there; Cloudflare then serves whatever the origin (our site repo)
provides.

- **Pros:** Single dashboard click; reversible; doesn't require
  worker code or page-rule budget.
- **Cons:** Requires dashboard access (manual step, not in the site
  repo's deploy pipeline); easy to revert accidentally during
  unrelated security-config changes.

**Recommend Option A as the MVP path** — fastest, reversible, and the
override exists explicitly in the dashboard so future maintainers
can find it.

### Option B — Cloudflare Worker

Deploy a tiny worker that intercepts `/robots.txt` requests and
serves the file from R2/origin, bypassing the platform default:

```javascript
// workers/robots-override.js
export default {
  async fetch(request) {
    const url = new URL(request.url);
    if (url.pathname === '/robots.txt') {
      return fetch(`${origin}/robots.txt`, { cf: { cacheTtl: 300 } });
    }
    return fetch(request);
  }
}
```

- **Pros:** Override lives in code; survives dashboard config
  changes; can be versioned in the site repo.
- **Cons:** Workers cost money (free tier covers light traffic but
  adds a moving piece); adds latency (~5-10ms per request).

**Defer Option B unless Option A proves unstable.**

### Option C — Page Rule that bypasses Cloudflare's content-signal

Page Rules (legacy) or Configuration Rules (new) can be set to
"Bypass" Cloudflare's automatic content-signal injection for
specific URL patterns. Requires Cloudflare dashboard access
(same as Option A) but lives in a different config surface.

- **Pros:** Targeted; doesn't disable the platform feature site-wide.
- **Cons:** Harder to discover than the dashboard toggle; depends on
  Cloudflare's plan (some Page Rule features are paid-tier-only).

**Defer Option C unless Option A is unavailable.**

### `content-signal` header — separate concern

Even after the platform-default robots.txt is bypassed (Option A),
Cloudflare may STILL inject the `content-signal` header at the edge
based on its own ai-bot management config. The header lives in
**Security → Bot Management → AI Bot Management** in the dashboard;
the toggle is "Block AI training crawlers." Set it to **OFF** to
opt in.

The applier MUST verify both the robots.txt body AND the
`content-signal` header have flipped after the dashboard changes —
they're controlled by different Cloudflare subsystems.

## Implementation notes (for the site repo)

- **File location.** Drop the body above into `public/robots.txt`.
  Astro/Starlight serves `public/` verbatim.
- **Cloudflare dashboard step (Option A).** Document the dashboard
  steps in the site repo's `DEPLOY.md` (or equivalent ops doc):
  ```
  1. Cloudflare Dashboard → Security → Bots
  2. Disable "Manage Robots.txt" (toggle off)
  3. Cloudflare Dashboard → Security → Bot Management → AI Bot Management
  4. Disable "Block AI training crawlers" (toggle off)
  5. Wait 5-10 minutes for edge cache invalidation
  6. Verify: `curl -I https://alint.org/robots.txt` returns the
     content from public/robots.txt (the site-repo file, not the
     Cloudflare-managed default)
  7. Verify: `curl -I https://alint.org/` does NOT include
     `content-signal: search=yes,ai-train=no` in the response
     headers; either the header is absent, or it reads
     `search=yes,ai-train=yes`
  ```
- **Coordination with `well-known-ai-txt.md`.** Both files express
  the AI-use posture; ship them in the same site-repo PR + the
  same Cloudflare-config window. Conflict resolution between the
  two files is undefined per spec.
- **No alint-repo equivalent.** This is purely site-side; the alint
  repo doesn't ship its own robots.txt.

## Open questions

1. **Cloudflare-managed override mechanism — which option?**
   Option A (dashboard toggle) is recommended for v0.10 ship.
   Risk: the dashboard step lives outside the site-repo deploy
   pipeline; if a future maintainer accidentally re-enables the
   platform default during unrelated config work, the
   `public/robots.txt` is silently overridden. **Mitigation:**
   document the dashboard steps in `DEPLOY.md` + add a
   site-deploy CI check that fetches `https://alint.org/robots.txt`
   post-deploy and asserts a known marker string from our own
   body (e.g., `# alint.org robots.txt`) is present.
2. **Cloudflare's ai-bot block is at the *bot-management* layer,
   not robots.txt.** Even after publishing an opt-in robots.txt,
   Cloudflare's bot-management may STILL block ClaudeBot/GPTBot
   etc. at the edge before the request reaches our origin. The
   "Block AI training crawlers" toggle in the dashboard is the
   load-bearing control. **Verify post-deploy** that
   ClaudeBot/GPTBot User-Agent fetches actually succeed (simulate
   with curl `-A`).
3. **Scope of opt-in.** Current draft opts in for the entire site
   (`Allow: /`). If we later add a `/private/` or
   `/contributors-only/` route, revisit. For v0.10, the broad
   opt-in is correct.
4. **Sitemap directive placement.** RFC convention is one
   `Sitemap:` line in the wildcard `User-Agent: *` block;
   per-UA blocks don't repeat it. Current draft follows
   convention.
5. **PerplexityBot UA spelling.** Some sources use `Perplexity-Bot`
   or `Perplexity-User`. Verify the canonical UA from
   https://docs.perplexity.ai/guides/bots before publish.
   Currently using `PerplexityBot` per launch-prep doc and recent
   Perplexity blog posts.
6. **Unlisted future bots.** New AI crawlers will appear; we don't
   want to refresh robots.txt every quarter. **Mitigation:** the
   `User-Agent: *` `Allow: /` at the top of the file is the
   broadest opt-in; per-UA `Allow: /` blocks below are explicit
   reaffirmations. New bots inherit the wildcard allow without a
   refresh. Per-UA blocks exist mainly to override any
   platform-defaults that ship with explicit Disallows.
7. **Reciprocal with `<meta name="robots">`.** Some pages may
   want `noindex` (e.g., 404 page, draft routes). robots.txt
   doesn't change the per-page meta; site authors still control
   that per-page. No interaction with the launch-decision
   opt-in.

## Pre-publish checklist

- [ ] `public/robots.txt` exists in the site repo with the
      proposed body.
- [ ] Cloudflare dashboard "Manage Robots.txt" toggle is OFF
      (verified by maintainer with dashboard access).
- [ ] Cloudflare dashboard "Block AI training crawlers" toggle is
      OFF (separate setting, verified independently).
- [ ] `curl https://alint.org/robots.txt` returns the
      `# alint.org robots.txt` marker comment (proves the
      site-repo file is served, not the Cloudflare default).
- [ ] `curl -I https://alint.org/` shows no
      `content-signal: search=yes,ai-train=no` header (or shows
      `…ai-train=yes`).
- [ ] `curl -A "ClaudeBot/1.0" https://alint.org/` returns 200
      (not 403/blocked).
- [ ] `curl -A "GPTBot/1.0" https://alint.org/` returns 200.
- [ ] `well-known-ai-txt.md` is in `ready` state for coordinated
      publish (matching opt-in posture).
- [ ] Post-deploy CI check added that asserts the robots.txt
      marker comment is present (defends against Cloudflare-
      managed reversion).
- [ ] STATE.md row for `https://alint.org/robots.txt` flipped
      from `stale` to `live` with date + commit SHA + Cloudflare
      config-change reference.
- [ ] `reference_alint-marketing-tracking.md` user-memory updated
      noting the opt-in implementation date (separate from the
      decision date).

## Estimated diff size

In the site repo:
- 1 new file at `public/robots.txt`: ~50 lines (body above).
- 1 update to `DEPLOY.md` or equivalent: ~20 lines (Cloudflare
  dashboard steps).
- 1 CI check addition (`.github/workflows/post-deploy-checks.yml`
  or similar): ~10 lines.

Outside the repo:
- Cloudflare dashboard: 2 toggles (manual, ~2 minutes).

Total: ~80 lines (site repo) + 2 dashboard toggles.

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `well-known-ai-txt.md` | The two files MUST express the same opt-in posture. Conflict resolution is undefined per spec. Ship coordinated. |
| `llms-txt.md`, `llms-full-txt.md` | Same posture (opt-in for AI use); different mechanism (LLM context discovery vs crawler control). The trio constitutes the AI-discovery surface bundle; ship together. |
| `well-known-security-txt.md` | Bundled into the same `/.well-known/`-touchpoint site PR is convenient (different concern, same edit window). |
| `releases-atom.md`, `api-endpoints.md` | The Sitemap directive points crawlers at `/sitemap-index.xml`; ensure that file is up-to-date with the new `/api/*.json` endpoints (if those should be discoverable; default for JSON endpoints is to NOT be in the sitemap, since they're not human-readable pages). |
