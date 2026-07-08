---
title: 'no_zero_width_chars'
description: 'Flag body-internal zero-width characters (U+200B, U+200C, U+200D, and non-leading U+FEFF). alint no_zero_width_chars rule, security / unicode sanity family.'
sidebar:
  order: 3
categories: ['security-unicode-sanity']
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

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
