---
title: 'toml_path_matches'
description: 'Same shape as the *_equals variants, but the asserted value is a **regex** matched against string values.'
sidebar:
  order: 7
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

## See also

- [`json_path_matches`](/docs/rules/structured-query/json_path_matches/)
- [`yaml_path_matches`](/docs/rules/structured-query/yaml_path_matches/)
- [`xml_path_matches`](/docs/rules/structured-query/xml_path_matches/)
