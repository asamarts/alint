---
title: 'dir_only_contains'
description: 'Every direct-child file of a directory matching select: must match at least one glob in allow. alint dir_only_contains rule, cross-file family.'
sidebar:
  order: 14
categories: ['cross-file', 'structure']
---

Every direct-child file of a directory matching `select:` must match at least one glob in `allow:`. Catches stray test data in `src/`.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `allow` | string or list of string | yes |  | Basename glob(s) accepted as direct children. Anything else is a violation. |
| `select` | string | yes |  | Glob selecting the directories to enumerate. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Stray non-Rust files under `src`

The rule fires on this repository:

```text
src/
src/a.rs
src/b.rs
src/notes.txt
src/stray.py
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: src-only-rs
    kind: dir_only_contains
    select: "src"
    allow: ["*.rs"]
    level: error
```

`alint check` reports:

```ansi
[2m--- src/notes.txt --------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2msrc-only-rs[0m
              src/notes.txt is not allowed in src (allow: [*.rs])

[2m--- src/stray.py ---------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2msrc-only-rs[0m
              src/stray.py is not allowed in src (allow: [*.rs])

[2mSummary (2 violations):[0m
  [1m[31mx 2 errors[0m
  0 passing [2m*[0m 1 failing
```

### The `src` directory holds only Rust files

This repository is compliant:

```text
src/
src/a.rs
src/b.rs
src/mod.rs
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: src-only-rs
    kind: dir_only_contains
    select: "src"
    allow: ["*.rs"]
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

