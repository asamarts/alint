---
title: 'file_max_size'
description: 'File must be at most max_bytes in size. alint file_max_size rule, content family.'
sidebar:
  order: 7
categories: ['content', 'structure']
---

File must be at most `max_bytes` in size. Catches accidental large-blob commits.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max_bytes` | integer (>= 0) | yes |  | Maximum allowed file size in bytes. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A file over the byte limit

The rule fires on this repository:

```text
big.txt
tiny.txt
```

`big.txt`:

```text
this content is easily over ten bytes long
```

`tiny.txt`:

```text
short
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: under-10-bytes
    kind: file_max_size
    paths: "**/*.txt"
    max_bytes: 10
    level: warning
```

`alint check` reports:

```ansi
[2m--- big.txt --------------------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2munder-10-bytes[0m
              file exceeds 10 byte(s) (actual: 42)

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### Every file is under the byte limit

This repository is compliant:

```text
also_tiny.txt
tiny.txt
```

`also_tiny.txt`:

```text
hi
```

`tiny.txt`:

```text
x
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: under-10-bytes
    kind: file_max_size
    paths: "**/*.txt"
    max_bytes: 10
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

