---
destination: alint.org/llms-full.txt (site-repo build artifact at the public root)
status: drafting
blocks_on: build-time inlining script (proposed below); coordinated publish with `llms-txt.md`; tier decision (recommend: ship Tier 1 at v0.10, Tier 2 deferred)
last_touched: 2026-05-06
---

# alint.org/llms-full.txt — content brief for the site repo

## Why

`/llms.txt` (the small index file — see `llms-txt.md`) lets browse-
capable LLMs find canonical alint URLs and fetch them on demand. But
many LLM clients run *without* browse tools — they get a single text
blob as context and have to answer from that. For those clients, the
llmstxt.org spec proposes a companion file `/llms-full.txt` that
contains the **same structure as llms.txt but with all linked content
inlined** into one large markdown blob.

The pitch is "an LLM with no browse tool can still answer
'how do I bootstrap alint?' or 'what's the structured-query syntax?'
from a single download."

For alint, the universe of inlineable content is large:

- 60 per-rule docs (one markdown file each, ~1-2KB)
- 19 bundled-ruleset docs (one markdown file each, ~2-3KB)
- 1 cookbook page (~10-15KB)
- 25 case-study READMEs (~3-5KB each)
- Getting Started + Quickstart + Configuration reference + CLI
  reference (~5-10KB each)
- Compare page (~25KB)
- Migrating-from-{repolinter,ls-lint,custom-bash} pages (~30-50KB each)

Inlined naively, that's ~500KB — usable by Claude (which has a
1M-token context) but borderline-to-unusable for smaller-context
clients. **Tiering is the answer:** ship two files, let the client
pick.

## Tiering proposal

| Tier | File | Size | Contents | Audience |
|---|---|---|---|---|
| **Tier 1** | `/llms-full.txt` | ~50KB | Cookbook + getting-started + configuration ref + CLI ref + compare-page TL;DR + migrating-from-repolinter TL;DR | Default. Fits comfortably in a 32k-token context with room for the user's question + assistant scratch space. Covers ~80% of "how do I use alint?" questions. |
| **Tier 2** | `/llms-everything.txt` | ~500KB | Everything in Tier 1 + all 60 per-rule docs + all 19 bundled-ruleset docs + all 25 case-study READMEs + full migrating-from guides | Power-user / large-context. For Claude/GPT-class assistants asked deeply technical questions ("write me a structured-query rule that…"). |

The llmstxt.org spec uses `/llms-full.txt` as the canonical second
URL; `/llms-everything.txt` is alint-specific. Both files include a
header note pointing at the other for size-tier discoverability.

**MVP recommendation:** ship Tier 1 only at v0.10. Defer Tier 2 until
demand surfaces (we have no evidence yet that anyone needs it; Tier 1
is the actual demand-validated artefact).

## Proposed `/llms-full.txt` body — Tier 1

```markdown
# alint — full reference (Tier 1)

> Fast, language-agnostic linter for repository structure, files, and
> content. This file contains the most-used alint docs inlined into
> one downloadable markdown blob, for LLM clients without browse
> tooling. For the URL index, see /llms.txt. For the everything-
> inlined version (~500KB, includes all 60 rule docs and 25 case
> studies), see /llms-everything.txt (if shipped — Tier 2).

## Table of contents

1. Quickstart (installation + first config)
2. Configuration reference
3. CLI reference
4. Cookbook (20+ patterns)
5. Compare with other repo-linters (TL;DR)
6. Migrating from Repolinter (TL;DR)
7. Where to find more (links to per-rule + per-ruleset + case-study URLs)

---

## 1. Quickstart

[INLINED FROM: docs/site/getting-started/installation.md]

[INLINED FROM: docs/site/getting-started/quickstart.md]

---

## 2. Configuration reference

[INLINED FROM: docs/site/configuration/index.md AND children]
- top-level fields (version, extends, facts, rules, output, ignore)
- rule fields (id, kind, paths, level, message, when, scope_filter, fix, …)
- the `extends:` URL scheme (alint://bundled/<name>@v1, https:// with SRI)
- output format options

---

## 3. CLI reference

[INLINED FROM: docs/site/cli/index.md AND children]
- `alint check`
- `alint fix`
- `alint validate-config`
- `alint --help` flag table
- exit codes

---

## 4. Cookbook

[INLINED FROM: docs/site/cookbook/index.md verbatim]

---

## 5. Compare with other repo-linters (TL;DR)

[INLINED FROM: docs/marketing/drafts/alint-org-compare.md — first ~80 lines, the TL;DR routing table + feature matrix; full per-tool deep dives elided with a "see /compare/ for the full version" note]

---

## 6. Migrating from Repolinter (TL;DR)

[INLINED FROM: docs/marketing/drafts/migrate-from-repolinter.md — the "TL;DR — the 60-second migration" section + the rule-mapping table; full per-rule walkthrough elided with a "see /migrating-from/repolinter/ for the full version" note]

---

## 7. Where to find more

For depth that didn't fit in this Tier 1 blob:

- **Per-rule docs** (60 rule kinds): https://alint.org/docs/rules/
- **Bundled-ruleset docs** (19 rulesets): https://alint.org/docs/bundled-rulesets/
- **Case studies** (25 production OSS repos): https://alint.org/examples/
- **Full migration guides**: https://alint.org/migrating-from/{repolinter,ls-lint,custom-bash-scripts}/
- **Comparison page** (full version): https://alint.org/compare/
- **Tier 2 inlined version**: /llms-everything.txt (if shipped)
- **URL index for browse-capable LLMs**: /llms.txt

This file is published as part of the alint.org build pipeline and
regenerated on every site deploy from the source markdown in
asamarts/alint's `docs-bundle` branch.
```

## Build-time generation strategy

Two approaches, in increasing order of complexity:

### Approach A — single shell script (recommended for MVP)

A `scripts/build-llms-full.sh` in the alint.org site repo that:

1. Reads a manifest file (`scripts/llms-full-tier1.manifest`)
   listing the source markdown files in inline-order.
2. For each manifest entry, emits a `## <heading>` separator + the
   file's body (stripped of frontmatter).
3. Writes the result to `public/llms-full.txt`.
4. Runs as a build step before `astro build`.

```bash
#!/usr/bin/env bash
# scripts/build-llms-full.sh
set -euo pipefail

OUT="public/llms-full.txt"
MANIFEST="scripts/llms-full-tier1.manifest"

cat scripts/llms-full-header.md > "$OUT"

while IFS= read -r line; do
  [[ -z "$line" || "$line" =~ ^# ]] && continue   # skip blanks and comments
  printf '\n\n---\n\n' >> "$OUT"
  # strip Astro/Starlight frontmatter (between leading --- markers)
  awk 'BEGIN{f=0} /^---$/{f++; next} f>=2{print}' "$line" >> "$OUT"
done < "$MANIFEST"

# Sanity check: bail if output exceeds Tier 1 budget
SIZE=$(wc -c < "$OUT")
if (( SIZE > 60000 )); then
  echo "ERROR: llms-full.txt exceeded 60KB Tier 1 budget ($SIZE bytes)" >&2
  exit 1
fi
```

The manifest:

```
# scripts/llms-full-tier1.manifest
# Order matters; this is the inlined-blob TOC order.
src/content/docs/getting-started/installation.md
src/content/docs/getting-started/quickstart.md
src/content/docs/configuration/index.md
src/content/docs/cli/index.md
src/content/docs/cookbook/index.md
# Compare and migrating-from TLDRs come from drafts in the alint repo, synced
# at docs-bundle time. They live at:
src/content/docs/compare/index.md
src/content/docs/migrating-from/repolinter.md
```

### Approach B — Astro/Starlight content collection

Define a content collection (`src/content/llms-full.config.ts`) and
a custom rendering route. More integrated but heavier engineering
investment for v0.10 ship target. Defer to v0.11+.

**Recommend Approach A.** Ship in 1-2 days; deferable to Approach B
once we have telemetry on whether `/llms-full.txt` actually gets
fetched.

## Tier 2 (`/llms-everything.txt`) — outline only

If/when we ship Tier 2, the manifest expands to include:

- Every `src/content/docs/rules/<family>/<kind>.md` (60 files)
- Every `src/content/docs/bundled-rulesets/<name>.md` (19 files)
- Every `examples/*/README.md` (25 files, sourced from
  `examples/` in the alint repo, synced via docs-bundle)
- Full body of compare page (not just TL;DR)
- Full bodies of all three migrating-from guides

Same generation script, different manifest. **Don't ship until
demand-validated** — Tier 2 doubles the complexity and cost (CDN
egress, slower builds, larger client downloads) for a use case we
haven't seen evidence of.

## Open questions

1. **Tier 1 size budget — 50KB enough?** Cookbook alone is ~10-15KB
   already; getting-started is ~5KB; compare TL;DR is ~10KB;
   migrating-from-repolinter TL;DR is ~10KB. Conservative estimate:
   ~45-55KB. The 60KB hard ceiling in the build script gives some
   headroom but might bite once doc content grows. Recommend:
   monitor budget at each docs-bundle release; if it ever hits
   60KB, either raise the ceiling to 80KB or trim cookbook patterns
   from Tier 1.
2. **Token-count vs byte-count.** 50KB markdown is ~12-15k tokens
   for most tokenizers — fits comfortably in a 32k-token context
   alongside a user prompt + assistant response. If the build
   script can call `tiktoken` or similar at build time, swap the
   byte-budget for a token-budget. (Not blocking; bytes are a fine
   approximation.)
3. **Inline-or-link decision per source doc.** Some docs (the
   per-rule pages) are heavily cross-referential and lose meaning
   when separated from their rule-family context. Tier 1 explicitly
   skips these; Tier 2 includes them but flags the cross-refs as
   "this rule belongs to family X — see also rule-Y, rule-Z".
   Tier-2 pre-publish review item.
4. **Sync with `llms.txt`.** The two files reference each other.
   When `llms.txt` adds a new section, `llms-full.txt`'s TOC + body
   needs to track it. Consider: a CI check that `llms.txt` H2
   headers are a subset of `llms-full.txt` H1/H2 headers.
5. **Cache invalidation.** A 50KB-500KB static file at the root
   means CDN caching matters. Cloudflare Pages defaults are fine
   (5-minute edge cache); just verify after first publish that
   `Cache-Control` doesn't get set to `no-cache` accidentally.

## Pre-publish checklist

- [ ] `scripts/build-llms-full.sh` exists in the site repo and runs
      cleanly as part of `astro build`.
- [ ] `scripts/llms-full-tier1.manifest` lists all source files in
      the right order.
- [ ] `public/llms-full.txt` is generated at build time (verify
      file exists in the deploy artifact).
- [ ] Output is under the 60KB Tier 1 budget.
- [ ] Inlined content has frontmatter stripped (no `---\ntitle:` at
      the top of any section).
- [ ] Cross-references in inlined content (`[See here](/docs/foo/)`)
      either resolve absolutely (preferred — rewrite during build)
      or include enough context to be parseable on their own.
- [ ] `/llms.txt` includes a sibling-pointer header note about
      `/llms-full.txt`.
- [ ] Manual smoke test: download `https://alint.org/llms-full.txt`,
      paste into a fresh Claude/GPT session, ask "how do I write a
      structured-query rule?" — verify the assistant can answer
      without browse tools.
- [ ] STATE.md row for `alint.org/llms-full.txt` flipped from
      `missing` to `live` with date + commit SHA.

## Estimated diff size on the site repo

- 1 new file at `scripts/build-llms-full.sh`: ~25 lines.
- 1 new file at `scripts/llms-full-tier1.manifest`: ~10 lines.
- 1 new file at `scripts/llms-full-header.md`: ~25 lines (the
  static H1 + summary + TOC at the top of the output).
- 1 new build hook in `package.json` or `astro.config.ts` to run
  the script before `astro build`: ~3 lines.
- 1 generated artifact at `public/llms-full.txt`: gitignored,
  built fresh each deploy.

Total: ~65 lines authored + 1 generated artifact.

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `llms-txt.md` | The two files reference each other and share the same conceptual purpose (LLM discovery). Ship coordinated. |
| `well-known-ai-txt.md` | Same posture (opt-in for AI use); different mechanism (training opt-in vs runtime context). |
| All P3.1 drafts | The inlined content sources from those drafts' destinations; they need to be live before this works. |
