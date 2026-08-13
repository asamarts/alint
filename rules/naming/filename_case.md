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

