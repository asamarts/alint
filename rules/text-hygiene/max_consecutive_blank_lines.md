---
title: 'max_consecutive_blank_lines'
description: 'Cap runs of blank lines to max. alint max_consecutive_blank_lines rule, text hygiene family.'
sidebar:
  order: 6
categories: ['text-hygiene']
---

Cap runs of blank lines to `max`. A blank line is empty or whitespace-only.

---

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max` | integer (>= 0) | yes |  | Maximum number of blank lines allowed in a row. `0` means no blank lines at all. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A document with too many blank lines in a row

The rule fires on this repository:

```text
docs/
docs/clean.md
docs/gappy.md
```

`docs/clean.md`:

```markdown
a

b
```

`docs/gappy.md`:

```markdown
a



b
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-dbl-blank
    kind: max_consecutive_blank_lines
    paths: "docs/**/*.md"
    max: 1
    level: warning
```

`alint check` reports:

```ansi
[2m--- docs/gappy.md --------------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2mno-dbl-blank[0m
              [2m3:1[0m  more than 1 consecutive blank line(s)

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### Blank-line runs stay within the limit

This repository is compliant:

```text
docs/
docs/a.md
docs/b.md
```

`docs/a.md`:

```markdown
# title

first paragraph

second paragraph
```

`docs/b.md`:

```markdown
x

y
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-dbl-blank
    kind: max_consecutive_blank_lines
    paths: "docs/**/*.md"
    max: 1
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

