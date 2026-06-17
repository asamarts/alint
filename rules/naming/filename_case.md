---
title: 'filename_case'
description: 'Basename (stem only or full) matches a case convention: snake, kebab, pascal, camel, screaming-snake, flat, lower, upper.'
sidebar:
  order: 1
---

Basename (stem only or full) matches a case convention: `snake`, `kebab`, `pascal`, `camel`, `screaming-snake`, `flat`, `lower`, `upper`.

```yaml
- id: rust-snake-case
  kind: filename_case
  paths: "crates/**/src/**/*.rs"
  case: snake
  level: error
```

Fix: `file_rename` — converts the stem to the configured case, preserving extension.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `case` | one of `lower` \| `lowercase` \| `upper` \| `uppercase` \| `pascal` \| `pascalcase` \| `PascalCase` \| `UpperCamelCase` \| `upper_camel` \| `upper_camel_case` \| `camel` \| `camelcase` \| `camelCase` \| `lowerCamelCase` \| `lower_camel` \| `lower_camel_case` \| `snake` \| `snakecase` \| `snake_case` \| `kebab` \| `kebabcase` \| `kebab-case` \| `dash` \| `dashcase` \| `dash-case` \| `screaming-snake` \| `screamingsnake` \| `screamingsnakecase` \| `SCREAMING_SNAKE_CASE` \| `upper_snake` \| `upper_snake_case` \| `flat` \| `flatcase` | yes |  |  |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
