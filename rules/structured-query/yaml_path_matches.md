---
title: 'yaml_path_matches'
description: 'Same shape as the *_equals variants, but the asserted value is a regex matched against string values. alint yaml_path_matches rule, structured query family.'
sidebar:
  order: 8
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

### A workflow action pinned to a floating tag

The rule fires on this repository:

```text
.github/
.github/workflows/
.github/workflows/bad.yml
.github/workflows/ok.yml
```

`.github/workflows/bad.yml`:

```yaml
name: Bad
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: echo hi
```

`.github/workflows/ok.yml`:

```yaml
name: OK
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@0ad4b8fadaa221de15dcec353f45205ec38ea70b
      - run: echo hi
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: pin-actions-to-sha
    kind: yaml_path_matches
    paths: ".github/workflows/*.yml"
    path: "$.jobs.*.steps[*].uses"
    matches: '^[a-zA-Z0-9._/-]+@[a-f0-9]{40}$'
    level: warning
```

`alint check` reports:

```ansi
[2m--- .github/workflows/bad.yml --------------------------------------------------[0m
  [1m[33m!  warning[0m  [2mpin-actions-to-sha[0m
              value at path "actions/checkout@v4" does not match regex
              ^[a-zA-Z0-9._/-]+@[a-f0-9]{40}$

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### Every workflow action is pinned to a commit SHA

This repository is compliant:

```text
.github/
.github/workflows/
.github/workflows/ci.yml
```

`.github/workflows/ci.yml`:

```yaml
name: CI
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@0ad4b8fadaa221de15dcec353f45205ec38ea70b
      - uses: actions/setup-node@1a4442cacd436585916779262731d5b162bc6ec7
      - run: echo hi
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: pin-actions-to-sha
    kind: yaml_path_matches
    paths: ".github/workflows/*.yml"
    path: "$.jobs.*.steps[*].uses"
    matches: '^[a-zA-Z0-9._/-]+@[a-f0-9]{40}$'
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`json_path_matches`](/docs/rules/structured-query/json_path_matches/)
- [`toml_path_matches`](/docs/rules/structured-query/toml_path_matches/)
- [`xml_path_matches`](/docs/rules/structured-query/xml_path_matches/)
- [`dotenv_path_matches`](/docs/rules/structured-query/dotenv_path_matches/)
- [`properties_path_matches`](/docs/rules/structured-query/properties_path_matches/)
