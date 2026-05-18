---
title: 'dir_only_contains'
description: 'Every direct-child file of a directory matching select: must match at least one glob in allow:. alint dir_only_contains rule, cross-file family.'
sidebar:
  order: 8
---

Every direct-child file of a directory matching `select:` must match at least one glob in `allow:`. Catches stray test data in `src/`.

```yaml
- id: src-only-rs
  kind: dir_only_contains
  select: "src/*"
  allow: ["*.rs", "README.md"]
  level: error
```

