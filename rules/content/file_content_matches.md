---
title: 'file_content_matches'
description: 'File contents must contain at least one match for a regex. alint file_content_matches rule, content family.'
sidebar:
  order: 1
---

File contents must contain at least one match for a regex.

```yaml
- id: crate-is-2024-edition
  kind: content_matches
  paths: "Cargo.toml"
  pattern: 'edition\s*=\s*"2024"'
  level: error
```

Fix: `file_append` — append declared content.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `pattern` | string | yes |  | Rust regex. File contents must match. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
