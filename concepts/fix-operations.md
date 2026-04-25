---
title: 'Fix operations'
description: 'alint concept: fix operations.'
---

Every `fix:` block uses one of these ops. See [ARCHITECTURE.md](design/ARCHITECTURE.md#fix-operations) for the full cross-reference of which op pairs with which rule kind.

**Path-only** (ignore `fix_size_limit`):

- `file_create: {content, path?, create_parents?}`
- `file_remove: {}`
- `file_rename: {}` (target derived from rule config)

**Content-editing** (skipped on files over `fix_size_limit`; default 1 MiB, `null` disables the cap):

- `file_prepend: {content}`
- `file_append: {content}`
- `file_trim_trailing_whitespace: {}`
- `file_append_final_newline: {}`
- `file_normalize_line_endings: {}` (target read from parent rule)
- `file_strip_bidi: {}`
- `file_strip_zero_width: {}`
- `file_strip_bom: {}`
- `file_collapse_blank_lines: {}` (max read from parent rule)

`fix_size_limit` is a top-level config field:

```yaml
version: 1
fix_size_limit: 1048576   # 1 MiB — the default; `null` disables
rules:
  - ...
```

Over-limit files report `Skipped` with a stderr warning rather than applying the fix.

---

