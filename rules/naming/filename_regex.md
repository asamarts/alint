---
title: 'filename_regex'
description: 'Basename matches a regex. alint filename_regex rule, naming family.'
sidebar:
  order: 2
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

