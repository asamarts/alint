---
title: 'file_min_size'
description: 'File must be at least min_bytes in size. alint file_min_size rule, content family.'
sidebar:
  order: 8
categories: ['content']
---

File must be at least `min_bytes` in size. Catches placeholder / stub files that pass existence checks but add no information (a 0-byte `LICENSE`, a `README.md` with only a title).

```yaml
- id: license-non-empty
  kind: min_size
  paths: ["LICENSE", "LICENSE.md", "LICENSE-APACHE", "LICENSE-MIT"]
  min_bytes: 200
  level: warning
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `min_bytes` | integer (>= 0) | yes |  | Minimum allowed file size in bytes. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
