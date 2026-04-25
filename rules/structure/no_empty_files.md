---
title: 'no_empty_files'
description: 'alint rule kind `no_empty_files` (Structure family).'
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

