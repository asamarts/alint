---
title: 'indent_style'
description: 'alint rule kind `indent_style` (Text hygiene family).'
sidebar:
  order: 5
---

Every non-blank line indents with the configured `style` (`tabs` or `spaces`). When `style: spaces`, optional `width` enforces a multiple.

```yaml
- id: yaml-2sp
  kind: indent_style
  paths: "**/*.yml"
  style: spaces
  width: 2
  level: warning
```

Check-only: tab-width-aware reindentation is language-specific. Pair with your editor's "reindent on save" for remediation.

