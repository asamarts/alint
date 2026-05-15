---
title: 'max_consecutive_blank_lines'
description: 'alint rule kind `max_consecutive_blank_lines` (Text hygiene family).'
sidebar:
  order: 6
---

Cap runs of blank lines to `max`. A blank line is empty or whitespace-only.

```yaml
- id: md-tidy
  kind: max_consecutive_blank_lines
  paths: "**/*.md"
  max: 1
  level: warning
  fix:
    file_collapse_blank_lines: {}
```

---

