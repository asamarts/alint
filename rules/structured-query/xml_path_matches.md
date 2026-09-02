---
title: 'xml_path_matches'
description: 'Same shape as the *_equals variants, but the asserted value is a regex matched against string values. alint xml_path_matches rule, structured query family.'
sidebar:
  order: 12
categories: ['structured-query']
---

Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. Non-string matches produce a clear "value is not a string" violation.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `if_present` | boolean |  | `false` | When true, a query returning zero matches is silently OK - only real matches that fail the op produce violations. |
| `matches` | string | yes |  | Rust-regex pattern to match against the value at `path`. |
| `path` | string | yes |  | `JSONPath` expression rooted at `$`. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A Maven dependency whose version is not a concrete release

The rule fires on this repository:

```text
pom.xml
```

`pom.xml`:

```text
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <dependencies>
    <dependency><artifactId>guava</artifactId><version>33.0.0-jre</version></dependency>
    <dependency><artifactId>internal</artifactId><version>${revision}</version></dependency>
  </dependencies>
</project>
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: pom-deps-pinned
    kind: xml_path_matches
    paths: "pom.xml"
    path: "$.project.dependencies.dependency[*].version"
    matches: '^\d'
    level: error
```

`alint check` reports:

```ansi
[2m--- pom.xml --------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mpom-deps-pinned[0m
              value at path "${revision}" does not match regex ^\d

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every Maven dependency declares a concrete version

This repository is compliant:

```text
pom.xml
```

`pom.xml`:

```text
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <dependencies>
    <dependency><groupId>com.google.guava</groupId><artifactId>guava</artifactId><version>33.0.0-jre</version></dependency>
    <dependency><groupId>junit</groupId><artifactId>junit</artifactId><version>4.13.2</version></dependency>
  </dependencies>
</project>
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: pom-deps-versioned
    kind: xml_path_matches
    paths: "pom.xml"
    path: "$.project.dependencies.dependency[*].version"
    matches: '^\d'
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`json_path_matches`](/docs/rules/structured-query/json_path_matches/)
- [`yaml_path_matches`](/docs/rules/structured-query/yaml_path_matches/)
- [`toml_path_matches`](/docs/rules/structured-query/toml_path_matches/)
- [`dotenv_path_matches`](/docs/rules/structured-query/dotenv_path_matches/)
- [`properties_path_matches`](/docs/rules/structured-query/properties_path_matches/)
- [`ini_path_matches`](/docs/rules/structured-query/ini_path_matches/)
- [`hcl_path_matches`](/docs/rules/structured-query/hcl_path_matches/)
