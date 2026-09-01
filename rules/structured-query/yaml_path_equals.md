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
**XML mapping** applies to every XML surface: `xml_path_*`, `json_schema_passes` `format: xml`, and the `xml:` extract used by `cross_file` / `file_graph` / `registry_paths_resolve`. XML is mapped to the queryable tree with the xmltodict-style convention so the JSONPath reads like the XML — the document is `{ <root-element>: … }` (`$.Project…`, `$.project…`); attributes are `@name` keys (`['@Version']`, in bracket notation, since `.@Version` is not valid JSONPath); a leaf element collapses to its text (`<TargetFramework>net8.0</TargetFramework>` → `"net8.0"`); namespaces flatten to the local name (Maven's default `pom.xml` namespace just works). Two properties of XML's data model to keep in mind:

- **Every XML leaf value is a string.** Quote expected values (`equals: "4.0.0"`, not `equals: 4.0.0`), reach for `*_path_matches`, and in a `json_schema_passes` schema type XML fields as `string` with a `pattern` — `type: integer` / `boolean` / `number` always fail against XML (every leaf is a string); `type: array` / `object` additionally depend on cardinality (next point).
- **Cardinality is data-dependent.** A single `<dependency>` is an object (a string for a leaf element); two or more become an array. A `[*]` wildcard therefore does **not** normalize cardinality — `dependency[*]` reads nothing for one element but every element for many, and a leaf query flips the same way (an attribute such as MSBuild's `Condition=` also turns a would-be leaf string into a `{ "@Condition": …, "#text": … }` object). When a rule must handle both one and many, use recursive descent: `$.Project.ItemGroup.ProjectReference..['@Include']` reads the `@Include` of one or many. Scope the `..` under a parent element so it does not over-match a same-named key elsewhere.

Full rationale and edge cases: `docs/design/v0.10/xml_path.md`.

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

`alint check` reports:

```ansi
[2m--- .github/workflows/bad.yml --------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mworkflow-contents-read[0m
              value at path does not equal expected: expected "read", got
              "write"

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
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

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`json_path_equals`](/docs/rules/structured-query/json_path_equals/)
- [`toml_path_equals`](/docs/rules/structured-query/toml_path_equals/)
- [`xml_path_equals`](/docs/rules/structured-query/xml_path_equals/)
