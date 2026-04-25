---
title: 'Naming'
description: 'Rule reference: the naming family.'
sidebar:
  order: 5
---

### `filename_case`

Basename (stem only or full) matches a case convention: `snake`, `kebab`, `pascal`, `camel`, `screaming-snake`, `flat`, `lower`, `upper`.

```yaml
- id: rust-snake-case
  kind: filename_case
  paths: "crates/**/src/**/*.rs"
  case: snake
  level: error
```

Fix: `file_rename` — converts the stem to the configured case, preserving extension.

### `filename_regex`

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

