---
title: 'file_max_size'
description: 'alint rule kind `file_max_size` (Content family).'
sidebar:
  order: 7
---

File must be at most `max_bytes` in size. Catches accidental large-blob commits.

```yaml
- id: no-huge-blobs
  kind: max_size
  paths: "**"
  max_bytes: 5242880   # 5 MiB
  level: warning
```

