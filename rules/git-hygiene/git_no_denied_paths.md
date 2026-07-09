---
title: 'git_no_denied_paths'
description: 'Fire when any tracked file matches a configured glob denylist. alint git_no_denied_paths rule, git hygiene family.'
sidebar:
  order: 4
categories: ['git-hygiene', 'security-unicode-sanity']
---

Fire when any tracked file matches a configured glob denylist. The absence-axis companion of `git_tracked_only`: instead of asking "does this tracked path exist?", it asks "is anything tracked that matches my denylist?" One rule covers what would otherwise need one `file_absent` per pattern. Reports every matching denylist entry per offending path so a single file hitting two patterns surfaces both.

```yaml
- id: no-secrets-or-keys
  kind: git_no_denied_paths
  denied:
    - "*.env"
    - ".env*"
    - "*.pem"
    - "id_rsa"
    - "secrets/**"
  level: error
  message: "Don't commit secrets or credentials."
```

An optional `since: <git-ref>` scopes the check to denied paths that changed in the `<ref>...HEAD` diff — the PR-scoped shape, which catches a secret added in the PR even if HEAD's tree still tracks an older one. It accepts the `{{env.X}}` interpolation (e.g. `since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"`); an unresolvable ref hard-fails with a shallow-clone hint.

Outside a git repo (or when `git` isn't on `PATH`) the rule silently no-ops — the rule's intent only makes sense inside a tracked working tree. Check-only — `git rm --cached` is too destructive to automate.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `denied` | list of string | yes |  | Globset patterns no tracked path may match. Both whole-path patterns (`secrets/**`) and basename-only patterns (`*.env`) work. |
| `since` | string |  | `null` | Optional git ref. When set, only denied paths that changed in the `<since>...HEAD` diff are flagged, catches a secret added in a PR even if HEAD's tree still tracks an older one. Accepts the `{{env.X}}` interpolation, e.g. `since: "{{env.ALINT_BASE_SHA \| default('origin/main')}}"`. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
