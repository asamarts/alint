---
title: 'filename_case'
description: 'alint rule kind `filename_case` (Naming family).'
sidebar:
  order: 1
---

Basename (stem only or full) matches a case convention: `snake`, `kebab`, `pascal`, `camel`, `screaming-snake`, `flat`, `lower`, `upper`.

```yaml
- id: rust-snake-case
  kind: filename_case
  paths: "crates/**/src/**/*.rs"
  case: snake
  level: error
```

Fix: `file_rename` — converts the stem to the configured case, preserving extension.

