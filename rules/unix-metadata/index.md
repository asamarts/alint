---
title: 'Unix metadata'
description: 'Rule reference: the unix metadata family.'
sidebar:
  order: 9
  label: 'Unix metadata'
---

Rule kinds in the **Unix metadata** family. Each entry below has its own page with options, an example, and any auto-fix support.

- [`no_symlinks`](/docs/rules/unix-metadata/no_symlinks/) — Flag tracked paths that are symbolic links.
- [`executable_bit`](/docs/rules/unix-metadata/executable_bit/) — Assert every file in scope either has the `+x` bit set (`require: true`) or does not (`require: false`).
- [`executable_has_shebang`](/docs/rules/unix-metadata/executable_has_shebang/) — Every file with `+x` set must begin with `#!`.
- [`shebang_has_executable`](/docs/rules/unix-metadata/shebang_has_executable/) — Every file starting with `#!` must have `+x` set.
