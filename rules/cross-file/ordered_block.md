---
title: 'ordered_block'
description: 'The lines between a start / end marker pair must stay sorted (and, with unique: true, free of duplicates) under comparator (lexical /.'
sidebar:
  order: 6
categories: ['cross-file', 'text-hygiene']
---

The lines between a `start` / `end` marker pair must stay sorted (and, with `unique: true`, free of duplicates) under `comparator` (`lexical` / `lexical-ci` / `numeric`). **Both markers are optional**: omit `end` to sort from `start` to EOF, omit both to sort the whole file (the markerless "this file is one sorted list" form — dictionaries, allow-lists, a fully-sorted `CODEOWNERS`). The generic form of per-project keep-sorted scripts (protobuf `failure_lists`, sorted `.gitignore` / `CODEOWNERS` / dependency lists). Per-file: with markers, a file with no `start` marker is silently fine; markers match the trimmed line; blank lines inside a block are ignored; one violation per out-of-order block; a fully-delimited block that never sees its `end` is reported `unclosed` (a block with an absent `end` runs to EOF by design). An optional `select:` regex restricts the sortable entries to lines matching it — other lines inside the block (comments, group headers) pass through untouched (the sectioned / keep-sorted-subset shape).

```yaml
- id: keep-sorted
  kind: ordered_block
  paths: ["**/.gitignore", "CODEOWNERS"]
  start: "# keep-sorted start"
  end: "# keep-sorted end"
  comparator: lexical
  unique: false
  select: '^\s*require '   # sort only the `require '…'` lines
  level: warning
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `comparator` | one of `lexical` \| `lexical-ci` \| `numeric` |  | `lexical` | Comparator used to order entries: lexical (default), lexical-ci, or numeric. |
| `end` | string |  | `null` | Marker line closing a block. Optional - omit to run the block to EOF. |
| `select` | string |  | `null` | Regex; when set, only lines inside a block matching it are sortable entries (others, such as comments or group headers, pass through). The sectioned / keep-sorted-subset shape. |
| `start` | string |  | `null` | Marker line opening a block (matched on the trimmed line). Optional - omit to anchor the block at the start of the file. |
| `unique` | boolean |  | `false` | When true, also forbid duplicate (equal) entries within a block. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
