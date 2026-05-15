---
title: 'file_max_lines'
description: 'alint rule kind `file_max_lines` (Content family).'
sidebar:
  order: 10
---

File must have at most `max_lines` lines, using the same accounting as `file_min_lines`. Catches the everything-module anti-pattern — a `lib.rs` / `index.ts` / `helpers.py` that grew unbounded.

```yaml
- id: cap-source-file-size
  kind: max_lines
  paths: "src/**/*.rs"
  max_lines: 800
  level: warning
```

