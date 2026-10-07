---
title: 'Encoding'
description: 'Rule reference: the encoding family.'
sidebar:
  order: 7
  label: 'Encoding'
---

Rule kinds in the **Encoding** family. Each rule below links to its own page with options, an example, and any auto-fix support.

| Rule | Description |
| --- | --- |
| [`file_is_text`](/docs/rules/content/file_is_text/) | Content is detected as text (magic bytes + UTF-8 validity check), fails on binary files matched by `paths`. |
| [`file_is_ascii`](/docs/rules/content/file_is_ascii/) | Every byte in the file must be < 0x80 (pure ASCII), except codepoints listed in `allow:`. |
| [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/) | Flag Trojan-Source bidi override characters (U+202A,202E, U+2066,2069). |
| [`no_zero_width_chars`](/docs/rules/security-unicode-sanity/no_zero_width_chars/) | Flag body-internal zero-width characters (U+200B, U+200C, U+200D, and non-leading U+FEFF). |
| [`no_bom`](/docs/rules/encoding/no_bom/) | Flag a leading UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE byte-order mark. |
