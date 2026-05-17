---
title: 'no_empty_files'
description: 'no_empty_files rule in alint''s structure family.'
sidebar:
  order: 3
---

Flag zero-byte files. Fixable via `file_remove`.

```yaml
- id: no-empty
  kind: no_empty_files
  paths: "**"
  level: warning
  fix:
    file_remove: {}
```

---

