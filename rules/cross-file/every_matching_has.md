---
title: 'every_matching_has'
description: 'For every file or directory matching select:, every nested rule under require: must be satisfied. alint every_matching_has rule, cross-file family.'
sidebar:
  order: 16
---

For every file or directory matching `select:`, every nested rule under `require:` must be satisfied. Lightweight sibling of `pair` that iterates both file and directory entries. `select:` is a single glob or a list with `!`-prefixed excludes (e.g. `["packages/*", "!packages/internal"]`).

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

