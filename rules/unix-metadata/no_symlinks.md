---
title: 'no_symlinks'
description: 'Flag tracked paths that are symbolic links. alint no_symlinks rule, unix metadata family.'
sidebar:
  order: 1
categories: ['unix-metadata']
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

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
