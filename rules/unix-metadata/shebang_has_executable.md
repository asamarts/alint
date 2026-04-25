---
title: 'shebang_has_executable'
description: 'alint rule kind `shebang_has_executable` (Unix metadata family).'
sidebar:
  order: 4
---

Every file starting with `#!` must have `+x` set. Catches scripts that got their `+x` bit stripped by `git add --chmod=-x`, a tar round-trip, or a `cp` across filesystems.

```yaml
- id: scripts-wired
  kind: shebang_has_executable
  paths: "ci/**/*.sh"
  level: warning
```

---

