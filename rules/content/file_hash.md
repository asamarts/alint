---
title: 'file_hash'
description: 'alint rule kind `file_hash` (Content family).'
sidebar:
  order: 6
---

Content SHA-256 must equal the expected digest. Rules-as-tripwire for generated / vendored files that should never drift.

```yaml
- id: schema-frozen
  kind: file_hash
  paths: "schemas/v1/config.json"
  sha256: "b7d0...c2e1"   # 64 hex chars
  level: error
```

