---
title: 'dotenv_path_absent'
description: 'Assert a JSONPath query over the document matches nothing; one file-level violation if present. alint dotenv_path_absent rule, structured query family.'
sidebar:
  order: 21
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

### A .env that commits an AWS secret

The rule fires on this repository:

```text
.env
```

`.env`:

```text
NODE_ENV=production
AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMIexampleKEY
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-committed-secret
    kind: dotenv_path_absent
    paths: ".env"
    path: "$.AWS_SECRET_ACCESS_KEY"
    level: error
    message: >-
      A secret is committed in .env; move it to a secrets manager and keep
      .env out of version control.
```

`alint check` reports:

```ansi
[2m--- .env -----------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-committed-secret[0m
              A secret is committed in .env; move it to a secrets manager and
              keep .env out of version control.

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A .env with no committed secret

This repository is compliant:

```text
.env
```

`.env`:

```text
NODE_ENV=production
PORT=8080
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-committed-secret
    kind: dotenv_path_absent
    paths: ".env"
    path: "$.AWS_SECRET_ACCESS_KEY"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`json_path_absent`](/docs/rules/structured-query/json_path_absent/)
- [`yaml_path_absent`](/docs/rules/structured-query/yaml_path_absent/)
- [`toml_path_absent`](/docs/rules/structured-query/toml_path_absent/)
- [`xml_path_absent`](/docs/rules/structured-query/xml_path_absent/)
- [`properties_path_absent`](/docs/rules/structured-query/properties_path_absent/)
- [`ini_path_absent`](/docs/rules/structured-query/ini_path_absent/)
- [`hcl_path_absent`](/docs/rules/structured-query/hcl_path_absent/)
