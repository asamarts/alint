---
title: 'file_absent'
description: 'alint rule kind `file_absent` (Existence family).'
sidebar:
  order: 2
---

No file matching `paths` may exist. The inverse of `file_exists`.

```yaml
- id: no-backup-files
  kind: file_absent
  paths: "**/*.bak"
  level: warning
```

Fix: `file_remove` — delete every violating file.

