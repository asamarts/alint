---
title: 'file_is_text'
description: 'Content is detected as text (magic bytes + UTF-8 validity check), fails on binary files matched by paths. alint file_is_text rule, content family.'
sidebar:
  order: 13
---

Content is detected as text (magic bytes + UTF-8 validity check) — fails on binary files matched by `paths`.

```yaml
- id: configs-are-text
  kind: file_is_text
  paths: ".github/**/*.{yml,yaml}"
  level: error
```

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
