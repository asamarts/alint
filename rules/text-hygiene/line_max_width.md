---
title: 'line_max_width'
description: 'alint rule kind `line_max_width` (Text hygiene family).'
sidebar:
  order: 4
---

Cap line length in characters (not bytes — code points). Optional `tab_width` for tab expansion.

```yaml
- id: docs-80-col
  kind: line_max_width
  paths: "docs/**/*.md"
  max: 80
  level: info
```

