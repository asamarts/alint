---
title: 'file_is_ascii'
description: 'alint rule kind `file_is_ascii` (Content family).'
sidebar:
  order: 20
---

Every byte in the file must be < 0x80. Strict variant of `is_text` for configs that must round-trip through strictly-ASCII tools.

```yaml
- id: licences-are-ascii
  kind: file_is_ascii
  paths: "LICENSE*"
  level: error
```

---

