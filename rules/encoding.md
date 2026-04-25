---
title: 'Encoding'
description: 'Rule reference: the encoding family.'
sidebar:
  order: 8
---

### `no_bom`

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

