---
title: 'json_path_equals'
description: 'Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value.'
sidebar:
  order: 1
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
```

**Semantics**:
- Multiple matches — every match must equal the expected value.
- Zero matches — counts as a violation (the key the rule is enforcing doesn't exist).
- Unparseable files — one violation per file (not silently skipped).

## See also

- [`yaml_path_equals`](/docs/rules/structured-query/yaml_path_equals/)
- [`toml_path_equals`](/docs/rules/structured-query/toml_path_equals/)
