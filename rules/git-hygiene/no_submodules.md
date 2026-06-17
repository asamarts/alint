---
title: 'no_submodules'
description: 'Flag the presence of .gitmodules at the repo root, always, regardless of paths. alint no_submodules rule, git hygiene family.'
sidebar:
  order: 1
---

Flag the presence of `.gitmodules` at the repo root — always, regardless of `paths`. For general "file X must not exist" checks, use `file_absent`.

```yaml
- id: no-submods
  kind: no_submodules
  level: warning
  fix:
    file_remove: {}
```

Note the fix only deletes `.gitmodules`; `git submodule deinit` and cleaning `.git/modules/` are still on the user.

## Options

_This rule takes no kind-specific options._

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
