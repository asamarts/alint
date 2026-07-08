---
title: 'no_case_conflicts'
description: 'Flag paths that differ only by case (e.g. alint no_case_conflicts rule, portable metadata family.'
sidebar:
  order: 1
categories: ['portable-metadata']
---

Flag paths that differ only by case (e.g. `README.md` + `readme.md`). They can't coexist on macOS HFS+/APFS or Windows NTFS defaults, so a Linux-only dev committing both breaks checkouts for teammates.

```yaml
- id: no-case-colliding-paths
  kind: no_case_conflicts
  paths: "**"
  level: error
```

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
