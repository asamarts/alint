# Marketing drafts

Active drafts for P3 marketing-refresh work. Each `.md` here corresponds
to a row in `../STATE.md`.

## File frontmatter convention

Every draft starts with:

```markdown
---
destination: <where this lands when published — file path or URL>
status: drafting | ready | published
blocks_on: <release version, doc dependency, etc. — optional>
last_touched: YYYY-MM-DD
---
```

The intent is that *anyone* (you, a reviewer, a future agent) can read
the frontmatter and know what this draft becomes, what state it's in,
and what's gating publication.

## Lifecycle

1. Draft created → frontmatter `status: drafting`
2. Author signals "this is good" → `status: ready`
3. Publishing pipeline (manual or scripted) ships it → `status: published`
4. After publish, the row in `../STATE.md` flips from "drafting" to
   "live" and the draft file CAN be retained as a historical reference
   or deleted at the author's discretion.

## Naming

Match the `STATE.md` row name (e.g., `readme-hero.md`,
`alint-org-compare.md`). Use kebab-case. The destination URL or file
path lives only in the frontmatter, not the filename.
