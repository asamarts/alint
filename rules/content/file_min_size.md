---
title: 'file_min_size'
description: 'alint rule kind `file_min_size` (Content family).'
sidebar:
  order: 8
---

File must be at least `min_bytes` in size. Catches placeholder / stub files that pass existence checks but add no information (a 0-byte `LICENSE`, a `README.md` with only a title).

```yaml
- id: license-non-empty
  kind: min_size
  paths: ["LICENSE", "LICENSE.md", "LICENSE-APACHE", "LICENSE-MIT"]
  min_bytes: 200
  level: warning
```

