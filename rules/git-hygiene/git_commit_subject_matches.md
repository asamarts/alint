---
title: 'git_commit_subject_matches'
description: 'Each commit''s subject line (the first line of its message) must match the matches: regex, the subject-grammar member of the commit family.'
sidebar:
  order: 8
categories: ['git-hygiene']
---

Each commit's subject line (the first line of its message) must match the `matches:` regex — the subject-grammar member of the commit family. Enforces a prefix + shape convention like go / Gerrit's `pkg/path: lowercase summary`, node's `subsystem: description`, or conventional-commit types. The regex is anchored to the **subject alone** (so `^…$` describes the first line exactly), unlike `git_commit_message`'s `pattern:` which matches the whole subject + body; for a subject-length cap use `git_commit_message`'s `subject_max_length:`. Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes (HEAD-only when `since:` is unset, `<since>..HEAD` when set; silent outside a git repo; a bad `since:` ref hard-fails with a shallow-clone hint).

```yaml
- id: subject-grammar
  kind: git_commit_subject_matches
  matches: '^[a-z0-9_/.-]+: [a-z].{0,70}$'   # `component: lowercase summary`
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `include_merges` | boolean |  | `false` | When validating a range (`since:` set), include merge commits. Has no effect when `since:` is unset; combining `include_merges: true` with no `since:` is a load-time error. |
| `matches` | string | yes |  | Rust-regex the commit subject (first line of the message) must match. |
| `since` | string |  | `null` | Git ref to use as the base of the commit range. When set, validates every commit in `<since>..HEAD` instead of just HEAD. Use the canonical `{{env.X}}` interpolation to pass a SHA via an env var, e.g. `since: "{{env.ALINT_BASE_SHA \| default('origin/main')}}"`. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
