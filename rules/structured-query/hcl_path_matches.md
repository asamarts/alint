---
title: 'hcl_path_matches'
description: 'Same shape as the *_equals variants, but the asserted value is a regex matched against string values. alint hcl_path_matches rule, structured query family.'
sidebar:
  order: 16
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

### A Terraform required_version without a pessimistic constraint

The rule fires on this repository:

```text
main.tf
```

`main.tf`:

```text
terraform {
  required_version = "1.0.0"
}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: pin-terraform
    kind: hcl_path_matches
    paths: "main.tf"
    path: "$.terraform.required_version"
    matches: "^~>"
    level: error
```

`alint check` reports:

```ansi
[2m--- main.tf --------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mpin-terraform[0m
              value at path "1.0.0" does not match regex ^~>

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A Terraform required_version with a pessimistic constraint

This repository is compliant:

```text
main.tf
```

`main.tf`:

```text
terraform {
  required_version = "~> 1.0"
}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: pin-terraform
    kind: hcl_path_matches
    paths: "main.tf"
    path: "$.terraform.required_version"
    matches: "^~>"
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
- [`properties_path_matches`](/docs/rules/structured-query/properties_path_matches/)
- [`ini_path_matches`](/docs/rules/structured-query/ini_path_matches/)
