---
title: 'yaml_path_equals'
description: 'Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value.'
sidebar:
  order: 2
categories: ['structured-query']
---

Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value.

**Semantics**:
- Multiple matches — every match must equal the expected value.
- Zero matches — counts as a violation (the key the rule is enforcing doesn't exist).
- Unparseable files — one violation per file (not silently skipped).

<a id="xml-mapping"></a>
**XML mapping** (`xml_path_*`): XML is mapped to the queryable tree with the xmltodict-style convention so the JSONPath reads like the XML — the document is `{ <root-element>: … }` (`$.Project…`, `$.project…`); attributes are `@name` keys (`['@Version']`); a leaf element collapses to its text (`<TargetFramework>net8.0</TargetFramework>` → `"net8.0"`); repeated sibling elements become an array (use `dependency[*]`, which works for one or many); namespaces flatten to the local name (Maven's default `pom.xml` namespace just works). **Every XML leaf value is a string** — quote the expected value (`equals: "4.0.0"`, not `equals: 4.0.0`) or use `xml_path_matches`. Full rationale and edge cases: `docs/design/v0.10/xml_path.md`.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `equals` | any value | yes |  | Expected value. Any JSON type (string, number, boolean, null, array, object). |
| `if_present` | boolean |  | `false` | When true, a query returning zero matches is silently OK - only real matches that fail the op produce violations. |
| `path` | string | yes |  | `JSONPath` expression rooted at `$`. Supports dot-access (`$.foo.bar`), array index (`$.deps[0]`), wildcards (`$.deps[*]`), filters, and every other RFC 9535 construct. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A workflow that sets contents permission to write

The rule fires on this repository:

```text
.github/
.github/workflows/
.github/workflows/bad.yml
.github/workflows/ci.yml
```

`.github/workflows/bad.yml`:

```yaml
name: Bad
on: push
permissions:
  contents: write
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
```

`.github/workflows/ci.yml`:

```yaml
name: CI
on: push
permissions:
  contents: read
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: workflow-contents-read
    kind: yaml_path_equals
    paths: ".github/workflows/*.yml"
    path: "$.permissions.contents"
    equals: "read"
    level: error
```

### Every workflow sets contents permission to read

This repository is compliant:

```text
.github/
.github/workflows/
.github/workflows/ci.yml
.github/workflows/release.yml
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

`.github/workflows/release.yml`:

```yaml
name: Release
on: push
permissions:
  contents: read
jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: workflow-default-permissions
    kind: yaml_path_equals
    paths: ".github/workflows/*.yml"
    path: "$.permissions.contents"
    equals: "read"
    level: error
```

## See also

- [`json_path_equals`](/docs/rules/structured-query/json_path_equals/)
- [`toml_path_equals`](/docs/rules/structured-query/toml_path_equals/)
- [`xml_path_equals`](/docs/rules/structured-query/xml_path_equals/)
