---
title: 'json_schema_passes'
description: 'Validate every JSON / YAML / TOML file in paths against a JSON Schema document. alint json_schema_passes rule, structured query family.'
sidebar:
  order: 9
categories: ['structured-query']
---

Validate every JSON / YAML / TOML file in `paths` against a JSON Schema document. Targets coerce through serde into the same `serde_json::Value` tree the schema sees, so a JSON-format schema can validate a YAML config (Kubernetes manifests, GitHub Actions workflows, Helm `values.schema.json`) or a TOML manifest (`Cargo.toml`, `pyproject.toml`) without separate schemas per format. The schema is loaded + compiled lazily on first evaluation and cached on the rule.

Each schema-validation error becomes one violation, with the failing instance path and the schema's error description in the message. A target that fails to parse produces a single parse-error violation, not a flood of schema errors against junk. Format is detected from the target's extension (`.json` / `.yaml` / `.yml` / `.toml`); pass `format:` to override.

Check-only — fixing schema violations is a "the user knows what value belongs there" problem, not alint's.

---

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `format` | one of `json` \| `yaml` \| `yml` \| `toml` |  |  | Override the auto-detected target format. When omitted, format is inferred from each target file's extension (.json / .yaml / .yml / .toml). |
| `schema_path` | string | yes |  | Path to a JSON Schema file relative to the lint root. The schema must itself be JSON even when validating YAML / TOML targets. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A package.json that violates its JSON Schema

The rule fires on this repository:

```text
package.json
schemas/
schemas/package.schema.json
```

`package.json`:

```json
{
  "name": "demo",
  "version": "v1.x"
}
```

`schemas/package.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": ["name", "version"],
  "properties": {
    "name": {"type": "string"},
    "version": {"type": "string", "pattern": "^[0-9]+\\.[0-9]+\\.[0-9]+$"}
  }
}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: package-conforms
    kind: json_schema_passes
    paths: "package.json"
    schema_path: "schemas/package.schema.json"
    level: error
```

`alint check` reports:

```ansi
[2m--- package.json ---------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mpackage-conforms[0m
              schema violation at `/version`: "v1.x" does not match
              "^[0-9]+\.[0-9]+\.[0-9]+$"

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A package.json that conforms to its JSON Schema

This repository is compliant:

```text
package.json
schemas/
schemas/package.schema.json
```

`package.json`:

```json
{
  "name": "demo",
  "version": "1.2.3"
}
```

`schemas/package.schema.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": ["name", "version"],
  "properties": {
    "name": {"type": "string"},
    "version": {"type": "string", "pattern": "^[0-9]+\\.[0-9]+\\.[0-9]+$"}
  }
}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: package-conforms
    kind: json_schema_passes
    paths: "package.json"
    schema_path: "schemas/package.schema.json"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

