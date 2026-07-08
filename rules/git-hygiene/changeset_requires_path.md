---
title: 'changeset_requires_path'
description: 'The <since>...HEAD diff must **add** (git status A) at least one path matching add_glob:, the "did you add a changelog entry?" gate.'
sidebar:
  order: 12
categories: ['git-hygiene']
---

The `<since>...HEAD` diff must **add** (git status `A`) at least one path matching `add_glob:` — the "did you add a changelog entry?" gate. Three corpus signals: prettier's `changelog_unreleased/`, cpython's `Misc/NEWS.d/next/`, pnpm's `.changeset/*.md`. `since:` (the base ref) is required — the rule asserts about the *set of files a contribution adds*, so it's diff-scoped. An optional `when_changed:` gates the requirement on some other glob having changed (don't demand a changelog for a docs-only PR); with no gate, any non-empty changeset triggers it. Builds on the same `<since>...HEAD` three-dot (merge-base) diff as `alint check --changed`. Silent no-op outside a git repo or when nothing relevant changed; a `since:` that fails to resolve hard-fails with a shallow-clone hint.

```yaml
- id: changelog-per-pr
  kind: changeset_requires_path
  add_glob: ".changeset/*.md"          # the PR must add a changeset file…
  when_changed: "src/**"               # …but only when it touches src/
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

---

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `add_glob` | string | yes |  | Glob; the diff must ADD at least one path matching it. |
| `since` | string | yes |  | Base ref for the `<since>...HEAD` diff. Use the canonical `{{env.X}}` interpolation, e.g. `since: "{{env.ALINT_BASE_SHA \| default('origin/main')}}"`. |
| `when_changed` | string |  | `null` | Optional gate: only require the add when some path matching this glob changed (any status). Omit to require it on any non-empty changeset. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
