---
title: 'Cross-file'
description: 'Rule reference: the cross-file family.'
sidebar:
  order: 11
  label: 'Cross-file'
---

Rule kinds in the **Cross-file** family. Each entry below has its own page with options, an example, and any auto-fix support.

- [`pair`](/docs/rules/cross-file/pair/) — For every file matching `primary`, a file matching the `partner` template must exist.
- [`for_each_dir`](/docs/rules/cross-file/for_each_dir/) — For every matching directory / file, evaluate a nested `require:` block with the entry as context.
- [`for_each_file`](/docs/rules/cross-file/for_each_file/) — For every matching directory / file, evaluate a nested `require:` block with the entry as context.
- [`dir_contains`](/docs/rules/cross-file/dir_contains/) — Every directory matching `paths` must contain files matching `require:`.
- [`dir_only_contains`](/docs/rules/cross-file/dir_only_contains/) — Every directory matching `paths` may contain only files matching `allow:`.
- [`unique_by`](/docs/rules/cross-file/unique_by/) — No two files matching `paths` may share the value of `key` (a path template).
- [`every_matching_has`](/docs/rules/cross-file/every_matching_has/) — For every file matching `paths`, at least one of `require:` must also exist (at a template-derived location).
