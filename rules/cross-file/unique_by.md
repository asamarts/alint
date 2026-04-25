---
title: 'unique_by'
description: 'alint rule kind `unique_by` (Cross-file family).'
sidebar:
  order: 6
---

No two files matching `paths` may share the value of `key` (a path template). Catches basename collisions across subdirectories.

```yaml
- id: unique-basenames
  kind: unique_by
  paths: "src/**/*.rs"
  key: "{stem}"
  level: warning
```

