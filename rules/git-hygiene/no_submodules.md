---
title: 'no_submodules'
description: 'alint rule kind `no_submodules` (Git hygiene family).'
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

