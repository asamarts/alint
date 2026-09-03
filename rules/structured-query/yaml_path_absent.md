---
title: 'yaml_path_absent'
description: 'Assert a JSONPath query over the document matches nothing; one file-level violation if present. alint yaml_path_absent rule, structured query family.'
sidebar:
  order: 18
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

### A workflow that grants write-all to the GITHUB_TOKEN

The rule fires on this repository:

```text
.github/
.github/workflows/
.github/workflows/ci.yml
```

`.github/workflows/ci.yml`:

```yaml
name: CI
on: push
permissions: write-all
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-write-all-token
    kind: yaml_path_absent
    paths: ".github/workflows/*.yml"
    path: "$[?($.permissions == 'write-all')]"
    level: error
    message: >-
      Workflow grants `write-all` to the GITHUB_TOKEN; restrict it to
      `contents: read` (or narrower).
```

`alint check` reports:

```ansi
[2m--- .github/workflows/ci.yml ---------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-write-all-token[0m
              Workflow grants `write-all` to the GITHUB_TOKEN; restrict it to
              `contents: read` (or narrower).

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A workflow that restricts the token to read

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
permissions:
  contents: read
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-write-all-token
    kind: yaml_path_absent
    paths: ".github/workflows/*.yml"
    path: "$[?($.permissions == 'write-all')]"
    level: error
    message: >-
      Workflow grants `write-all` to the GITHUB_TOKEN; restrict it to
      `contents: read` (or narrower).
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`json_path_absent`](/docs/rules/structured-query/json_path_absent/)
- [`toml_path_absent`](/docs/rules/structured-query/toml_path_absent/)
- [`xml_path_absent`](/docs/rules/structured-query/xml_path_absent/)
- [`dotenv_path_absent`](/docs/rules/structured-query/dotenv_path_absent/)
- [`properties_path_absent`](/docs/rules/structured-query/properties_path_absent/)
- [`ini_path_absent`](/docs/rules/structured-query/ini_path_absent/)
- [`hcl_path_absent`](/docs/rules/structured-query/hcl_path_absent/)
