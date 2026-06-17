---
title: 'executable_bit'
description: 'Assert every file in scope either has the +x bit set (require: true) or does not (require: false). alint executable_bit rule, unix metadata family.'
sidebar:
  order: 2
---

Assert every file in scope either has the `+x` bit set (`require: true`) or does not (`require: false`).

```yaml
- id: ci-scripts-exec
  kind: executable_bit
  paths: "ci/**/*.sh"
  require: true
  level: warning
```

No fix op — chmod auto-apply is deferred.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `require` | boolean | yes |  | `true` → +x must be set; `false` → +x must NOT be set. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
