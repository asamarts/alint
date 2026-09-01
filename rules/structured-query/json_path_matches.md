---
title: 'json_path_matches'
description: 'Same shape as the *_equals variants, but the asserted value is a regex matched against string values. alint json_path_matches rule, structured query family.'
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

### A package version that is not valid semver

The rule fires on this repository:

```text
packages/
packages/bad/
packages/bad/package.json
packages/ok/
packages/ok/package.json
```

`packages/bad/package.json`:

```json
{"name": "@demo/bad", "version": "not-a-semver"}
```

`packages/ok/package.json`:

```json
{"name": "@demo/ok", "version": "1.2.3"}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: require-semver-version
    kind: json_path_matches
    paths: "packages/*/package.json"
    path: "$.version"
    matches: '^\d+\.\d+\.\d+$'
    level: error
```

`alint check` reports:

```ansi
[2m--- packages/bad/package.json --------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mrequire-semver-version[0m
              value at path "not-a-semver" does not match regex ^\d+\.\d+\.\d+$

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every package version is valid semver

This repository is compliant:

```text
packages/
packages/a/
packages/a/package.json
packages/b/
packages/b/package.json
```

`packages/a/package.json`:

```json
{"name": "@demo/a", "version": "1.2.3"}
```

`packages/b/package.json`:

```json
{"name": "@demo/b", "version": "0.10.0"}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: semver-versions
    kind: json_path_matches
    paths: "packages/*/package.json"
    path: "$.version"
    matches: '^\d+\.\d+\.\d+$'
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`yaml_path_matches`](/docs/rules/structured-query/yaml_path_matches/)
- [`toml_path_matches`](/docs/rules/structured-query/toml_path_matches/)
- [`xml_path_matches`](/docs/rules/structured-query/xml_path_matches/)
- [`dotenv_path_matches`](/docs/rules/structured-query/dotenv_path_matches/)
- [`properties_path_matches`](/docs/rules/structured-query/properties_path_matches/)
