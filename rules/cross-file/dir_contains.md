---
title: 'dir_contains'
description: 'alint rule kind `dir_contains` (Cross-file family).'
sidebar:
  order: 4
---

Every directory matching `select:` must contain files matching every glob in `require:`. Sugar for a common `for_each_dir` shape.

```yaml
- id: packages-have-readme-and-license
  kind: dir_contains
  select: "packages/*"
  require: ["README.md", "LICENSE*"]
  level: error
```

