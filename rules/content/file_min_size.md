---
title: 'file_min_size'
description: 'File must be at least min_bytes in size. alint file_min_size rule, content family.'
sidebar:
  order: 8
categories: ['content', 'structure']
---

File must be at least `min_bytes` in size. Catches placeholder / stub files that pass existence checks but add no information (a 0-byte `LICENSE`, a `README.md` with only a title).

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `min_bytes` | integer (>= 0) | yes |  | Minimum allowed file size in bytes. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A stub file below the byte floor

The rule fires on this repository:

```text
healthy.md
tiny.txt
```

`healthy.md`:

```markdown
# Healthy doc

This document has more than fifty bytes of content, well
above the minimum. A short README reads like a stub.
```

`tiny.txt`:

```text
too short
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: min-50
    kind: file_min_size
    paths: ["**/*.txt", "**/*.md"]
    min_bytes: 50
    level: warning
```

`alint check` reports:

```ansi
[2m--- tiny.txt -------------------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2mmin-50[0m
              file below 50 byte(s) (actual: 10)

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### A file that clears the byte floor

This repository is compliant:

```text
README.md
```

`README.md`:

```markdown
# demo

A short but non-stub README.
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: min-10
    kind: file_min_size
    paths: "**/*.md"
    min_bytes: 10
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

