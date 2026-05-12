---
title: 'toml_path_matches'
description: 'alint rule kind `toml_path_matches` (Structured query family).'
sidebar:
  order: 6
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
```

## See also

- [`json_path_matches`](/docs/rules/structured-query/json_path_matches/)
- [`yaml_path_matches`](/docs/rules/structured-query/yaml_path_matches/)
