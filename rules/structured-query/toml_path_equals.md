---
title: 'toml_path_equals'
description: 'Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value.'
sidebar:
  order: 3
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

### A crate pinned to an older Rust edition

The rule fires on this repository:

```text
crates/
crates/bad/
crates/bad/Cargo.toml
crates/ok/
crates/ok/Cargo.toml
```

`crates/bad/Cargo.toml`:

```toml
[package]
name = "bad"
edition = "2021"
```

`crates/ok/Cargo.toml`:

```toml
[package]
name = "ok"
edition = "2024"
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: rust-edition-2024
    kind: toml_path_equals
    paths: "crates/*/Cargo.toml"
    path: "$.package.edition"
    equals: "2024"
    level: warning
```

### Every crate targets the 2024 edition

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
edition = "2024"
```

`crates/b/Cargo.toml`:

```toml
[package]
name = "b"
edition = "2024"
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: rust-edition-2024
    kind: toml_path_equals
    paths: "crates/*/Cargo.toml"
    path: "$.package.edition"
    equals: "2024"
    level: warning
```

## See also

- [`json_path_equals`](/docs/rules/structured-query/json_path_equals/)
- [`yaml_path_equals`](/docs/rules/structured-query/yaml_path_equals/)
- [`xml_path_equals`](/docs/rules/structured-query/xml_path_equals/)
