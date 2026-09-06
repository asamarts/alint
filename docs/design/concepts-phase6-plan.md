---
title: "Concepts redesign: Phase 6 execution plan (relocations, renames, grouped sidebar, redirects)"
description: The concrete, file-level plan for the final phase of the Concepts-section redesign. Batches every URL change (renames, subdir grouping, retirements, relocations) into one atomic cross-repo cut, plus the new bundled-rulesets page.
---

# Phase 6 execution plan

The final phase of the [Concepts redesign](./concepts-section-redesign.md). Phases 1-5 rewrote the content and shipped the animated diagrams while deliberately deferring every filename and URL change to here, so the whole re-org lands as one atomic cut with a single redirect set. This doc is the file-level plan; nothing is executed until it is approved.

It follows the redesign doc's information architecture (section 5.1), the decided sidebar mechanism (section 5.4), the relocations table (section 5.2), and the migration/redirect section (section 9). It is a **cross-repo** change: page moves and content folds happen in `alint/docs/site/concepts/`, while the grouped sidebar and the redirects are edits in the `alint.org` repo.

## 1. Goal and shape

Turn today's 20 flat concept pages into ~16 grouped concept pages plus 2 relocated feature/command pages, with every old URL redirected. After Phase 6 the redesign is complete.

- **Stays in Concepts (15 existing + 1 new), moved into 6 group subdirectories.**
- **Renamed (2):** `how-it-works` -> `how-alint-works`, `walker-and-gitignore` -> `the-walker-and-git`.
- **Retired from Concepts (3), content folded + redirected:** `content-from`, `drop-ins`, `variable-interpolation`.
- **Relocated out of Concepts (2):** `templates` -> Configuration, `suggest` -> Cookbook.
- **New (1):** `bundled-rulesets` (decision #229 Q2).

## 2. Group subdirectories and the move map

Pages move into `docs/site/concepts/<group>/`, so the group slug becomes a URL segment (for example `/docs/concepts/targeting/scoping/`). The section landing `index.md` stays at the section root. Per-group `sidebar.order` restarts at 1 (today's duplicate/gap orders are retired by the move).

| Group dir (label) | Page | Old path | New path |
|---|---|---|---|
| (root) | Concepts hub | `concepts/index.md` | `concepts/index.md` (unchanged) |
| `start-here` (Start here) | how-alint-works | `concepts/how-it-works.md` | `concepts/start-here/how-alint-works.md` |
| `start-here` | the-config-model | `concepts/the-config-model.md` | `concepts/start-here/the-config-model.md` |
| `start-here` | kinds-families-categories | `concepts/kinds-families-categories.md` | `concepts/start-here/kinds-families-categories.md` |
| `start-here` | severity-and-exit-codes | `concepts/severity-and-exit-codes.md` | `concepts/start-here/severity-and-exit-codes.md` |
| `targeting` (How rules target files) | the-walker-and-git | `concepts/walker-and-gitignore.md` | `concepts/targeting/the-walker-and-git.md` |
| `targeting` | scoping | `concepts/scoping.md` | `concepts/targeting/scoping.md` |
| `targeting` | changed-mode | `concepts/changed-mode.md` | `concepts/targeting/changed-mode.md` |
| `composition` (Composition and trust) | composition-and-trust | `concepts/composition-and-trust.md` | `concepts/composition/composition-and-trust.md` |
| `composition` | bundled-rulesets (NEW) | -- | `concepts/composition/bundled-rulesets.md` |
| `composition` | config-layering | `concepts/config-layering.md` | `concepts/composition/config-layering.md` |
| `multi-file` (Beyond single files) | cross-file-rules | `concepts/cross-file-rules.md` | `concepts/multi-file/cross-file-rules.md` |
| `multi-file` | structured-queries | `concepts/structured-queries.md` | `concepts/multi-file/structured-queries.md` |
| `adoption` (Adoption and fixing) | fixing | `concepts/fixing.md` | `concepts/adoption/fixing.md` |
| `adoption` | baseline | `concepts/baseline.md` | `concepts/adoption/baseline.md` |
| `agents` (Working with agents) | the-agent-surface | `concepts/the-agent-surface.md` | `concepts/agents/the-agent-surface.md` |

Group order in the sidebar: Start here, How rules target files, Composition and trust, Beyond single files, Adoption and fixing, Working with agents (progressive: mental model -> targeting -> composition -> multi-file -> adoption -> agents).

## 3. Retirements (fold + redirect)

Each retired page's unique content folds into the surviving concept page; the standalone file is deleted and its old URL redirected. The fold targets already carry most of this from Phase 4/5, so the work is migrating the remaining unique material, not rewriting.

- **`content-from` -> `adoption/fixing`.** `fixing.md` already has a `## content_from` section. Migrate the parts it lacks: "when to reach for it" (LICENSE/SPDX rationale) and the monorepo-templates note. The exhaustive per-op detail belongs in the fix-op reference, not the concept page. Redirect `/docs/concepts/content-from/` -> `/docs/concepts/adoption/fixing/`.
- **`drop-ins` -> `composition/config-layering`.** `config-layering.md` already covers `.alint.d/` layering (verified: 10 hits). Migrate anything unique from drop-ins' Layout / Trust posture / What-gets-merged-where / Limits. Redirect `/docs/concepts/drop-ins/` -> `/docs/concepts/composition/config-layering/`.
- **`variable-interpolation` -> `composition/config-layering` (timing) + Configuration (full reference).** The conceptual hook (the three interpolation timing layers) folds into config-layering; the full syntax/coercion/security reference moves to Configuration. Redirect `/docs/concepts/variable-interpolation/` -> `/docs/concepts/composition/config-layering/`.

## 4. Relocations (out of Concepts)

- **`templates` -> Configuration.** Move `concepts/templates.md` to `docs/configuration/templates.md` (a config construct, not a concept). Redirect `/docs/concepts/templates/` -> `/docs/configuration/templates/`.
- **`suggest` -> Cookbook.** Move `concepts/suggest.md` to `docs/cookbook/suggest.md` (a command workflow). Redirect `/docs/concepts/suggest/` -> `/docs/cookbook/suggest/`.

Both target sections already exist in the `alint.org` sidebar (`Configuration` autogenerates `docs/configuration`, `Cookbook` autogenerates `docs/cookbook`), so no sidebar change is needed for the relocations beyond the file move and a `sidebar.order`.

## 5. New page: `bundled-rulesets`

Per decision #229 Q2 ("bundled-rulesets = own concept page"). A concept page in the `composition` group covering: the 22 bundled rulesets as the on-ramp (`alint init` scaffolds them), fact-gating so a ruleset only fires where it applies, and local override via `extends:` field-merge. It teaches the model and links to the existing **Bundled Rulesets** reference section (`docs/bundled-rulesets`, already in the sidebar) for per-ruleset detail. One worked `extends:` example and one animated diagram in the established visual language (custom vars, light/dark, reduced-motion, contiguous inline SVG). Every count and ruleset name is code-verified before writing, per the Phase 5 discipline.

## 6. Cross-link updates (mandatory, not redirect-covered)

Moving pages changes their URLs, so every internal link to a concept page must be updated to the new grouped URL. Redirects catch external and stale links, but the `coverage_audit_doc_links` gate resolves links against real pages, so in-repo links must be updated, not left to redirects.

- **Within the concept pages:** every `/docs/concepts/<slug>/` cross-link (in prose and in "Going deeper") becomes `/docs/concepts/<group>/<slug>/`, with the two renamed slugs updated too. Links to retired pages repoint to their fold target.
- **From other docs:** getting-started, cookbook, configuration, rules pages, and the README that link to a concept page. Sweep repo-wide for `/docs/concepts/` and repoint.
- **The LikeC4 model / DIAGRAMS surfaces** if any reference a concept URL (sweep, per the erratum lesson that docs surfaces hide in the model too).

## 7. Grouped sidebar (alint.org `astro.config.mjs`)

Replace the single flat entry (today `{ label: 'Concepts', autogenerate: { directory: 'docs/concepts' } }`) with a manual group whose items are per-subgroup autogenerated blocks -- the same pattern the `Reference` group already uses for its labeled sub-group. Shape:

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

One entry per changed URL (Cloudflare `_redirects`, `301`). The full set:

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
/docs/concepts/variable-interpolation/  /docs/concepts/composition/config-layering/           301
/docs/concepts/templates/               /docs/configuration/templates/                        301
/docs/concepts/suggest/                 /docs/cookbook/suggest/                               301
```

(Exact `_redirects` syntax and any existing splat rules to be confirmed against the live file during execution.)

## 9. Cross-repo sequencing

1. **alint PR** (this repo): the file moves/renames/folds/relocations, the new `bundled-rulesets.md`, and all in-repo cross-link updates. This is the content of Phase 6.
2. **alint.org PR**: the grouped sidebar block (section 7) and the redirects (section 8).

The alint.org docs sync from the alint release tag, so the sidebar/redirect PR is timed with the docs-bundle refresh. Because the moves change URLs, the two PRs are coordinated: the sidebar's per-group `autogenerate` needs the subdirectories to exist in the synced content, and the redirects must be live before the old URLs 404. Sequencing detail (bundle refresh vs. redirect deploy) is confirmed against the docs-bundle pipeline during execution.

## 10. Verification

- `alint.org` build green (`npm run build:no-sync`) with the grouped sidebar rendering the 6 groups in order.
- `coverage_audit_doc_links` green (all cross-links resolve to real pages, no reliance on redirects).
- `coverage_audit_doc_examples` green (moved YAML examples still load).
- `coverage_audit_site_docs_frontmatter` green (every moved page keeps valid frontmatter + a `sidebar.order`).
- Redirect check: each old URL 301s to its new home (the migration guard).
- Diagrams: the overflow detector stays clean on all pages (diagrams are unchanged, but re-verified after the move), and the signal scan stays em-dash-free.
- `bundled-rulesets.md`: every ruleset name/count code-verified; diagram screenshot-QA'd light/dark/360px.

## 11. Risks and mitigations

- **The drift-gate / stale-bundle chain.** URL changes ripple through the alint.org drift gates; the redirects and the coordinated bundle refresh are the mitigation. Sequence per the drift-gate architecture so a partial deploy does not 404 live pages.
- **Missed cross-links fail the doc-link gate.** Sweep repo-wide for `/docs/concepts/`, do not rely on redirects for in-repo links.
- **Fold completeness.** Before deleting a retired page, confirm its unique content is present in the fold target (diff the headings); a redirect to a page missing the content is a silent regression.
- **Big atomic diff.** The move touches ~20 files plus cross-links. Keep the moves as `git mv` (rename detection) so the diff reads as moves + edits, not delete/add.

## 12. Out of scope

Content rewrites (done in Phases 3-5), the animated-diagram system (shipped), and the em-dash/output-hygiene work (a separate PR). Phase 6 is purely the re-org plus the one new page.
