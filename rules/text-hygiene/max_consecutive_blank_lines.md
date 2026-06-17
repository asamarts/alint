---
title: 'max_consecutive_blank_lines'
description: 'Cap runs of blank lines to max. alint max_consecutive_blank_lines rule, text hygiene family.'
sidebar:
  order: 6
---

Cap runs of blank lines to `max`. A blank line is empty or whitespace-only.

```yaml
- id: md-tidy
  kind: max_consecutive_blank_lines
  paths: "**/*.md"
  max: 1
  level: warning
  fix:
    file_collapse_blank_lines: {}
```

---

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max` | integer (>= 0) | yes |  | Maximum number of blank lines allowed in a row. `0` means no blank lines at all. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
