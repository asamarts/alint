---
title: 'toml_path_matches'
description: 'Same shape as the *_equals variants, but the asserted value is a regex matched against string values. alint toml_path_matches rule, structured query family.'
sidebar:
  order: 11
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

### A crate version that isn't valid semver

The rule fires on this repository:

```text
crates/
crates/a/
crates/a/Cargo.toml
crates/b/
crates/b/Cargo.toml
```

`crates/a/Cargo.toml`:

```toml
[package]
name = "a"
version = "1.2.3"
```

`crates/b/Cargo.toml`:

```toml
[package]
name = "b"
version = "0.4"
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: pinned-version
    kind: toml_path_matches
    paths: "crates/*/Cargo.toml"
    path: "$.package.version"
    matches: '^\d+\.\d+\.\d+$'
    level: error
```

`alint check` reports:

```ansi
[2m--- crates/b/Cargo.toml --------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mpinned-version[0m
              value at path "0.4" does not match regex ^\d+\.\d+\.\d+$

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every crate version matches the semver pattern

This repository is compliant:

```text
crates/
crates/a/
crates/a/Cargo.toml
crates/b/
crates/b/Cargo.toml
```

`crates/a/Cargo.toml`:

```toml
[package]
name = "a"
version = "1.2.3"
```

`crates/b/Cargo.toml`:

```toml
[package]
name = "b"
version = "0.4.5"
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: pinned-version
    kind: toml_path_matches
    paths: "crates/*/Cargo.toml"
    path: "$.package.version"
    matches: '^\d+\.\d+\.\d+$'
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`json_path_matches`](/docs/rules/structured-query/json_path_matches/)
- [`yaml_path_matches`](/docs/rules/structured-query/yaml_path_matches/)
- [`xml_path_matches`](/docs/rules/structured-query/xml_path_matches/)
- [`dotenv_path_matches`](/docs/rules/structured-query/dotenv_path_matches/)
- [`properties_path_matches`](/docs/rules/structured-query/properties_path_matches/)
- [`ini_path_matches`](/docs/rules/structured-query/ini_path_matches/)
- [`hcl_path_matches`](/docs/rules/structured-query/hcl_path_matches/)
