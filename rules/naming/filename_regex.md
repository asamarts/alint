---
title: 'filename_regex'
description: 'Basename matches a regex. alint filename_regex rule, naming family.'
sidebar:
  order: 2
categories: ['naming']
---

Basename matches a regex. Use `stem: true` to match the stem only.

---

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `pattern` | string | yes |  | Rust regex, automatically anchored with ^...$ by the engine. |
| `stem` | boolean |  | `false` | Match the file stem (no extension) instead of the full basename. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A test file whose name ignores the required `test_` prefix

The rule fires on this repository:

```text
tests/
tests/BadNamingHere.rs
tests/test_lexer.rs
tests/test_parser.rs
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: tests-named-test-prefix
    kind: filename_regex
    paths: "tests/**/*.rs"
    pattern: "^test_[a-z0-9_]+$"
    stem: true
    level: error
```

`alint check` reports:

```ansi
[2m--- tests/BadNamingHere.rs -----------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mtests-named-test-prefix[0m
              filename stem "BadNamingHere" does not match /^test_[a-z0-9_]+$/

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every test file name matches the required `test_` prefix

This repository is compliant:

```text
tests/
tests/test_eval_123.rs
tests/test_lexer.rs
tests/test_parser.rs
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: tests-named-test-prefix
    kind: filename_regex
    paths: "tests/**/*.rs"
    pattern: "^test_[a-z0-9_]+$"
    stem: true
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

