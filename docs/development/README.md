# docs/development

Contributor-facing engineering notes — design exploration, audit
logs, validation passes, and the in-flight maintenance trackers
that don't fit anywhere else in the source tree.

This directory mixes two audiences:

1. **Public docs synced to alint.org.** Currently one file:
   `rule-authoring.md`. The `xtask docs-export` pipeline at
   `xtask/src/docs_export.rs:28` enumerates this explicitly; adding
   another public dev doc means adding a path constant there and
   bundling it into the `target/docs-bundle/development/` output.
2. **Internal-but-tracked engineering artefacts.** Audit logs,
   per-batch analysis files, cleanup-cycle trackers. Committed so
   the institutional memory survives, but not pushed to alint.org.
   Some are linked-by-URL from canonical docs (e.g. `ROADMAP.md`
   references `launch-evidence.md`); the link points at the GitHub
   raw file, not at the public site.

## File-by-file

| File | Audience | Notes |
|---|---|---|
| `rule-authoring.md` | **Public** (synced) | Four-step workflow every new rule kind, bundled ruleset, and alias follows. Synced to `alint.org/docs/development/rule-authoring/`. |
| `launch-evidence.md` | **Public** (URL-referenced) | Engineering audit summary across 30 OSS case studies. Linked from `ROADMAP.md` + the launch blog post; lives only here, served via `raw.githubusercontent`. |
| `CONFIG-AUTHORING.md` | Internal | 22-pitfall catalogue from the launch-prep validation passes (P2a + P2b). Reference material for `.alint.yml` schema, parser, and runtime-audit work. |
| `case-study-deep-analysis-log.md` | Internal | Master tracking + cross-cutting findings from the per-case-study deep analyses that fed the v0.10 rule-kind backlog. |
| `case-study-revalidation-log.md` | Internal | Master tracker for the 2026-05-07 30-case-study revalidation pass against v0.9.17. |
| `case-study-revalidation-batch-{1..6}.md` | Internal | Per-batch findings from the same pass (5 case studies each, alphabetical). |
| `marketing-extraction-batch-{1..6}.md` | Internal | Per-batch marketing / optics / positioning extraction from the case studies. Source material for landing-page copy + the launch blog post. |
| `archive/v0.9.22-cleanup-plan.md` | Internal (archived) | Closed tracker for the v0.9.22 doc + prevention-automation cleanup cycle (all 14 items done). Moved to `archive/` at cycle-close per the one-shot-tracker convention below. |

## Conventions

When adding a new file:

- **Public doc** (showing up on alint.org): add the path constant
  to `xtask/src/docs_export.rs`'s `paths` module, then add a row
  here marked **Public** with the synced URL. The sidebar in
  alint.org's `astro.config.mjs` may need a matching entry too.
- **Internal artefact** (audit log, analysis pass, etc.): just
  commit it; add a row here marked **Internal** with one-line
  context. Multi-file series (per-batch logs) share a single
  row with a `{1..N}` glob in the filename column.
- **One-shot tracker** (cleanup cycle, refactor plan): mark
  **Internal (in-flight tracker)** so a future reader knows the
  file's lifecycle is bounded. At cycle-close, move it to the
  `archive/` subdir and re-mark the row **Internal (archived)**.
  (Established with `archive/v0.9.22-cleanup-plan.md` — the first
  closed cycle; supersedes the older "keep in place" convention.)

## Historical retention

The case-study and marketing-extraction batch files are kept
around even though their immediate use (the v0.9 launch evidence
gathering, the v0.9.17 revalidation pass) is complete. They're
the only record of what 30 specific OSS repos contributed to the
rule-kind backlog, the marketing copy, and the pitfall catalogue.
If they're ever moved or deleted, the canonical references in
`ROADMAP.md` and the launch blog post need to be updated to match,
or replaced with a summary that doesn't depend on the per-batch
detail.
