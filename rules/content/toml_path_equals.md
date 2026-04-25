---
title: 'toml_path_equals'
description: 'alint rule kind `toml_path_equals` (Content family).'
sidebar:
  order: 15
---

Query a structured document (JSON / YAML / TOML) with a [JSONPath](https://datatracker.ietf.org/doc/html/rfc9535) expression and assert every match deep-equals the supplied value. YAML and TOML are parsed through serde and then treated as JSON-shaped trees, so the same JSONPath engine handles all three formats.

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
```

**Semantics**:
- Multiple matches — every match must equal the expected value.
- Zero matches — counts as a violation (the key the rule is enforcing doesn't exist).
- Unparseable files — one violation per file (not silently skipped).

## See also

- [`json_path_equals`](/docs/rules/content/json_path_equals/)
- [`yaml_path_equals`](/docs/rules/content/yaml_path_equals/)
