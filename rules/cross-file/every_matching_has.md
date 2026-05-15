---
title: 'every_matching_has'
description: 'alint rule kind `every_matching_has` (Cross-file family).'
sidebar:
  order: 7
---

For every file or directory matching `select:`, every nested rule under `require:` must be satisfied. Lightweight sibling of `pair` that iterates both file and directory entries.

```yaml
- id: every-pkg-has-readme
  kind: every_matching_has
  select: "packages/*"
  require:
    - kind: file_exists
      paths: "{path}/README.md"
  level: error
```

---

