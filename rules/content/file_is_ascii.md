---
title: 'file_is_ascii'
description: 'Every byte in the file must be < 0x80 (pure ASCII), except codepoints listed in allow:. alint file_is_ascii rule, content family.'
sidebar:
  order: 14
categories: ['content', 'encoding', 'security-unicode-sanity']
---

Every byte in the file must be < 0x80 (pure ASCII), except codepoints listed in `allow:`. Strict variant of `is_text` for configs that must round-trip through strictly-ASCII tools. `allow:` exempts specific non-ASCII codepoints — each entry a single character (`"ö"`), a `U+XXXX` codepoint, or a `U+XXXX-U+YYYY` inclusive range (curl keeps its source ASCII but allows `ö` in "Björn"; the recurring need across llvm / vscode / elixir). With `allow:` the file is decoded as UTF-8 and checked per character; without it, the strict byte-level fast path is used.

```yaml
- id: source-ascii-but-allow-accents
  kind: file_is_ascii
  paths: "src/**"
  allow: ["ö", "U+00E9", "U+2010-U+2015"]   # ö, é, and the dash block
  level: error
```

---

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `allow` | list of string |  | `[]` | Permitted non-ASCII codepoints - each a single character (e.g. "o-umlaut"), a `U+XXXX` codepoint, or a `U+XXXX-U+YYYY` inclusive range. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
