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

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `case_insensitive` | boolean |  | `false` | Fold the key to lowercase before grouping, so keys that collide only under case-folding count as duplicates — the case-insensitive-filesystem hazard (Windows / macOS). |
| `key` | string |  | `{basename}` | Path-template producing a key per matched file. Default: {basename}. |
| `select` | string | yes |  | Glob selecting the files to deduplicate. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
