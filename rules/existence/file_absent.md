---
title: 'file_absent'
description: 'alint rule kind `file_absent` (Existence family).'
sidebar:
  order: 2
---

No file matching `paths` may exist in the walked tree. The inverse of `file_exists`.

```yaml
- id: no-backup-files
  kind: file_absent
  paths: "**/*.bak"
  level: warning
```

Fix: `file_remove` — delete every violating file.

**Optional `git_tracked_only: true`** restricts the check to files in git's index. With it set, the rule fires only on tracked paths regardless of `.gitignore` state — closing the gap where a `git add -f`'d file slips past the walker's gitignore filter. Outside a git repo the rule becomes a silent no-op.

```yaml
- id: no-tracked-env
  kind: file_absent
  paths: ".env"
  git_tracked_only: true
  level: error
```

**What "exists" means**: alint walks the filesystem and honours `.gitignore` by default, so a `file_absent` rule fires whenever a matching file is **present in the walked tree**, not when it's tracked in git. Files filtered by `.gitignore` are invisible to the rule. See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics, the `--no-gitignore` flag, and the gap between this and git's actual index.

