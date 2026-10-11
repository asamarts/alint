---
title: 'Unix metadata'
description: 'Rule reference: the unix metadata family.'
sidebar:
  order: 10
  label: 'Unix metadata'
---

The rules that read the `+x` bit (`executable_bit`, `executable_has_shebang`, `shebang_has_executable`) are no-ops on Windows, which has no executable bit, so the same config runs unchanged on every platform. `no_symlinks` and `file_shebang` check every platform.

Rule kinds in the **Unix metadata** family. Each rule below links to its own page with options, an example, and any auto-fix support.

| Rule | Description |
| --- | --- |
| [`file_shebang`](/docs/rules/content/file_shebang/) | First line of each file in scope must match the `shebang` regex. |
| [`no_symlinks`](/docs/rules/unix-metadata/no_symlinks/) | Flag tracked paths that are symbolic links. |
| [`executable_bit`](/docs/rules/unix-metadata/executable_bit/) | Assert every file in scope either has the `+x` bit set (`require: true`) or does not (`require: false`). |
| [`executable_has_shebang`](/docs/rules/unix-metadata/executable_has_shebang/) | Every file with `+x` set must begin with `#!`. |
| [`shebang_has_executable`](/docs/rules/unix-metadata/shebang_has_executable/) | Every file starting with `#!` must have `+x` set. |
