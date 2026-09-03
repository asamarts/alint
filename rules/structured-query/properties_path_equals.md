---
title: 'properties_path_equals'
description: 'Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value.'
sidebar:
  order: 6
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

### A .properties on the wrong port

The rule fires on this repository:

```text
application.properties
```

`application.properties`:

```text
server.port=9090
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: canonical-port
    kind: properties_path_equals
    paths: "application.properties"
    path: "$['server.port']"
    equals: "8080"
    level: error
```

`alint check` reports:

```ansi
[2m--- application.properties -----------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mcanonical-port[0m
              value at path does not equal expected: expected "8080", got "9090"

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A .properties on the canonical port

This repository is compliant:

```text
application.properties
```

`application.properties`:

```text
server.port=8080
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: canonical-port
    kind: properties_path_equals
    paths: "application.properties"
    path: "$['server.port']"
    equals: "8080"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`json_path_equals`](/docs/rules/structured-query/json_path_equals/)
- [`yaml_path_equals`](/docs/rules/structured-query/yaml_path_equals/)
- [`toml_path_equals`](/docs/rules/structured-query/toml_path_equals/)
- [`xml_path_equals`](/docs/rules/structured-query/xml_path_equals/)
- [`dotenv_path_equals`](/docs/rules/structured-query/dotenv_path_equals/)
- [`ini_path_equals`](/docs/rules/structured-query/ini_path_equals/)
- [`hcl_path_equals`](/docs/rules/structured-query/hcl_path_equals/)
