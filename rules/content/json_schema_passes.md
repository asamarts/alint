---
title: 'json_schema_passes'
description: 'alint rule kind `json_schema_passes` (Content family).'
sidebar:
  order: 19
---

Validate every JSON / YAML / TOML file in `paths` against a JSON Schema document. Targets coerce through serde into the same `serde_json::Value` tree the schema sees, so a JSON-format schema can validate a YAML config (Kubernetes manifests, GitHub Actions workflows, Helm `values.schema.json`) or a TOML manifest (`Cargo.toml`, `pyproject.toml`) without separate schemas per format. The schema is loaded + compiled lazily on first evaluation and cached on the rule.

Each schema-validation error becomes one violation, with the failing instance path and the schema's error description in the message. A target that fails to parse produces a single parse-error violation, not a flood of schema errors against junk. Format is detected from the target's extension (`.json` / `.yaml` / `.yml` / `.toml`); pass `format:` to override.

```yaml
- id: package-json-shape
  kind: json_schema_passes
  paths: "packages/*/package.json"
  schema_path: "schemas/package.schema.json"
  level: error

- id: workflow-shape
  kind: json_schema_passes
  paths: ".github/workflows/*.yml"
  schema_path: "schemas/workflow.schema.json"
  format: yaml
  level: warning
```

Check-only — fixing schema violations is a "the user knows what value belongs there" problem, not alint's.

