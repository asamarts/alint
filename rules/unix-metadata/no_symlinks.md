---
title: 'no_symlinks'
description: 'alint rule kind `no_symlinks` (Unix metadata family).'
sidebar:
  order: 1
---

Flag tracked paths that are symbolic links. Symlinks are a portability footgun: Windows NTFS needs admin rights to create them, git-for-Windows can silently flatten them, CI runners vary.

```yaml
- id: no-symlinks
  kind: no_symlinks
  paths: "**"
  level: warning
  fix:
    file_remove: {}   # unlinks the symlink; target is untouched
```

