---
title: 'file_exists'
description: 'alint rule kind `file_exists` (Existence family).'
sidebar:
  order: 1
---

Every glob match in `paths` must correspond to a real file. Use an array to accept any of several names.

```yaml
- id: readme-exists
  kind: file_exists
  paths: ["README.md", "README", "README.rst"]
  root_only: true
  level: error
```

Fix: `file_create` — write a declared `content`. With an array of `paths`, the fix creates the first entry.

