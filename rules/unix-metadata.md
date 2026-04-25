---
title: 'Unix metadata'
description: 'Rule reference: the unix metadata family.'
sidebar:
  order: 11
---

All rules in this family are no-ops on Windows — the +x bit and symlinks don't have a portable cross-platform story, so configs stay identical either way.

### `no_symlinks`

Flag tracked paths that are symbolic links. Symlinks are a portability footgun: Windows NTFS needs admin rights to create them, git-for-Windows can silently flatten them, CI runners vary.

```yaml
- id: no-symlinks
  kind: no_symlinks
  paths: "**"
  level: warning
  fix:
    file_remove: {}   # unlinks the symlink; target is untouched
```

### `executable_bit`

Assert every file in scope either has the `+x` bit set (`require: true`) or does not (`require: false`).

```yaml
- id: ci-scripts-exec
  kind: executable_bit
  paths: "ci/**/*.sh"
  require: true
  level: warning
```

No fix op — chmod auto-apply is deferred.

### `executable_has_shebang`

Every file with `+x` set must begin with `#!`. Catches plain text files accidentally marked executable.

### `shebang_has_executable`

Every file starting with `#!` must have `+x` set. Catches scripts that got their `+x` bit stripped by `git add --chmod=-x`, a tar round-trip, or a `cp` across filesystems.

```yaml
- id: scripts-wired
  kind: shebang_has_executable
  paths: "ci/**/*.sh"
  level: warning
```

---

