---
title: 'toml_path_absent'
description: 'Assert a JSONPath query over the document matches nothing; one file-level violation if present. alint toml_path_absent rule, structured query family.'
sidebar:
  order: 19
categories: ['structured-query']
---

Assert a JSONPath query over the document matches nothing; one file-level violation if present.

**Semantics**:
- The query must select zero nodes. Any match fires exactly one violation for the file — never per-match, so a `$[?…]` filter that fans out over every top-level key still yields a single violation.
- The existence sibling of the value-checking kinds; mirrors `file_absent` for a path. `equals` / `matches` / `if_present` don't apply.
- Useful for forbidding a key: a `postinstall` script in `package.json`, a `[patch]` table in `Cargo.toml`, or `write-all` permissions in a workflow.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `path` | string | yes |  | `JSONPath` expression rooted at `$`. The rule fires one violation per file if the query matches any node (the path must be absent). |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A Cargo.toml that overrides a dependency via [patch]

The rule fires on this repository:

```text
Cargo.toml
```

`Cargo.toml`:

```toml
[package]
name = "demo"
version = "0.1.0"

[patch.crates-io]
serde = { git = "https://github.com/example/serde" }
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-cargo-patch
    kind: toml_path_absent
    paths: "Cargo.toml"
    path: "$.patch"
    level: error
    message: >-
      Cargo.toml carries a `[patch]` table; patched dependencies are easy to
      forget and hard to audit. Prefer a normal version requirement.
```

`alint check` reports:

```ansi
[2m--- Cargo.toml -----------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-cargo-patch[0m
              Cargo.toml carries a `[patch]` table; patched dependencies are
              easy to forget and hard to audit. Prefer a normal version
              requirement.

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A Cargo.toml with only normal dependencies

This repository is compliant:

```text
Cargo.toml
```

`Cargo.toml`:

```toml
[package]
name = "demo"
version = "0.1.0"

[dependencies]
serde = "1"
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-cargo-patch
    kind: toml_path_absent
    paths: "Cargo.toml"
    path: "$.patch"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`json_path_absent`](/docs/rules/structured-query/json_path_absent/)
- [`yaml_path_absent`](/docs/rules/structured-query/yaml_path_absent/)
- [`xml_path_absent`](/docs/rules/structured-query/xml_path_absent/)
- [`dotenv_path_absent`](/docs/rules/structured-query/dotenv_path_absent/)
- [`properties_path_absent`](/docs/rules/structured-query/properties_path_absent/)
- [`ini_path_absent`](/docs/rules/structured-query/ini_path_absent/)
- [`hcl_path_absent`](/docs/rules/structured-query/hcl_path_absent/)
