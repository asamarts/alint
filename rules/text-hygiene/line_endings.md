---
title: 'line_endings'
description: 'alint rule kind `line_endings` (Text hygiene family).'
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

