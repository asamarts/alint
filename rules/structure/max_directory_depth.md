---
title: 'max_directory_depth'
description: 'Tree depth from repo root may not exceed max_depth. alint max_directory_depth rule, structure family.'
sidebar:
  order: 1
categories: ['structure']
---

Tree depth from repo root may not exceed `max_depth`. A shallow depth stops deeply-nested imports and keeps CI path globs sane.

```yaml
- id: shallow-tree
  kind: max_directory_depth
  paths: "**"
  max_depth: 6
  level: warning
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max_depth` | integer (>= 1) | yes |  | Maximum allowed path depth (number of `/`-separated components). |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
