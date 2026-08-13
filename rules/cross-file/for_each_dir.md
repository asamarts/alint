---
title: 'for_each_dir'
description: 'For every matching directory / file, evaluate a nested require: block with the entry as context. alint for_each_dir rule, cross-file family.'
sidebar:
  order: 11
categories: ['cross-file']
---

For every matching directory / file, evaluate a nested `require:` block with the entry as context. Template tokens (`{dir}`, `{stem}`, `{ext}`, `{basename}`, `{path}`, `{parent_name}`) expand against each match. `select:` is a single glob or a list with `!`-prefixed excludes (e.g. `["src/*", "!src/internal"]`).

**`when_iter:` — per-iteration filter.** Optional expression in the `when:` grammar, with one extra namespace: `iter.*` references the entry currently being iterated. Iterations whose verdict is false are skipped before any nested rule is built — the canonical use case for monorepos shaped like Cargo / pnpm / Bazel workspaces:

The `iter` namespace exposes:

| Reference | Type | Notes |
|---|---|---|
| `iter.path` | string | Relative path of the iterated entry. |
| `iter.basename` | string | Basename. |
| `iter.parent_name` | string | Parent dir name. |
| `iter.stem` | string | Basename minus the final extension (mainly useful for files). |
| `iter.ext` | string | Final extension without the dot. |
| `iter.is_dir` | bool | True for `for_each_dir`, false for `for_each_file`; always available. |
| `iter.has_file(pattern)` | bool | Glob match relative to the iterated dir. `iter.has_file("Cargo.toml")`, `iter.has_file("**/*.bzl")`. Always false for file iteration. |

`when_iter:` composes with the rule's outer `when:` (whole-rule gate, evaluated once) and with each nested rule's `when:` (which now also sees the same `iter.*` context). Same field is available on `for_each_file` and `every_matching_has`.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `require` | list of nested rule | yes |  | Nested rules evaluated against each matched directory. |
| `select` | string or list of string | yes |  | Glob(s) selecting the directories to iterate — a single glob, or a list with `!`-prefixed excludes (e.g. ["src/*", "!src/internal"]). |
| `when_iter` | string |  |  | Per-iteration `when:` filter — evaluated against `iter.*` in the iterated entry's context. Iterations whose verdict is false are skipped before any nested rule is built. Examples: `iter.has_file("Cargo.toml")`, `iter.basename matches "^pkg-"`. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A module directory missing its `mod.rs`

The rule fires on this repository:

```text
src/
src/alpha/
src/alpha/mod.rs
src/beta/
src/beta/lib.rs
src/gamma/
src/gamma/mod.rs
```

`src/alpha/mod.rs`:

```rust
pub fn a() {}
```

`src/beta/lib.rs`:

```rust
pub fn b() {}
```

`src/gamma/mod.rs`:

```rust
pub fn c() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: every-module-has-mod
    kind: for_each_dir
    select: "src/*"
    require:
      - kind: file_exists
        paths: "{path}/mod.rs"
    level: error
```

`alint check` reports:

```ansi
[2m--- src/beta -------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mevery-module-has-mod[0m
              expected a file matching [src/beta/mod.rs]

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every module directory has a `mod.rs`

This repository is compliant:

```text
src/
src/alpha/
src/alpha/mod.rs
src/beta/
src/beta/mod.rs
```

`src/alpha/mod.rs`:

```rust
pub fn a() {}
```

`src/beta/mod.rs`:

```rust
pub fn b() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: every-module-has-mod
    kind: for_each_dir
    select: "src/*"
    require:
      - kind: file_exists
        paths: "{path}/mod.rs"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`for_each_file`](/docs/rules/cross-file/for_each_file/)
