---
title: 'pair'
description: 'alint rule kind `pair` (Cross-file family).'
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

