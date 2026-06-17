---
title: 'xml_path_matches'
description: 'Same shape as the *_equals variants, but the asserted value is a **regex** matched against string values.'
sidebar:
  order: 8
---

Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. Non-string matches produce a clear "value is not a string" violation.

```yaml
- id: semver-version
  kind: json_path_matches
  paths: "packages/*/package.json"
  path: "$.version"
  matches: '^\d+\.\d+\.\d+$'
  level: error

- id: pin-actions-to-sha
  kind: yaml_path_matches
  paths: ".github/workflows/*.yml"
  path: "$.jobs.*.steps[*].uses"
  matches: '^[a-zA-Z0-9._/-]+@[a-f0-9]{40}$'
  level: warning

- id: packageref-has-version
  kind: xml_path_matches
  paths: "**/*.csproj"
  path: "$.Project.ItemGroup.PackageReference[*]['@Version']"
  matches: '^\d'
  level: error
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `if_present` | boolean |  | `false` | When true, a query returning zero matches is silently OK - only real matches that fail the op produce violations. |
| `matches` | string | yes |  | Rust-regex pattern to match against the value at `path`. |
| `path` | string | yes |  | `JSONPath` expression rooted at `$`. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## See also

- [`json_path_matches`](/docs/rules/structured-query/json_path_matches/)
- [`yaml_path_matches`](/docs/rules/structured-query/yaml_path_matches/)
- [`toml_path_matches`](/docs/rules/structured-query/toml_path_matches/)
