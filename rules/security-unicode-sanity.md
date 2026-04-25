---
title: 'Security / Unicode sanity'
description: 'Rule reference: the security / unicode sanity family.'
sidebar:
  order: 7
---

### `no_merge_conflict_markers`

Flag `<<<<<<< `, `=======`, `>>>>>>> ` markers at the start of a line — almost always left over from an unresolved merge.

```yaml
- id: no-conflicts
  kind: no_merge_conflict_markers
  paths: "**"
  level: error
```

### `no_bidi_controls`

Flag Trojan-Source bidi override characters (U+202A–202E, U+2066–2069). Defense against [CVE-2021-42574](https://trojansource.codes/).

```yaml
- id: no-bidi
  kind: no_bidi_controls
  paths: "crates/**/src/**/*.rs"
  level: error
  fix:
    file_strip_bidi: {}
```

### `no_zero_width_chars`

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

