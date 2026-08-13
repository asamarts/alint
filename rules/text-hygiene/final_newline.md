---
title: 'final_newline'
description: 'File must end with a single \n. alint final_newline rule, text hygiene family.'
sidebar:
  order: 2
categories: ['text-hygiene']
---

File must end with a single `\n`. Fixable via `file_append_final_newline`.

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A Markdown file missing its trailing newline

The rule fires on this repository:

```text
docs/
docs/clean.md
docs/missing.md
```

`docs/clean.md`:

```markdown
# Hi
```

`docs/missing.md`:

```markdown
# No newline
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: eof
    kind: final_newline
    paths: "docs/**/*.md"
    level: error
```

`alint check` reports:

```ansi
[2m--- docs/missing.md ------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2meof[0m
              file does not end with a newline

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every file ends with a trailing newline

This repository is compliant:

```text
docs/
docs/a.md
docs/b.md
docs/empty.md
```

`docs/a.md`:

```markdown
# Hi
```

`docs/b.md`:

```markdown
Hello
world
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: eof
    kind: final_newline
    paths: "docs/**/*.md"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

