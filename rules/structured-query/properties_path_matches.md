---
title: 'properties_path_matches'
description: 'Same shape as the *_equals variants, but the asserted value is a regex matched against string values.'
sidebar:
  order: 14
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

### A .properties JDBC url on the wrong engine

The rule fires on this repository:

```text
application.properties
```

`application.properties`:

```text
spring.datasource.url=jdbc:mysql://localhost/app
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: postgres-only
    kind: properties_path_matches
    paths: "application.properties"
    path: "$['spring.datasource.url']"
    matches: "^jdbc:postgresql:"
    level: error
```

`alint check` reports:

```ansi
[2m--- application.properties -----------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mpostgres-only[0m
              value at path "jdbc:mysql://localhost/app" does not match regex
              ^jdbc:postgresql:

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A .properties JDBC url on postgres

This repository is compliant:

```text
application.properties
```

`application.properties`:

```text
spring.datasource.url=jdbc:postgresql://localhost/app
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: postgres-only
    kind: properties_path_matches
    paths: "application.properties"
    path: "$['spring.datasource.url']"
    matches: "^jdbc:postgresql:"
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
- [`dotenv_path_matches`](/docs/rules/structured-query/dotenv_path_matches/)
- [`ini_path_matches`](/docs/rules/structured-query/ini_path_matches/)
- [`hcl_path_matches`](/docs/rules/structured-query/hcl_path_matches/)
