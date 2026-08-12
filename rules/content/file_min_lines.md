---
title: 'file_min_lines'
description: 'File must have at least min_lines lines (\n-terminated, with an unterminated trailing segment counting as one more, wc -l semantics).'
sidebar:
  order: 9
categories: ['content', 'structure']
---

File must have at least `min_lines` lines (`\n`-terminated, with an unterminated trailing segment counting as one more — `wc -l` semantics). Use for "README has more than a title and a TODO".

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `min_lines` | integer (>= 0) | yes |  | Minimum allowed line count. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A stub with too few lines

The rule fires on this repository:

```text
fine.md
stub.md
```

`fine.md`:

```markdown
# Project

A description.
Usage: run me.
```

`stub.md`:

```markdown
# Project
TODO
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: min-3-lines
    kind: file_min_lines
    paths: "**/*.md"
    min_lines: 3
    level: info
```

### A file that clears the line floor

This repository is compliant:

```text
README.md
```

`README.md`:

```markdown
# Demo

A description.
Install: `cargo install foo`.
Run: `foo`.
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: min-4-lines
    kind: file_min_lines
    paths: "**/*.md"
    min_lines: 4
    level: warning
```

