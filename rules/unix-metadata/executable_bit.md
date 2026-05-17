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

