---
title: 'file_footer'
description: 'Last lines lines of each file in scope must match a regex. alint file_footer rule, content family.'
sidebar:
  order: 11
---

Last `lines` lines of each file in scope must match a regex. Mirror of `file_header` anchored at the end of the file. Use for license footers, signed-off-by trailers, generated-file sentinels.

```yaml
- id: license-footer
  kind: footer
  paths: "src/**/*.rs"
  pattern: "Licensed under the Apache License, Version 2\\.0"
  lines: 3
  level: error
```

Fix: `file_append` — append a declared `content`. With no fix declared, violations are unfixable.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `lines` | integer (>= 1) |  | `20` | Number of trailing lines to consider. |
| `pattern` | string | yes |  | Rust regex. The last `lines` lines of each file must match. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
