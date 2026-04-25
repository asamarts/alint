---
title: 'dir_absent'
description: 'alint rule kind `dir_absent` (Existence family).'
sidebar:
  order: 4
---

Directory counterpart of `file_absent`. The match-and-fire semantics are the same as `file_absent` — including the `.gitignore` interaction. A `dir_absent` rule with `paths: "**/target"` only fires when `target/` exists in the walked tree; if it's gitignored, the walker filters it out and the rule stays silent.

```yaml
- id: no-tracked-target
  kind: dir_absent
  paths: "**/target"
  level: error
```

See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics.

---

