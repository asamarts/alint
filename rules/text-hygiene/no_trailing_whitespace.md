---
title: 'no_trailing_whitespace'
description: 'alint rule kind `no_trailing_whitespace` (Text hygiene family).'
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

