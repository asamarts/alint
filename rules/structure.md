---
title: 'Structure'
description: 'Rule reference: the structure family.'
sidebar:
  order: 9
---

### `max_directory_depth`

Tree depth from repo root may not exceed `max`. A shallow depth stops deeply-nested imports and keeps CI path globs sane.

```yaml
- id: shallow-tree
  kind: max_directory_depth
  paths: "**"
  max: 6
  level: warning
```

### `max_files_per_directory`

Per-directory fanout may not exceed `max`. Useful for vendor directories that accidentally grow to thousands of entries.

### `no_empty_files`

Flag zero-byte files. Fixable via `file_remove`.

```yaml
- id: no-empty
  kind: no_empty_files
  paths: "**"
  level: warning
  fix:
    file_remove: {}
```

---

