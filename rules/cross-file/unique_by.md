---
title: 'unique_by'
description: 'No two files matching select may share the value of key (a path template; tokens {path}/{dir}/{basename}/{stem}/{ext}/{parent_name}).'
sidebar:
  order: 15
---

No two files matching `select` may share the value of `key` (a path template; tokens `{path}`/`{dir}`/`{basename}`/`{stem}`/`{ext}`/`{parent_name}`). Catches basename collisions across subdirectories. With `case_insensitive: true` the key is folded to lowercase before grouping, so `README.md` and `readme.md` collide — the case-insensitive-filesystem hazard (Windows / macOS).

```yaml
- id: unique-basenames
  kind: unique_by
  paths: "src/**/*.rs"
  key: "{stem}"
  level: warning
```

