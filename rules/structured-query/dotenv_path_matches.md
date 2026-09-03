---
title: 'dotenv_path_matches'
description: 'Same shape as the *_equals variants, but the asserted value is a regex matched against string values. alint dotenv_path_matches rule, structured query family.'
sidebar:
  order: 13
categories: ['structured-query']
---

Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. Non-string matches produce a clear "value is not a string" violation.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `if_present` | boolean |  | `false` | When true, a query returning zero matches is silently OK - only real matches that fail the op produce violations. |
| `matches` | string | yes |  | Rust-regex pattern to match against the value at `path`. |
| `path` | string | yes |  | `JSONPath` expression rooted at `$`. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A .env DATABASE_URL pointing at the wrong engine

The rule fires on this repository:

```text
.env
```

`.env`:

```text
DATABASE_URL=mysql://localhost/app
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: postgres-only
    kind: dotenv_path_matches
    paths: ".env"
    path: "$.DATABASE_URL"
    matches: "^postgres://"
    level: error
```

`alint check` reports:

```ansi
[2m--- .env -----------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mpostgres-only[0m
              value at path "mysql://localhost/app" does not match regex
              ^postgres://

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A .env DATABASE_URL on postgres

This repository is compliant:

```text
.env
```

`.env`:

```text
DATABASE_URL=postgres://localhost/app
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: postgres-only
    kind: dotenv_path_matches
    paths: ".env"
    path: "$.DATABASE_URL"
    matches: "^postgres://"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`json_path_matches`](/docs/rules/structured-query/json_path_matches/)
- [`yaml_path_matches`](/docs/rules/structured-query/yaml_path_matches/)
- [`toml_path_matches`](/docs/rules/structured-query/toml_path_matches/)
- [`xml_path_matches`](/docs/rules/structured-query/xml_path_matches/)
- [`properties_path_matches`](/docs/rules/structured-query/properties_path_matches/)
- [`ini_path_matches`](/docs/rules/structured-query/ini_path_matches/)
- [`hcl_path_matches`](/docs/rules/structured-query/hcl_path_matches/)
