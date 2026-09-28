---
title: 'Fix operations'
description: 'alint concept: fix operations.'
---

Every `fix:` block uses one of these ops. See [ARCHITECTURE.md](design/ARCHITECTURE.md#fix-operations) for the full cross-reference of which op pairs with which rule kind.

**Path-only** (ignore `fix_size_limit`):

- `file_create: {content, path?, create_parents?}`
- `file_remove: {}`
- `file_rename: {}` (target derived from rule config)
- `dir_create: {}`
- `relocate: {}` (moves the file to the repo root)
- `chmod: {}` (direction from the rule)
- `git_untrack: {}` (`git rm --cached`; spawning)
- `command: {run, timeout?}` (runs a user command; spawning)

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
- `replace: {replacement}` (pattern from parent rule) — located, one splice per match
- `set_value: {}` (value from the rule's `equals:`) — for the `*_path_equals` kinds
- `remove_value: {}` (target from parent rule) — for the `*_path_absent` kinds
- `sync_from: {}` (source + relation from parent rule) — for `cross_file`
- `create_and_register: {content?, content_from?}` — for `cross_file` `relation: registered`
- `sort: {}` (markers / comparator / `unique` / `select` from parent rule) — for `ordered_block`
- `indent_style: {}` (style / width from parent rule) — for `indent_style` (`style: spaces` + `width` only)
- `insert_line: {}` (require lines / comparator from parent rule) — for a markerless `ordered_block` with `require:`
- `insert_header: {content?, content_from?}` — for `file_header`; inserts AFTER a leading BOM / shebang / `<?xml?>` (position-aware `file_prepend`)

`fix_size_limit` is a top-level config field:

<!-- alint:ignore-example -->
```yaml
version: 1
fix_size_limit: 1048576   # 1 MiB — the default; `null` disables
rules:
  - ...
```

Over-limit files report `Skipped` with a stderr warning rather than applying the fix.

---

