---
title: 'dir_exists'
description: 'alint rule kind `dir_exists` (Existence family).'
sidebar:
  order: 3
---

Directory counterpart of `file_exists`. Every match must correspond to a real directory in the walked tree.

```yaml
- id: docs-dir-exists
  kind: dir_exists
  paths: "docs"
  root_only: true
  level: error
```

