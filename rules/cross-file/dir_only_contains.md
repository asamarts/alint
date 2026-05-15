---
title: 'dir_only_contains'
description: 'alint rule kind `dir_only_contains` (Cross-file family).'
sidebar:
  order: 5
---

Every direct-child file of a directory matching `select:` must match at least one glob in `allow:`. Catches stray test data in `src/`.

```yaml
- id: src-only-rs
  kind: dir_only_contains
  select: "src/*"
  allow: ["*.rs", "README.md"]
  level: error
```

