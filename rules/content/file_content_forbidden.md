---
title: 'file_content_forbidden'
description: 'alint rule kind `file_content_forbidden` (Content family).'
sidebar:
  order: 2
---

File contents must NOT match a regex.

```yaml
- id: no-dbg-macros
  kind: content_forbidden
  paths: "crates/**/src/**/*.rs"
  pattern: '\bdbg!\('
  level: warning
```

