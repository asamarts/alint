---
title: 'shebang_has_executable'
description: 'Every file starting with #! must have +x set. alint shebang_has_executable rule, unix metadata family.'
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

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
