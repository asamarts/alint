---
title: 'no_bidi_controls'
description: 'alint rule kind `no_bidi_controls` (Security / Unicode sanity family).'
sidebar:
  order: 2
---

Flag Trojan-Source bidi override characters (U+202A–202E, U+2066–2069). Defense against [CVE-2021-42574](https://trojansource.codes/).

```yaml
- id: no-bidi
  kind: no_bidi_controls
  paths: "crates/**/src/**/*.rs"
  level: error
  fix:
    file_strip_bidi: {}
```

