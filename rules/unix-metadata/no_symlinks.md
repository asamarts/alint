---
title: 'no_symlinks'
description: 'Flag tracked paths that are symbolic links. alint no_symlinks rule, unix metadata family.'
sidebar:
  order: 1
categories: ['unix-metadata', 'portable-metadata', 'security-unicode-sanity']
---

Flag tracked paths that are symbolic links. Symlinks are a portability footgun: Windows NTFS needs admin rights to create them, git-for-Windows can silently flatten them, CI runners vary.

Caveat: a symlink whose target escapes the repository root (`link -> /etc`) is pruned by the walker *before* indexing, so it is **not** flagged — the rule reports in-tree symlinks (to files or directories), not escaping ones. The escaping symlink can't be read out-of-root either (path confinement blocks that), so this is a reporting gap, not a disclosure one; recording escaping symlinks safely is a tracked follow-up.

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
