---
title: 'dir_contains'
description: 'Every directory matching select: must contain files matching every glob in require:. alint dir_contains rule, cross-file family.'
sidebar:
  order: 13
categories: ['cross-file', 'structure']
---

Every directory matching `select:` must contain files matching every glob in `require:`. Sugar for a common `for_each_dir` shape.

```yaml
- id: packages-have-readme-and-license
  kind: dir_contains
  select: "packages/*"
  require: ["README.md", "LICENSE*"]
  level: error
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `require` | string or list of string | yes |  | Basename glob(s) — every dir matching `select` must have at least one child matching each. |
| `select` | string | yes |  | Glob selecting the directories to check. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
