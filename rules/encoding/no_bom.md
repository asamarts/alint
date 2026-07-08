---
title: 'no_bom'
description: 'Flag a leading UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE byte-order mark. alint no_bom rule, encoding family.'
sidebar:
  order: 1
categories: ['encoding']
---

Flag a leading UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE byte-order mark. The fixer strips whichever BOM is detected.

```yaml
- id: no-bom
  kind: no_bom
  paths: ["**/*.rs", "**/*.toml", "**/*.yml"]
  level: warning
  fix:
    file_strip_bom: {}
```

---

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
