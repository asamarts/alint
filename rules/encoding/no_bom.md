---
title: 'no_bom'
description: 'alint rule kind `no_bom` (Encoding family).'
sidebar:
  order: 1
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

