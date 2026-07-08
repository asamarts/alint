---
title: 'filename_regex'
description: 'Basename matches a regex. alint filename_regex rule, naming family.'
sidebar:
  order: 2
categories: ['naming']
---

Basename matches a regex. Use `stem: true` to match the stem only.

```yaml
- id: toml-kebab-or-cargo
  kind: filename_regex
  paths: "**/*.toml"
  stem: true
  pattern: "[a-z][a-z0-9_-]*|Cargo"
  level: warning
```

---

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `pattern` | string | yes |  | Rust regex, automatically anchored with ^...$ by the engine. |
| `stem` | boolean |  | `false` | Match the file stem (no extension) instead of the full basename. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
