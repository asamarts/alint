---
title: 'file_header'
description: 'alint rule kind `file_header` (Content family).'
sidebar:
  order: 3
---

The first N lines must match a regex (line-oriented). For a byte-level prefix check, prefer `file_starts_with`.

```yaml
- id: spdx-header
  kind: header
  paths: "src/**/*.rs"
  pattern: "^// SPDX-License-Identifier: MIT"
  level: error
```

Fix: `file_prepend` — inject declared content at the top (preserves UTF-8 BOM).

