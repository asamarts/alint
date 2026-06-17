---
title: 'file_exists'
description: 'Every glob match in paths must correspond to a real file. alint file_exists rule, existence family.'
sidebar:
  order: 1
---

Every glob match in `paths` must correspond to a real file. Use an array to accept any of several names.

```yaml
- id: readme-exists
  kind: file_exists
  paths: ["README.md", "README", "README.rst"]
  root_only: true
  level: error
```

Fix: `file_create` — write a declared `content`. With an array of `paths`, the fix creates the first entry.

**Optional `git_tracked_only: true`** further requires that the matching file be in git's index — useful for rules like "every release must commit a CHANGELOG entry" where local-only files shouldn't satisfy the requirement. Outside a git repo, the rule fails (no file qualifies). See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `git_tracked_only` | git tracked only |  |  |  |
| `respect_gitignore` | per rule respect gitignore |  |  |  |
| `root_only` | boolean |  | `false` | If true, only files directly at the repository root satisfy the rule. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
