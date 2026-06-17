---
title: 'indent_style'
description: 'Every non-blank line indents with the configured style (tabs or spaces). alint indent_style rule, text hygiene family.'
sidebar:
  order: 5
---

Every non-blank line indents with the configured `style` (`tabs` or `spaces`). When `style: spaces`, optional `width` enforces a multiple.

```yaml
- id: yaml-2sp
  kind: indent_style
  paths: "**/*.yml"
  style: spaces
  width: 2
  level: warning
```

Check-only: tab-width-aware reindentation is language-specific. Pair with your editor's "reindent on save" for remediation.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `style` | one of `tabs` \| `spaces` | yes |  | Required indentation style: `tabs` rejects any leading space; `spaces` rejects any leading tab. |
| `width` | integer (>= 1) |  | `null` | When `style: spaces`, the leading-space count on every non-blank line must be a multiple of this. Ignored for `style: tabs`. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
