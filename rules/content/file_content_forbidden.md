---
title: 'file_content_forbidden'
description: 'File contents must NOT match a regex. alint file_content_forbidden rule, content family.'
sidebar:
  order: 2
categories: ['content']
---

File contents must NOT match a regex.

```yaml
- id: no-dbg-macros
  kind: content_forbidden
  paths: "crates/**/src/**/*.rs"
  pattern: '\bdbg!\('
  level: warning
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `pattern` | string | yes |  | Rust regex. File contents must NOT match. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
