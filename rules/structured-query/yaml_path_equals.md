---
title: 'yaml_path_equals'
description: 'Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value.'
sidebar:
  order: 2
---

Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value.

```yaml
- id: require-mit-license
  kind: json_path_equals
  paths: "packages/*/package.json"
  path: "$.license"
  equals: "MIT"
  level: error

- id: workflow-contents-read
  kind: yaml_path_equals
  paths: ".github/workflows/*.yml"
  path: "$.permissions.contents"
  equals: "read"
  level: error

- id: rust-edition-2024
  kind: toml_path_equals
  paths: "crates/*/Cargo.toml"
  path: "$.package.edition"
  equals: "2024"
  level: warning

- id: csproj-targets-net8
  kind: xml_path_equals
  paths: "**/*.csproj"
  path: "$.Project.PropertyGroup.TargetFramework"
  equals: "net8.0"
  level: error
```

**Semantics**:
- Multiple matches — every match must equal the expected value.
- Zero matches — counts as a violation (the key the rule is enforcing doesn't exist).
- Unparseable files — one violation per file (not silently skipped).

<a id="xml-mapping"></a>
**XML mapping** (`xml_path_*`): XML is mapped to the queryable tree with the xmltodict-style convention so the JSONPath reads like the XML — the document is `{ <root-element>: … }` (`$.Project…`, `$.project…`); attributes are `@name` keys (`['@Version']`); a leaf element collapses to its text (`<TargetFramework>net8.0</TargetFramework>` → `"net8.0"`); repeated sibling elements become an array (use `dependency[*]`, which works for one or many); namespaces flatten to the local name (Maven's default `pom.xml` namespace just works). **Every XML leaf value is a string** — quote the expected value (`equals: "4.0.0"`, not `equals: 4.0.0`) or use `xml_path_matches`. Full rationale and edge cases: `docs/design/v0.10/xml_path.md`.

## See also

- [`json_path_equals`](/docs/rules/structured-query/json_path_equals/)
- [`toml_path_equals`](/docs/rules/structured-query/toml_path_equals/)
- [`xml_path_equals`](/docs/rules/structured-query/xml_path_equals/)
