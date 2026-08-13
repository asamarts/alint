---
title: 'no_case_conflicts'
description: 'Flag paths that differ only by case (e.g. alint no_case_conflicts rule, portable metadata family.'
sidebar:
  order: 1
categories: ['portable-metadata', 'naming']
---

Flag paths that differ only by case (e.g. `README.md` + `readme.md`). They can't coexist on macOS HFS+/APFS or Windows NTFS defaults, so a Linux-only dev committing both breaks checkouts for teammates.

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A repo containing both README.md and readme.md

The rule fires on this repository:

```text
README.md
other.txt
readme.md
```

`README.md`:

```markdown
# upper
```

`readme.md`:

```markdown
# lower
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-case
    kind: no_case_conflicts
    paths: "**"
    level: error
```

`alint check` reports:

```ansi
[2m--- README.md ------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-case[0m
              case-insensitive collision: README.md (collides with: readme.md)

[2m--- readme.md ------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-case[0m
              case-insensitive collision: readme.md (collides with: README.md)

[2mSummary (2 violations):[0m
  [1m[31mx 2 errors[0m
  0 passing [2m*[0m 1 failing
```

### Filenames that stay unique when case is ignored

This repository is compliant:

```text
CONTRIBUTING.md
README.md
src/
src/lib.rs
src/util.rs
```

`CONTRIBUTING.md`:

```markdown
# guide
```

`README.md`:

```markdown
# hi
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-case
    kind: no_case_conflicts
    paths: "**"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

