---
title: 'dir_absent'
description: 'Directory counterpart of file_absent. alint dir_absent rule, existence family.'
sidebar:
  order: 4
categories: ['existence']
---

Directory counterpart of `file_absent`. The match-and-fire semantics are the same as `file_absent` — including the `.gitignore` interaction. A `dir_absent` rule with `paths: "**/target"` only fires when `target/` exists in the walked tree; if it's gitignored, the walker filters it out and the rule stays silent.

```yaml
- id: no-tracked-target
  kind: dir_absent
  paths: "**/target"
  level: error
```

**Optional `root_only: true`** (like `dir_exists`) restricts the check to the repository root: a directory forbidden at the root does not fire on nested directories of the same name.
**Optional `git_tracked_only: true`** restricts the check to directories that contain at least one git-tracked file. With it set, a developer's locally-built `target/` (gitignored, no tracked content) doesn't trigger; a `target/` whose contents made it into git's index does. This is the canonical "don't let `target/` be committed" semantic.

```yaml
- id: no-tracked-target
  kind: dir_absent
  paths: "**/target"
  git_tracked_only: true
  level: error
```

See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics.

---

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `git_tracked_only` | boolean |  | `false` | Restrict matches to directories that contain at least one git-tracked file. No effect outside a git repo. Default `false`. |
| `root_only` | boolean |  | `false` | If true, only a directory directly at the repository root is forbidden; a nested match with the same name is allowed. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
