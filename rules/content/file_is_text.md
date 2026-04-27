---
title: 'file_is_text'
description: 'alint rule kind `file_is_text` (Content family).'
sidebar:
  order: 20
---

Content is detected as text (magic bytes + UTF-8 validity check) — fails on binary files matched by `paths`.

```yaml
- id: configs-are-text
  kind: file_is_text
  paths: ".github/**/*.{yml,yaml}"
  level: error
```

