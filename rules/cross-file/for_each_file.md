---
title: 'for_each_file'
description: 'For every matching directory / file, evaluate a nested require: block with the entry as context. alint for_each_file rule, cross-file family.'
sidebar:
  order: 12
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
| `require` | list of nested rule | yes |  | Nested rules evaluated against each matched file. |
| `select` | string or list of string | yes |  | Glob(s) selecting the files to iterate — a single glob, or a list with `!`-prefixed excludes. |
| `when_iter` | string |  |  | Per-iteration `when:` filter — see rule_for_each_dir.when_iter. `iter.has_file(...)` always evaluates to false on file iteration; useful predicates here include `iter.basename`, `iter.ext`, `iter.parent_name`. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A unit test with no snapshot file

The rule fires on this repository:

```text
tests/
tests/snapshots/
tests/snapshots/parser.snap
tests/unit/
tests/unit/lexer.rs
tests/unit/parser.rs
```

`tests/snapshots/parser.snap`:

```text
output
```

`tests/unit/lexer.rs`:

```rust
fn t2() {}
```

`tests/unit/parser.rs`:

```rust
fn t1() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: unit-has-snapshot
    kind: for_each_file
    select: "tests/unit/*.rs"
    require:
      - kind: file_exists
        paths: "tests/snapshots/{stem}.snap"
    level: warning
```

`alint check` reports:

```ansi
[2m--- tests/unit/lexer.rs --------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2munit-has-snapshot[0m
              expected a file matching [tests/snapshots/lexer.snap]

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### Every unit test has a snapshot file

This repository is compliant:

```text
tests/
tests/snapshots/
tests/snapshots/lexer.snap
tests/snapshots/parser.snap
tests/unit/
tests/unit/lexer.rs
tests/unit/parser.rs
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: unit-has-snapshot
    kind: for_each_file
    select: "tests/unit/*.rs"
    require:
      - kind: file_exists
        paths: "tests/snapshots/{stem}.snap"
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
