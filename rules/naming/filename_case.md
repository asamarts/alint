---
title: 'filename_case'
description: 'Basename (stem only or full) matches a case convention: snake, kebab, pascal, camel, screaming-snake, flat, lower, upper.'
sidebar:
  order: 1
categories: ['naming']
---

Basename (stem only or full) matches a case convention: `snake`, `kebab`, `pascal`, `camel`, `screaming-snake`, `flat`, `lower`, `upper`.

Fix: `file_rename` — converts the stem to the configured case, preserving extension.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `case` | one of `lower` \| `lowercase` \| `upper` \| `uppercase` \| `pascal` \| `pascalcase` \| `PascalCase` \| `UpperCamelCase` \| `upper_camel` \| `upper_camel_case` \| `camel` \| `camelcase` \| `camelCase` \| `lowerCamelCase` \| `lower_camel` \| `lower_camel_case` \| `snake` \| `snakecase` \| `snake_case` \| `kebab` \| `kebabcase` \| `kebab-case` \| `dash` \| `dashcase` \| `dash-case` \| `screaming-snake` \| `screamingsnake` \| `screamingsnakecase` \| `SCREAMING_SNAKE_CASE` \| `upper_snake` \| `upper_snake_case` \| `flat` \| `flatcase` | yes |  |  |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Rust source files whose names break the snake_case convention

The rule fires on this repository:

```text
src/
src/BadModule.rs
src/OtherName.rs
src/good_module.rs
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: rust-snake
    kind: filename_case
    paths: "src/**/*.rs"
    case: snake
    level: error
```

`alint check` reports:

```ansi
[2m--- src/BadModule.rs -----------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mrust-snake[0m
              filename stem "BadModule" is not snake_case

[2m--- src/OtherName.rs -----------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mrust-snake[0m
              filename stem "OtherName" is not snake_case

[2mSummary (2 violations):[0m
  [1m[31mx 2 errors[0m
  0 passing [2m*[0m 1 failing
```

### Every Rust source file name is snake_case

This repository is compliant:

```text
src/
src/lib.rs
src/main.rs
src/nested_mod.rs
src/with1number.rs
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: rust-snake
    kind: filename_case
    paths: "src/**/*.rs"
    case: snake
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

