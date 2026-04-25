---
title: 'Security / Unicode sanity'
description: 'Rule reference: the security / unicode sanity family.'
sidebar:
  order: 5
  label: 'Security / Unicode sanity'
---

Rule kinds in the **Security / Unicode sanity** family. Each entry below has its own page with options, an example, and any auto-fix support.

- [`no_merge_conflict_markers`](/docs/rules/security-unicode-sanity/no_merge_conflict_markers/) — Flag `<<<<<<< `, `=======`, `>>>>>>> ` markers at the start of a line — almost always left over from an unresolved merge.
- [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/) — Flag Trojan-Source bidi override characters (U+202A–202E, U+2066–2069).
- [`no_zero_width_chars`](/docs/rules/security-unicode-sanity/no_zero_width_chars/) — Flag body-internal zero-width characters (U+200B, U+200C, U+200D, and non-leading U+FEFF).
