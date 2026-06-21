---
title: 'line_max_width'
description: 'Cap line length in characters (not bytes, code points). alint line_max_width rule, text hygiene family.'
sidebar:
  order: 4
---

Cap line length in characters (not bytes — code points). Optional `tab_width` for tab expansion.

```yaml
- id: docs-80-col
  kind: line_max_width
  paths: "docs/**/*.md"
  max_width: 80
  level: info
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max_width` | integer (>= 1) | yes |  | Maximum number of Unicode scalar values (chars) allowed per line. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
