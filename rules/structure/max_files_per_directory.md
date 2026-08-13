---
title: 'max_files_per_directory'
description: 'Per-directory fanout may not exceed max_files. alint max_files_per_directory rule, structure family.'
sidebar:
  order: 2
categories: ['structure']
---

Per-directory fanout may not exceed `max_files`. Useful for vendor directories that accidentally grow to thousands of entries.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max_files` | integer (>= 1) | yes |  | Maximum number of in-scope files allowed as immediate children of any one directory (non-recursive). |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A directory holding more files than the cap allows

The rule fires on this repository:

```text
other/
other/single.md
packages/
packages/a.md
packages/b.md
packages/c.md
packages/d.md
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: cap
    kind: max_files_per_directory
    paths: "**/*.md"
    max_files: 2
    level: warning
```

`alint check` reports:

```ansi
[2m--- packages -------------------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2mcap[0m
              packages has 4 files; max is 2

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### Every directory stays under the file-count cap

This repository is compliant:

```text
docs/
docs/a.md
docs/b.md
src/
src/c.md
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: cap
    kind: max_files_per_directory
    paths: "**/*.md"
    max_files: 5
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

