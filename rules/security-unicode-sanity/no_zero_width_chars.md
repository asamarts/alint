---
title: 'no_zero_width_chars'
description: 'alint rule kind `no_zero_width_chars` (Security / Unicode sanity family).'
sidebar:
  order: 3
---

Flag body-internal zero-width characters (U+200B, U+200C, U+200D, and non-leading U+FEFF). A leading U+FEFF is `no_bom`'s concern.

```yaml
- id: no-zwsp
  kind: no_zero_width_chars
  paths: "crates/**/src/**/*.rs"
  level: error
  fix:
    file_strip_zero_width: {}
```

---

