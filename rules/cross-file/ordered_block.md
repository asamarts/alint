---
title: 'ordered_block'
description: 'The lines between a start / end marker pair must stay sorted (and, with unique: true, free of duplicates) under comparator (lexical /.'
sidebar:
  order: 6
---

The lines between a `start` / `end` marker pair must stay sorted (and, with `unique: true`, free of duplicates) under `comparator` (`lexical` / `lexical-ci` / `numeric`). The generic form of per-project keep-sorted scripts (protobuf `failure_lists`, sorted `.gitignore` / `CODEOWNERS` / dependency lists). Per-file: a file with no `start` marker is silently fine; markers match the trimmed line; blank lines inside a block are ignored; one violation per out-of-order block. An optional `select:` regex restricts the sortable entries to lines matching it — other lines inside the block (comments, group headers) pass through untouched (the sectioned / keep-sorted-subset shape).

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

