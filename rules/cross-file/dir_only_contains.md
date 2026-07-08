---
title: 'dir_only_contains'
description: 'Every direct-child file of a directory matching select: must match at least one glob in allow:. alint dir_only_contains rule, cross-file family.'
sidebar:
  order: 14
categories: ['cross-file']
---

Every direct-child file of a directory matching `select:` must match at least one glob in `allow:`. Catches stray test data in `src/`.

```yaml
- id: src-only-rs
  kind: dir_only_contains
  select: "src/*"
  allow: ["*.rs", "README.md"]
  level: error
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `allow` | string or list of string | yes |  | Basename glob(s) accepted as direct children. Anything else is a violation. |
| `select` | string | yes |  | Glob selecting the directories to enumerate. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
