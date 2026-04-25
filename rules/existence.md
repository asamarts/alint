---
title: 'Existence'
description: 'Rule reference: the existence family.'
sidebar:
  order: 3
---

### `file_exists`

Every glob match in `paths` must correspond to a real file. Use an array to accept any of several names.

```yaml
- id: readme-exists
  kind: file_exists
  paths: ["README.md", "README", "README.rst"]
  root_only: true
  level: error
```

Fix: `file_create` — write a declared `content`. With an array of `paths`, the fix creates the first entry.

### `file_absent`

No file matching `paths` may exist. The inverse of `file_exists`.

```yaml
- id: no-backup-files
  kind: file_absent
  paths: "**/*.bak"
  level: warning
```

Fix: `file_remove` — delete every violating file.

### `dir_exists` / `dir_absent`

Directory-flavored counterparts of the above.

---

