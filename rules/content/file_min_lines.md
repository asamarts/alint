---
title: 'file_min_lines'
description: 'File must have at least min_lines lines (\n-terminated, with an unterminated trailing segment counting as one more, wc -l semantics).'
sidebar:
  order: 9
---

File must have at least `min_lines` lines (`\n`-terminated, with an unterminated trailing segment counting as one more — `wc -l` semantics). Use for "README has more than a title and a TODO".

```yaml
- id: readme-non-stub
  kind: min_lines
  paths: ["README.md", "README"]
  min_lines: 5
  level: info
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `min_lines` | integer (>= 0) | yes |  | Minimum allowed line count. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
