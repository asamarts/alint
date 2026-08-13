---
title: 'max_directory_depth'
description: 'Tree depth from repo root may not exceed max_depth. alint max_directory_depth rule, structure family.'
sidebar:
  order: 1
categories: ['structure']
---

Tree depth from repo root may not exceed `max_depth`. A shallow depth stops deeply-nested imports and keeps CI path globs sane.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max_depth` | integer (>= 1) | yes |  | Maximum allowed path depth (number of `/`-separated components). |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A source file nested too many directories deep

The rule fires on this repository:

```text
src/
src/deep/
src/deep/nested/
src/deep/nested/way_down/
src/deep/nested/way_down/lost.rs
src/lib.rs
top.txt
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: shallow
    kind: max_directory_depth
    paths: "**"
    max_depth: 3
    level: warning
```

### Every file stays within the directory-depth cap

This repository is compliant:

```text
README.md
src/
src/lib.rs
src/mod/
src/mod/inner.rs
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: shallow
    kind: max_directory_depth
    paths: "**"
    max_depth: 3
    level: warning
```

