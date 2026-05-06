---
destination: alint.org site repo (Astro/Starlight build config + integration)
status: drafting
blocks_on: site repo applier needs to confirm the current `@astrojs/sitemap` integration version + verify Starlight isn't shadowing the `lastmod` knob
last_touched: 2026-05-06
---

# alint.org sitemap `<lastmod>` — content brief for the site repo

## Why

The alint.org inventory (subagent fetch 2026-05-06) confirmed:

- `https://alint.org/sitemap-index.xml` exists and references
  `/sitemap-0.xml` (122 URLs).
- **No per-URL `<lastmod>` tags.** Every entry is just `<loc>`.
- `https://alint.org/sitemap.xml` 404s; only the `-index.xml` form
  works.

`<lastmod>` is one of the three signals
([per sitemap.org](https://www.sitemaps.org/protocol.html#xmlTagDefinitions))
that crawlers use to prioritise re-fetch. Google
[publicly confirmed in 2023](https://developers.google.com/search/blog/2023/06/sitemaps-lastmod-ping)
that `lastmod` is the only sitemap signal it actually trusts —
`changefreq` and `priority` are ignored.

Without `lastmod`:

- Google re-crawls the rule pages on its own schedule (typically
  weeks-to-months for a low-traffic domain). When we ship a v0.9.x
  release that updates 60 rule docs in one push, all 60 sit stale in
  the index until Google's next sweep.
- Our docs-bundle pipeline (push to `alint:main` → docs-bundle branch
  → Cloudflare rebuild in 2-3 min) is engineered for fast turnaround
  but the SEO signal of "this page is fresh" never reaches the
  crawler.

This brief specifies how to wire `lastmod` into the auto-generated
sitemap, with the docs-bundle commit timestamp as the source of
truth.

## Background — current sitemap stack

Starlight uses [`@astrojs/sitemap`](https://docs.astro.build/en/guides/integrations-guide/sitemap/)
under the hood. The integration is preconfigured by the Starlight
preset and emits `/sitemap-index.xml` + numbered `/sitemap-N.xml`
shards. By default it does NOT emit `lastmod` because Astro doesn't
know the per-page modification time — every page is generated at
build time, so the filesystem mtime is "build time" for everything.

The fix is to pass a `serialize` (or `lastmod`) callback to
`@astrojs/sitemap` that resolves a meaningful timestamp per page.

## Proposed config — `astro.config.mjs`

The site repo's `astro.config.mjs` currently configures Starlight +
sitemap roughly like:

```js
// current (sketch — site repo applier should verify against the actual file)
import sitemap from '@astrojs/sitemap';

export default defineConfig({
  site: 'https://alint.org',
  integrations: [
    starlight({ /* ... */ }),
    sitemap(),
  ],
});
```

Replace the `sitemap()` call with:

```js
import sitemap from '@astrojs/sitemap';
import { execSync } from 'node:child_process';
import { statSync } from 'node:fs';
import { join } from 'node:path';

// Build-time: resolve the docs-bundle's HEAD commit date once.
// The sync step (npm run sync) clones docs-bundle into
// src/content/docs/docs/ — read its commit date there.
function bundleCommitDate() {
  try {
    const bundlePath = 'src/content/docs/docs/.bundle-commit-date';
    // Preferred: a tiny file written by the sync step.
    return new Date(readFileSync(bundlePath, 'utf8').trim());
  } catch {
    // Fallback: ask git directly inside the synced bundle (the sync
    // does a shallow clone so the .git dir IS present).
    try {
      const out = execSync(
        'git -C src/content/docs/docs log -1 --format=%cI',
      ).toString().trim();
      return new Date(out);
    } catch {
      // Last fallback: build time. Better than no lastmod at all.
      return new Date();
    }
  }
}

const BUNDLE_DATE = bundleCommitDate();
const SITE_REPO_DATE = new Date(
  execSync('git log -1 --format=%cI').toString().trim(),
);

export default defineConfig({
  site: 'https://alint.org',
  integrations: [
    starlight({ /* ... */ }),
    sitemap({
      serialize(item) {
        // Bundle-sourced pages: lastmod = docs-bundle HEAD commit date.
        // Site-shell pages (landing, /compare/, /examples/ index, blog):
        //   lastmod = site repo HEAD commit date.
        const isBundlePage =
          item.url.startsWith('https://alint.org/docs/') &&
          !item.url.startsWith('https://alint.org/docs/changelog/');
          // changelog is bundle-sourced too; adjust the predicate to
          // match the actual sync output. The site-repo applier
          // should sanity-check by listing src/content/docs/docs/.

        item.lastmod = (isBundlePage ? BUNDLE_DATE : SITE_REPO_DATE)
          .toISOString();
        return item;
      },
    }),
  ],
});
```

The shape of the `serialize` callback is documented in
[`@astrojs/sitemap` v3 docs](https://docs.astro.build/en/guides/integrations-guide/sitemap/#serialize)
— it receives a `SitemapItem` (`{ url, lastmod?, changefreq?, priority?, links? }`)
and returns a (possibly modified) item or `undefined` to drop it.

## Source-of-truth rules

| Page family | `lastmod` source | Why |
|---|---|---|
| `/docs/**` (everything synced from the docs-bundle: getting-started, concepts, rules, bundled-rulesets, cookbook, integrations, CLI, changelog) | docs-bundle HEAD commit ISO timestamp | These pages regenerate every time `alint:main` updates. The bundle's commit date is the most precise signal — bumps when ANY docs-source file changes (rule YAML, ruleset YAML, CLI `--help`, hand-written `.md` under `docs/site/`). |
| `/`, `/compare/`, `/examples/` (gallery index), `/blog/**`, `/migrating-from/**`, keyword landing pages (`/repolinter-alternative` etc.) | site repo HEAD commit ISO timestamp | These are written directly in the alint.org site repo. Their mtime is the site repo's commit when each was last touched. |
| `/examples/<owner>-<repo>/` (per-case-study sub-pages, ingested from `examples/*/README.md` per the gallery draft) | docs-bundle HEAD commit ISO timestamp | Bundle-sourced; same logic as `/docs/**`. |

If we want **per-page precision** (not just "the whole bundle bumped"),
the docs-bundle pipeline can emit a sidecar `lastmod.json` mapping
`{slug: ISO-timestamp}` derived from `git log -1 --format=%cI -- <file>`
in the alint repo. That's a v2 enhancement — the bundle-wide
`lastmod` proposed above is a strict improvement over today's "no
lastmod at all" and ships in a single config change.

## Implementation notes (for the site repo applier)

1. **Confirm `@astrojs/sitemap` version.** v3+ supports the
   `serialize` callback shape above. v2 had a different signature.
   `npm ls @astrojs/sitemap` in the site repo will say.

2. **Confirm the sync step's git-history retention.** The current
   pipeline (per `reference_alint-org-docs-pipeline`) does a
   `git clone --depth=1 --branch=docs-bundle …` into
   `src/content/docs/docs/`. Depth=1 means `git log -1 --format=%cI`
   inside that dir works (returns the HEAD commit date) — but
   per-file `git log -- <file>` would return the same date for every
   file. If we want per-file precision, the sync step needs
   `--depth=N` or no depth limit. For the bundle-wide approach
   proposed above, depth=1 is fine.

3. **Optional: write `.bundle-commit-date`.** Tiny enhancement to the
   docs-bundle workflow at `~/projects/alint/.github/workflows/docs-bundle.yml`
   — write `git log -1 --format=%cI` into the bundle as
   `.bundle-commit-date`. Saves the site repo from invoking
   `git log` at build time. The proposed config above tries the
   sidecar file first, falls back to `git log`, falls back to
   build-time. Either works.

4. **Verify Starlight isn't shadowing the integration.** Starlight's
   own preset doesn't currently override `@astrojs/sitemap`'s
   `serialize` — but if the Starlight version on the site repo
   bumps and starts overriding it, the `serialize` callback above
   will silently no-op. After deploying, `curl
   https://alint.org/sitemap-0.xml | grep -c lastmod` should match
   `grep -c '<url>'`. CI check ideal but manual is fine for v1.

5. **Cache-control on the sitemap.** Default Cloudflare Pages
   cache is OK; the sitemap is small. If we later host on a CDN
   with longer TTLs, set the sitemap to a short TTL (e.g. 1h) so
   `lastmod` changes propagate fast.

## Open questions before publish

1. **`changefreq` and `priority`.** Worth setting them? Google
   explicitly ignores both ([2023 blog post linked above](https://developers.google.com/search/blog/2023/06/sitemaps-lastmod-ping));
   Bing still reads `priority`. Recommend: skip both. They add noise
   and any non-trivial values will eventually drift.
2. **Per-file precision (v2).** When the rule catalogue grows
   (current 60 → maybe 80 in v0.10), bundle-wide `lastmod` will
   bump every release even if only 3 rule pages changed. At that
   point the per-file sidecar JSON becomes worth shipping. Out of
   scope for v1.
3. **Sitemap split.** `@astrojs/sitemap` shards at 45,000 URLs by
   default. We're at 122. No action needed for years.

## Pre-publish checklist

- [ ] Site repo applier identifies the actual `astro.config.mjs`
      sitemap config + verifies `@astrojs/sitemap` v3+.
- [ ] Sync step's clone strategy verified (depth=1 is fine for the
      bundle-wide approach).
- [ ] Optional: docs-bundle workflow writes `.bundle-commit-date`
      sidecar.
- [ ] After deploy, `curl https://alint.org/sitemap-0.xml | grep -c
      lastmod` returns 122 (or whatever the current URL count is) —
      every `<url>` has a `<lastmod>`.
- [ ] After deploy, ISO-8601 dates parse cleanly: spot-check 3 rule
      pages, the landing, and a `/docs/cookbook/` page have plausible
      `<lastmod>` values.
- [ ] After deploy, resubmit the sitemap to Google Search Console +
      Bing Webmaster Tools (sitemap dashboard → "Resubmit").
- [ ] STATE.md row for `https://alint.org/sitemap-index.xml` flipped
      from `stale` to `live (just refreshed)` with date + commit SHA.

## Estimated diff size on the site repo

- `astro.config.mjs`: ~30 lines added (the `serialize` callback +
  the `bundleCommitDate` helper + the two date constants).
- (Optional) `~/projects/alint/.github/workflows/docs-bundle.yml`:
  ~3 lines added to write `.bundle-commit-date` sidecar. This is
  in the alint repo, not the site repo.

Total: ~30 lines on the site repo + ~3 lines on the alint repo
(optional).

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `meta-descriptions.md` | Same file (`astro.config.mjs`) may need touching for both — apply together as one site-repo PR. |
| `schema-org-jsonld.md` | Same site-shell layer. Same PR if cheap. |
| `keyword-landing-pages.md` | New pages need a `lastmod` source — site repo HEAD commit covers them automatically with the rules above. |
