---
title: 'file_hash'
description: 'Content SHA-256 must equal the expected digest. alint file_hash rule, content family.'
sidebar:
  order: 6
categories: ['content', 'security-unicode-sanity']
---

Content SHA-256 must equal the expected digest. Rules-as-tripwire for generated / vendored files that should never drift.

```yaml
- id: schema-frozen
  kind: file_hash
  paths: "schemas/v1/config.json"
  sha256: "0000000000000000000000000000000000000000000000000000000000000000"   # 64 hex chars
  level: error
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `sha256` | string | yes |  | Expected SHA-256 in lowercase hex (64 chars). Accepting uppercase and the `sha256:` prefix keeps the field forgiving. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
