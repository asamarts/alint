---
title: 'max_files_per_directory'
description: 'alint rule kind `max_files_per_directory` (Structure family).'
sidebar:
  order: 2
---

Per-directory fanout may not exceed `max_files`. Useful for vendor directories that accidentally grow to thousands of entries.

```yaml
- id: vendor-dir-fanout-cap
  kind: max_files_per_directory
  paths: "vendor/**"
  max_files: 200
  level: warning
```

