---
title: "Concepts redesign: Phase 6 execution plan (relocations, renames, grouped sidebar, redirects)"
description: The concrete, file-level plan for the final phase of the Concepts-section redesign. Batches every URL change (renames, subdir grouping, retirements, relocations) into one atomic cross-repo cut, plus the new bundled-rulesets page.
---

# Phase 6 execution plan

The final phase of the [Concepts redesign](./concepts-section-redesign.md). Phases 1-5 rewrote the content and shipped the animated diagrams while deliberately deferring every filename and URL change to here, so the whole re-org lands as one atomic cut with a single redirect set. This doc is the file-level plan; nothing is executed until it is approved.

It follows the redesign doc's information architecture (section 5.1), the relocations table (section 5.2), the decided sidebar mechanism (section 5.4), and the migration/redirect section (section 9). Phase 6 is deliberately broader than the redesign doc's own section-8 phase bullet: it also absorbs the two renames (deferred from Phases 3-4) and the new `bundled-rulesets` page (a later decision, section 11 item 8), because both change or add URLs and belong in the single atomic cut.

It is a **cross-repo** change: page content lives in `alint/docs/site/` (every section, not just concepts, is synced verbatim to `alint.org/src/content/docs/docs/`), so all page moves and folds happen in the `alint` repo; only the grouped sidebar (`alint.org` `astro.config.mjs`) and the redirects (`alint.org` `public/_redirects`) are `alint.org` edits.

## 1. Goal and shape

Turn today's 20 flat concept pages into 16 grouped concept pages, with 5 feature/command pages relocated or folded away, and every old URL redirected. After Phase 6 the redesign is complete.

- **Stay in Concepts (15): 14 existing pages move into 6 group subdirectories, plus `index.md` at the section root.**
- **Renamed (2, among the movers):** `how-it-works` -> `how-alint-works`, `walker-and-gitignore` -> `the-walker-and-git`.
- **Retired from Concepts (2), content folded + redirected:** `content-from` -> fixing, `drop-ins` -> config-layering.
- **Relocated out of Concepts (3):** `templates` -> Configuration, `suggest` -> Cookbook, `variable-interpolation` -> Configuration (its full reference; the interpolation-timing hook already lives in config-layering).
- **New (1):** `bundled-rulesets` (decision section 11 item 8).

Accounting: 15 stay + 2 retire + 3 relocate = 20 current pages; the section ends at 16 (15 + the new `bundled-rulesets`).

## 2. Group subdirectories and the move map

Pages move into `docs/site/concepts/<group>/`, so the group slug becomes a URL segment (for example `/docs/concepts/targeting/scoping/`). The section landing `index.md` stays at the section root. Per-group `sidebar.order` restarts at 1 (today's duplicate/gap orders are retired by the move).

| Group dir (label) | Page | Old path | New path |
|---|---|---|---|
| (root) | Concepts hub | `docs/site/concepts/index.md` | `docs/site/concepts/index.md` (unchanged) |
| `start-here` (Start here) | how-alint-works | `docs/site/concepts/how-it-works.md` | `docs/site/concepts/start-here/how-alint-works.md` |
| `start-here` | the-config-model | `docs/site/concepts/the-config-model.md` | `docs/site/concepts/start-here/the-config-model.md` |
| `start-here` | kinds-families-categories | `docs/site/concepts/kinds-families-categories.md` | `docs/site/concepts/start-here/kinds-families-categories.md` |
| `start-here` | severity-and-exit-codes | `docs/site/concepts/severity-and-exit-codes.md` | `docs/site/concepts/start-here/severity-and-exit-codes.md` |
| `targeting` (How rules target files) | the-walker-and-git | `docs/site/concepts/walker-and-gitignore.md` | `docs/site/concepts/targeting/the-walker-and-git.md` |
| `targeting` | scoping | `docs/site/concepts/scoping.md` | `docs/site/concepts/targeting/scoping.md` |
| `targeting` | changed-mode | `docs/site/concepts/changed-mode.md` | `docs/site/concepts/targeting/changed-mode.md` |
| `composition` (Composition and trust) | composition-and-trust | `docs/site/concepts/composition-and-trust.md` | `docs/site/concepts/composition/composition-and-trust.md` |
| `composition` | bundled-rulesets (NEW) | -- | `docs/site/concepts/composition/bundled-rulesets.md` |
| `composition` | config-layering | `docs/site/concepts/config-layering.md` | `docs/site/concepts/composition/config-layering.md` |
| `multi-file` (Beyond single files) | cross-file-rules | `docs/site/concepts/cross-file-rules.md` | `docs/site/concepts/multi-file/cross-file-rules.md` |
| `multi-file` | structured-queries | `docs/site/concepts/structured-queries.md` | `docs/site/concepts/multi-file/structured-queries.md` |
| `adoption` (Adoption and fixing) | fixing | `docs/site/concepts/fixing.md` | `docs/site/concepts/adoption/fixing.md` |
| `adoption` | baseline | `docs/site/concepts/baseline.md` | `docs/site/concepts/adoption/baseline.md` |
| `agents` (Working with agents) | the-agent-surface | `docs/site/concepts/the-agent-surface.md` | `docs/site/concepts/agents/the-agent-surface.md` |

Group order in the sidebar: Start here, How rules target files, Composition and trust, Beyond single files, Adoption and fixing, Working with agents (progressive: mental model -> targeting -> composition -> multi-file -> adoption -> agents).

## 3. Retirements (fold + redirect)

Two pages are deleted after their content folds into a surviving concept page; the old URL is redirected. The fold targets already carry the concept, so the work is migrating the remaining unique material, then deleting the source and repointing links (section 6).

- **`content-from` -> `adoption/fixing`.** `fixing.md` already has a `## content_from` section. Migrate the parts it lacks (the "when to reach for it" LICENSE/SPDX rationale and the monorepo-templates note). The exhaustive per-op detail is NOT re-homed on the concept page: it already lives in the generated rule reference (`docs/rules.md`, the fix-op shapes and per-rule "Fix:" notes) which `fixing.md` links. Delete `docs/site/concepts/content-from.md`; redirect `/docs/concepts/content-from/` -> `/docs/concepts/adoption/fixing/`.
- **`drop-ins` -> `composition/config-layering`.** `config-layering.md` already has a `## Drop-ins: .alint.d/` section covering the mechanic. Migrate anything unique from drop-ins (Layout / Trust posture / What-gets-merged-where / Limits). Delete `docs/site/concepts/drop-ins.md`; redirect `/docs/concepts/drop-ins/` -> `/docs/concepts/composition/config-layering/`.

## 4. Relocations (out of Concepts)

These pages move out of Concepts into an existing sibling section (still authored in the `alint` repo under `docs/site/`, synced to `alint.org`).

- **`templates` -> Configuration.** `git mv docs/site/concepts/templates.md docs/site/configuration/templates.md` (a config construct, not a concept). Redirect `/docs/concepts/templates/` -> `/docs/configuration/templates/`.
- **`suggest` -> Cookbook.** `git mv docs/site/concepts/suggest.md docs/site/cookbook/suggest.md` (a command workflow). Redirect `/docs/concepts/suggest/` -> `/docs/cookbook/suggest/`.
- **`variable-interpolation` -> Configuration.** Per design section 5.2 this page is a config feature with one conceptual hook (timing). The interpolation-timing hook already lives in `config-layering.md`, so the full reference page (syntax / where-it-applies / type coercion / `env.X` in `when:` / foreign-template passthrough / migration / security) relocates as its own Configuration page: `git mv docs/site/concepts/variable-interpolation.md docs/site/configuration/variable-interpolation.md`. It stays the link target for the deep references in `composition-and-trust.md` and `integrations/github-actions.md` (section 6 repoints them). Redirect `/docs/concepts/variable-interpolation/` -> `/docs/configuration/variable-interpolation/`.

The `Configuration` and `Cookbook` sidebar groups already `autogenerate` their directories (`astro.config.mjs`), so a relocated page auto-lists once it has a `sidebar.order`; no sidebar change is needed for the relocations.

## 5. New page: `bundled-rulesets`

Per decision section 11 item 8 ("bundled-rulesets = own concept page"). A concept page in the `composition` group covering: the 22 bundled rulesets as the on-ramp (`alint init` scaffolds them), fact-gating so a ruleset only fires where it applies, and local override via `extends:` field-merge. It teaches the model and links to the existing **Bundled Rulesets** reference at `/docs/bundled-rulesets/` (already a sidebar group; that reference is generated at bundle-build and has no `docs/site/` source dir, so link it by URL, do not expect a source file). One worked `extends:` example and one animated diagram in the established visual language (custom vars, light/dark, reduced-motion, contiguous inline SVG). Every count and ruleset name is code-verified before writing, per the Phase 5 discipline.

## 6. Cross-link updates (mandatory, not redirect-covered)

Moving pages changes their URLs, so every internal link to a concept page must be updated to the new grouped URL. Redirects catch external and stale links, but the `coverage_audit_doc_links` gate resolves links against real pages, so in-repo links must be updated, not left to redirects.

- **Within the concept pages:** every `/docs/concepts/<slug>/` cross-link (in prose and in "Going deeper") becomes `/docs/concepts/<group>/<slug>/`, with the two renamed slugs updated too.
- **Links to a retired/relocated page repoint to its new home** -- with a **self-link carve-out**: a "Going deeper" link that lives *inside* the fold target must be removed, not repointed to itself. Specifically:
  * `fixing.md` currently links `content-from` in Going deeper; since `content-from` folds INTO `fixing.md`, delete that bullet (do not self-link).
  * `config-layering.md` currently links `variable-interpolation` (and `templates`); repoint those to `/docs/configuration/variable-interpolation/` and `/docs/configuration/templates/` (they relocated), not to config-layering itself.
- **From other docs:** sweep the whole repo for `/docs/concepts/` and repoint. The links are not only in getting-started / cookbook / configuration / rules / README: they also live in `docs/site/integrations/github-actions.md` (-> variable-interpolation) and `docs/site/reference/output-formats/index.md` (-> baseline). Absolute `alint.org/docs/concepts/...` URLs in `CHANGELOG.md` are historical and left to redirects.
- **The LikeC4 model / DIAGRAMS surfaces** if any reference a concept URL (sweep, per the erratum lesson that docs surfaces hide in the model too).

## 7. Grouped sidebar (alint.org `astro.config.mjs`)

Replace the single flat entry (today `{ label: 'Concepts', autogenerate: { directory: 'docs/concepts' } }`) with a manual group whose items are per-subgroup autogenerated blocks -- the same pattern the `Reference` group already uses for its labeled sub-group, with a leading link item like the `Rules` group's `Index`. Shape:

```js
{
  label: 'Concepts',
  items: [
    { label: 'Overview', link: '/docs/concepts/' },
    { label: 'Start here', autogenerate: { directory: 'docs/concepts/start-here' } },
    { label: 'How rules target files', autogenerate: { directory: 'docs/concepts/targeting' } },
    { label: 'Composition and trust', autogenerate: { directory: 'docs/concepts/composition' } },
    { label: 'Beyond single files', autogenerate: { directory: 'docs/concepts/multi-file' } },
    { label: 'Adoption and fixing', autogenerate: { directory: 'docs/concepts/adoption' } },
    { label: 'Working with agents', autogenerate: { directory: 'docs/concepts/agents' } },
  ],
},
```

Group order is the array order; within each group, page order is the per-page `sidebar.order`. Adding a page later stays zero-config (drop a file with an order into a group dir).

## 8. Redirects (alint.org `public/_redirects`)

One entry per changed URL. The existing `public/_redirects` uses the Cloudflare `path  dest  code` shape (space-separated), so these are `301` permanent moves:

```
/docs/concepts/how-it-works/            /docs/concepts/start-here/how-alint-works/            301
/docs/concepts/the-config-model/        /docs/concepts/start-here/the-config-model/           301
/docs/concepts/kinds-families-categories/ /docs/concepts/start-here/kinds-families-categories/ 301
/docs/concepts/severity-and-exit-codes/ /docs/concepts/start-here/severity-and-exit-codes/    301
/docs/concepts/walker-and-gitignore/    /docs/concepts/targeting/the-walker-and-git/          301
/docs/concepts/scoping/                 /docs/concepts/targeting/scoping/                     301
/docs/concepts/changed-mode/            /docs/concepts/targeting/changed-mode/                301
/docs/concepts/composition-and-trust/   /docs/concepts/composition/composition-and-trust/    301
/docs/concepts/config-layering/         /docs/concepts/composition/config-layering/           301
/docs/concepts/cross-file-rules/        /docs/concepts/multi-file/cross-file-rules/           301
/docs/concepts/structured-queries/      /docs/concepts/multi-file/structured-queries/         301
/docs/concepts/fixing/                  /docs/concepts/adoption/fixing/                       301
/docs/concepts/baseline/                /docs/concepts/adoption/baseline/                     301
/docs/concepts/the-agent-surface/       /docs/concepts/agents/the-agent-surface/             301
/docs/concepts/content-from/            /docs/concepts/adoption/fixing/                       301
/docs/concepts/drop-ins/                /docs/concepts/composition/config-layering/           301
/docs/concepts/templates/               /docs/configuration/templates/                        301
/docs/concepts/suggest/                 /docs/cookbook/suggest/                               301
/docs/concepts/variable-interpolation/  /docs/configuration/variable-interpolation/           301
```

19 entries: the 14 moved-into-subdir non-index pages, the 2 retired (to their fold targets), and the 3 relocated. `index` is unchanged and needs no redirect; the new `bundled-rulesets` has no old URL. Any existing splat rule in the live file is preserved above these.

## 9. Cross-repo sequencing

1. **alint PR** (this repo): the file moves/renames/folds/relocations, the new `bundled-rulesets.md`, and all in-repo cross-link updates. This is the content of Phase 6.
2. **alint.org PR**: the grouped sidebar block (section 7) and the redirects (section 8).

The alint.org docs sync from the alint release tag, so the sidebar/redirect PR is timed with the docs-bundle refresh. Because the moves change URLs, the two PRs are coordinated: the sidebar's per-group `autogenerate` needs the subdirectories to exist in the synced content, and the redirects must be live before the old URLs 404. Sequencing detail (bundle refresh vs. redirect deploy) is confirmed against the docs-bundle pipeline during execution.

## 10. Verification

- `alint.org` build green (`npm run build:no-sync`) with the grouped sidebar rendering the 6 groups in order.
- `coverage_audit_doc_links` green (all cross-links resolve to real pages, no reliance on redirects; the self-link carve-out in section 6 is what keeps this green).
- `coverage_audit_doc_examples` green (moved YAML examples still load).
- `coverage_audit_site_docs_frontmatter` green (every moved page keeps valid frontmatter + a `sidebar.order`).
- Redirect check: each old URL 301s to its new home (the migration guard).
- Diagrams: the overflow detector stays clean on all pages (diagrams are unchanged, but re-verified after the move), and the signal scan stays em-dash-free.
- `bundled-rulesets.md`: every ruleset name/count code-verified; diagram screenshot-QA'd light/dark/360px.

## 11. Risks and mitigations

- **The drift-gate / stale-bundle chain.** URL changes ripple through the alint.org drift gates; the redirects and the coordinated bundle refresh are the mitigation. Sequence per the drift-gate architecture so a partial deploy does not 404 live pages.
- **Missed cross-links fail the doc-link gate.** Sweep repo-wide for `/docs/concepts/` (including `integrations/` and `reference/`), do not rely on redirects for in-repo links.
- **Self-links from folds.** A "Going deeper" link inside a fold target to the page being folded in becomes a self-link; the section-6 carve-out removes those explicitly.
- **Fold completeness.** Before deleting a retired page, confirm its unique content is present in the fold target (diff the headings); a redirect to a page missing the content is a silent regression.
- **Big atomic diff.** The move touches ~20 files plus cross-links. Keep the moves as `git mv` (rename detection) so the diff reads as moves + edits, not delete/add.

## 12. Out of scope

Content rewrites (done in Phases 3-5), the animated-diagram system (shipped), and the em-dash/output-hygiene work (separate PRs). Phase 6 is purely the re-org plus the one new page.
