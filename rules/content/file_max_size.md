---
title: 'file_max_size'
description: 'File must be at most max_bytes in size. alint file_max_size rule, content family.'
sidebar:
  order: 7
categories: ['content']
---

File must be at most `max_bytes` in size. Catches accidental large-blob commits.

```yaml
- id: no-huge-blobs
  kind: max_size
  paths: "**"
  max_bytes: 5242880   # 5 MiB
  level: warning
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max_bytes` | integer (>= 0) | yes |  | Maximum allowed file size in bytes. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
