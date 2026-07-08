---
title: 'max_files_per_directory'
description: 'Per-directory fanout may not exceed max_files. alint max_files_per_directory rule, structure family.'
sidebar:
  order: 2
categories: ['structure']
---

Per-directory fanout may not exceed `max_files`. Useful for vendor directories that accidentally grow to thousands of entries.

```yaml
- id: vendor-dir-fanout-cap
  kind: max_files_per_directory
  paths: "vendor/**"
  max_files: 200
  level: warning
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max_files` | integer (>= 1) | yes |  | Maximum number of in-scope files allowed as immediate children of any one directory (non-recursive). |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
