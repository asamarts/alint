---
title: 'file_is_ascii'
description: 'Every byte in the file must be < 0x80. alint file_is_ascii rule, content family.'
sidebar:
  order: 14
---

Every byte in the file must be < 0x80. Strict variant of `is_text` for configs that must round-trip through strictly-ASCII tools.

```yaml
- id: licences-are-ascii
  kind: file_is_ascii
  paths: "LICENSE*"
  level: error
```

---

