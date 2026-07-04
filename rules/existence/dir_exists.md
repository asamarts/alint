---
title: 'dir_exists'
description: 'Directory counterpart of file_exists. alint dir_exists rule, existence family.'
sidebar:
  order: 3
---

Directory counterpart of `file_exists`. Every match must correspond to a real directory in the walked tree.

```yaml
- id: docs-dir-exists
  kind: dir_exists
  paths: "docs"
  root_only: true
  level: error
```

**Optional `git_tracked_only: true`** further requires that the directory contain at least one tracked file. A tree with a `docs/` checked out from a stale clone where every file was later removed via `git rm` would fail under this stricter check. See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `git_tracked_only` | boolean |  | `false` | Restrict matches to directories that contain at least one git-tracked file. No effect outside a git repo. Default `false`. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
