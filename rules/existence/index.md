---
title: 'Existence'
description: 'Rule reference: the existence family.'
sidebar:
  order: 1
  label: 'Existence'
---

Rule kinds in the **Existence** family. Each entry below has its own page with options, an example, and any auto-fix support.

- [`file_exists`](/docs/rules/existence/file_exists/) — Every glob match in `paths` must correspond to a real file.
- [`file_absent`](/docs/rules/existence/file_absent/) — No file matching `paths` may exist in the walked tree.
- [`dir_exists`](/docs/rules/existence/dir_exists/) — Directory counterpart of `file_exists`.
- [`dir_absent`](/docs/rules/existence/dir_absent/) — Directory counterpart of `file_absent`.
