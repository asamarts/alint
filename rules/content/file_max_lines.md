---
title: 'file_max_lines'
description: 'File must have at most max_lines lines, using the same accounting as file_min_lines. alint file_max_lines rule, content family.'
sidebar:
  order: 10
categories: ['content', 'structure']
---

File must have at most `max_lines` lines, using the same accounting as `file_min_lines`. Catches the everything-module anti-pattern — a `lib.rs` / `index.ts` / `helpers.py` that grew unbounded.

```yaml
- id: cap-source-file-size
  kind: file_max_lines
  paths: "src/**/*.rs"
  max_lines: 800
  level: warning
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max_lines` | integer (>= 0) | yes |  | Maximum allowed line count. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
