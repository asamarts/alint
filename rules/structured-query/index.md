---
title: 'Structured query'
description: 'Rule reference: the structured query family.'
sidebar:
  order: 3
  label: 'Structured query'
---

Rule kinds in the **Structured query** family. Each entry below has its own page with options, an example, and any auto-fix support.

- [`json_path_equals`](/docs/rules/structured-query/json_path_equals/) — Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value.
- [`yaml_path_equals`](/docs/rules/structured-query/yaml_path_equals/) — Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value.
- [`toml_path_equals`](/docs/rules/structured-query/toml_path_equals/) — Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value.
- [`json_path_matches`](/docs/rules/structured-query/json_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values.
- [`yaml_path_matches`](/docs/rules/structured-query/yaml_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values.
- [`toml_path_matches`](/docs/rules/structured-query/toml_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values.
- [`json_schema_passes`](/docs/rules/structured-query/json_schema_passes/) — Validate every JSON / YAML / TOML file in `paths` against a JSON Schema document.
