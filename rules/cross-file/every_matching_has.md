---
title: 'every_matching_has'
description: 'For every file or directory matching select:, every nested rule under require: must be satisfied. alint every_matching_has rule, cross-file family.'
sidebar:
  order: 16
categories: ['cross-file']
---

For every file or directory matching `select:`, every nested rule under `require:` must be satisfied. Lightweight sibling of `pair` that iterates both file and directory entries. `select:` is a single glob or a list with `!`-prefixed excludes (e.g. `["packages/*", "!packages/internal"]`).

---

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `require` | list of nested rule | yes |  | One or more nested rules that every file or directory matching `select` must satisfy. |
| `select` | SelectSpec | yes |  | Glob(s) selecting the files/dirs to iterate: a single glob, or a list with `!`-prefixed excludes (e.g. `["packages/*", "!packages/internal"]`). |
| `when_iter` | string |  | `null` | Per-iteration `when:` filter (same semantics as `for_each_dir`'s `when_iter`). |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A package directory without a `package.json`

The rule fires on this repository:

```text
packages/
packages/alpha/
packages/alpha/package.json
packages/beta/
packages/beta/README.md
packages/gamma/
packages/gamma/package.json
```

`packages/alpha/package.json`:

```json
{}
```

`packages/beta/README.md`:

```markdown
# beta
```

`packages/gamma/package.json`:

```json
{}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: every-pkg-has-package-json
    kind: every_matching_has
    select: "packages/*"
    require:
      - kind: file_exists
        paths: "{path}/package.json"
    level: error
```

`alint check` reports:

```ansi
[2m--- packages/beta --------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mevery-pkg-has-package-json[0m
              expected a file matching [packages/beta/package.json]

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every package directory has a `package.json`

This repository is compliant:

```text
packages/
packages/alpha/
packages/alpha/package.json
packages/beta/
packages/beta/package.json
```

`packages/alpha/package.json`:

```json
{}
```

`packages/beta/package.json`:

```json
{}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: every-pkg-has-package-json
    kind: every_matching_has
    select: "packages/*"
    require:
      - kind: file_exists
        paths: "{path}/package.json"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

