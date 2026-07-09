---
title: 'Structure'
description: 'Rule reference: the structure family.'
sidebar:
  order: 8
  label: 'Structure'
---

Rule kinds in the **Structure** family. Each rule below links to its own page with options, an example, and any auto-fix support.

| Rule | Description |
| --- | --- |
| [`file_max_size`](/docs/rules/content/file_max_size/) | File must be at most `max_bytes` in size. |
| [`file_min_size`](/docs/rules/content/file_min_size/) | File must be at least `min_bytes` in size. |
| [`file_min_lines`](/docs/rules/content/file_min_lines/) | File must have at least `min_lines` lines (`\n`-terminated, with an unterminated trailing segment counting as one more, `wc -l` semantics). |
| [`file_max_lines`](/docs/rules/content/file_max_lines/) | File must have at most `max_lines` lines, using the same accounting as `file_min_lines`. |
| [`max_directory_depth`](/docs/rules/structure/max_directory_depth/) | Tree depth from repo root may not exceed `max_depth`. |
| [`max_files_per_directory`](/docs/rules/structure/max_files_per_directory/) | Per-directory fanout may not exceed `max_files`. |
| [`no_empty_files`](/docs/rules/structure/no_empty_files/) | Flag zero-byte files. |
| [`dir_contains`](/docs/rules/cross-file/dir_contains/) | Every directory matching `select:` must contain files matching every glob in `require:`. |
| [`dir_only_contains`](/docs/rules/cross-file/dir_only_contains/) | Every direct-child file of a directory matching `select:` must match at least one glob in `allow:`. |
