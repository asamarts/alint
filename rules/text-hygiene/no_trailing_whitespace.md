---
title: 'no_trailing_whitespace'
description: 'No line may end with space or tab. alint no_trailing_whitespace rule, text hygiene family.'
sidebar:
  order: 1
---

No line may end with space or tab.

```yaml
- id: rust-no-trailing-ws
  kind: no_trailing_whitespace
  paths: "crates/**/src/**/*.rs"
  level: warning
  fix:
    file_trim_trailing_whitespace: {}
```

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
