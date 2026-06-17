---
title: 'line_endings'
description: 'Every line ending matches target: lf or crlf. alint line_endings rule, text hygiene family.'
sidebar:
  order: 3
---

Every line ending matches `target`: `lf` or `crlf`. Mixed endings in a single file fail.

```yaml
- id: lf-only
  kind: line_endings
  paths: ["**/*.rs", "**/*.md"]
  target: lf
  level: warning
  fix:
    file_normalize_line_endings: {}
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `target` | one of `lf` \| `crlf` | yes |  | Required line ending style: `lf` or `crlf`. Mixed endings within a file also fail. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
