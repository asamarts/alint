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

**What "exists" means**: alint walks the filesystem and honours `.gitignore` by default, so a `file_absent` rule fires whenever a matching file is **present in the walked tree**, not when it's tracked in git. Files filtered by `.gitignore` are invisible to the rule. See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics, the `--no-gitignore` flag, and the gap between this and git's actual index.

