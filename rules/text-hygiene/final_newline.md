---
title: 'final_newline'
description: 'File must end with a single \n. alint final_newline rule, text hygiene family.'
sidebar:
  order: 2
---

File must end with a single `\n`. Fixable via `file_append_final_newline`.

```yaml
- id: text-files-final-newline
  kind: final_newline
  paths: "**/*.{md,yml,yaml,toml,sh}"
  level: warning
  fix:
    file_append_final_newline: {}
```

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
