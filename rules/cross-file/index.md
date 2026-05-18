---
title: 'Cross-file'
description: 'Rule reference: the cross-file family.'
sidebar:
  order: 12
  label: 'Cross-file'
---

Rule kinds in the **Cross-file** family. Each entry below has its own page with options, an example, and any auto-fix support.

- [`pair`](/docs/rules/cross-file/pair/) — For every file matching `primary`, a file matching the `partner` template must exist.
- [`registry_paths_resolve`](/docs/rules/cross-file/registry_paths_resolve/) — A manifest file enumerates path entries; each must resolve to an on-disk artefact.
- [`cross_file_value_equals`](/docs/rules/cross-file/cross_file_value_equals/) — A value extracted from one authoritative `source` file must equal a value extracted from one or more `targets`.
- [`for_each_dir`](/docs/rules/cross-file/for_each_dir/) — For every matching directory / file, evaluate a nested `require:` block with the entry as context.
- [`for_each_file`](/docs/rules/cross-file/for_each_file/) — For every matching directory / file, evaluate a nested `require:` block with the entry as context.
- [`dir_contains`](/docs/rules/cross-file/dir_contains/) — Every directory matching `select:` must contain files matching every glob in `require:`.
- [`dir_only_contains`](/docs/rules/cross-file/dir_only_contains/) — Every direct-child file of a directory matching `select:` must match at least one glob in `allow:`.
- [`unique_by`](/docs/rules/cross-file/unique_by/) — No two files matching `paths` may share the value of `key` (a path template).
- [`every_matching_has`](/docs/rules/cross-file/every_matching_has/) — For every file or directory matching `select:`, every nested rule under `require:` must be satisfied.
