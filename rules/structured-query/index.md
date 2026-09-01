---
title: 'Structured query'
description: 'Rule reference: the structured query family.'
sidebar:
  order: 3
  label: 'Structured query'
---

Rule kinds in the **Structured query** family. Each rule below links to its own page with options, an example, and any auto-fix support.

| Rule | Description |
| --- | --- |
| [`json_path_equals`](/docs/rules/structured-query/json_path_equals/) | Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value. |
| [`yaml_path_equals`](/docs/rules/structured-query/yaml_path_equals/) | Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value. |
| [`toml_path_equals`](/docs/rules/structured-query/toml_path_equals/) | Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value. |
| [`xml_path_equals`](/docs/rules/structured-query/xml_path_equals/) | Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value. |
| [`dotenv_path_equals`](/docs/rules/structured-query/dotenv_path_equals/) | Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value. |
| [`properties_path_equals`](/docs/rules/structured-query/properties_path_equals/) | Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value. |
| [`json_path_matches`](/docs/rules/structured-query/json_path_matches/) | Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. |
| [`yaml_path_matches`](/docs/rules/structured-query/yaml_path_matches/) | Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. |
| [`toml_path_matches`](/docs/rules/structured-query/toml_path_matches/) | Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. |
| [`xml_path_matches`](/docs/rules/structured-query/xml_path_matches/) | Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. |
| [`dotenv_path_matches`](/docs/rules/structured-query/dotenv_path_matches/) | Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. |
| [`properties_path_matches`](/docs/rules/structured-query/properties_path_matches/) | Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. |
| [`json_path_absent`](/docs/rules/structured-query/json_path_absent/) | Assert a JSONPath query over the document matches nothing; one file-level violation if present. |
| [`yaml_path_absent`](/docs/rules/structured-query/yaml_path_absent/) | Assert a JSONPath query over the document matches nothing; one file-level violation if present. |
| [`toml_path_absent`](/docs/rules/structured-query/toml_path_absent/) | Assert a JSONPath query over the document matches nothing; one file-level violation if present. |
| [`xml_path_absent`](/docs/rules/structured-query/xml_path_absent/) | Assert a JSONPath query over the document matches nothing; one file-level violation if present. |
| [`dotenv_path_absent`](/docs/rules/structured-query/dotenv_path_absent/) | Assert a JSONPath query over the document matches nothing; one file-level violation if present. |
| [`properties_path_absent`](/docs/rules/structured-query/properties_path_absent/) | Assert a JSONPath query over the document matches nothing; one file-level violation if present. |
| [`json_schema_passes`](/docs/rules/structured-query/json_schema_passes/) | Validate every JSON / YAML / TOML / XML / dotenv / properties file in `paths` against a JSON Schema document. |
