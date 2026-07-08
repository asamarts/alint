---
title: 'every_matching_has'
description: 'For every file or directory matching select:, every nested rule under require: must be satisfied. alint every_matching_has rule, cross-file family.'
sidebar:
  order: 16
categories: ['cross-file']
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

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `require` | list of nested rule | yes |  | One or more nested rules that every file or directory matching `select` must satisfy. |
| `select` | string or list of string | yes |  | Glob(s) selecting the files/dirs to iterate — a single glob, or a list with `!`-prefixed excludes (e.g. ["packages/*", "!packages/internal"]). |
| `when_iter` | string |  |  | Per-iteration `when:` filter — see rule_for_each_dir.when_iter. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
