---
title: 'final_newline'
description: 'alint rule kind `final_newline` (Text hygiene family).'
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

