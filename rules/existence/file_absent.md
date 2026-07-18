---
title: 'file_absent'
description: 'No file matching paths may exist in the walked tree. alint file_absent rule, existence family.'
sidebar:
  order: 2
categories: ['existence']
---

No file matching `paths` may exist in the walked tree. The inverse of `file_exists`.

```yaml
- id: no-backup-files
  kind: file_absent
  paths: "**/*.bak"
  level: warning
```

Fix: `file_remove` — delete every violating file.

**Optional `root_only: true`** (like `file_exists`) restricts the check to the repository root: a file forbidden at the root does not fire on nested copies of the same name.
**Optional `git_tracked_only: true`** restricts the check to files in git's index. With it set, the rule fires only on tracked paths regardless of `.gitignore` state — closing the gap where a `git add -f`'d file slips past the walker's gitignore filter. Outside a git repo the rule becomes a silent no-op.

```yaml
- id: no-tracked-env
  kind: file_absent
  paths: ".env"
  git_tracked_only: true
  level: error
```

**What "exists" means**: alint walks the filesystem and honours `.gitignore` by default, so a `file_absent` rule fires whenever a matching file is **present in the walked tree**, not when it's tracked in git. Files filtered by `.gitignore` are invisible to the rule. See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics, the `--no-gitignore` flag, and the gap between this and git's actual index.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `git_tracked_only` | boolean |  | `false` | Restrict matches to files tracked in git's index: entries present in the walked tree but not in `git ls-files` are skipped. No effect outside a git repo. Default `false`. |
| `root_only` | boolean |  | `false` | If true, only a file matching `paths` directly at the repository root is forbidden; a nested match with the same name is allowed. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
