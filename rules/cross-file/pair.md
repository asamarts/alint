---
title: 'pair'
description: 'For every file matching primary, a file matching the partner template must exist. alint pair rule, cross-file family.'
sidebar:
  order: 1
---

For every file matching `primary`, a file matching the `partner` template must exist.

```yaml
- id: every-impl-has-test
  kind: pair
  primary: "src/**/*.rs"
  partner: "tests/{stem}.test.rs"
  level: warning
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `partner` | string | yes |  | Path template resolved per primary match. Example: "{dir}/{stem}.h". |
| `primary` | string | yes |  | Glob selecting the primary files. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
