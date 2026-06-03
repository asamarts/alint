---
title: 'cross_file'
description: 'A value, or *set* of values, extracted from one authoritative source file must hold a relation to the values extracted from each of one or.'
sidebar:
  order: 4
---

A value — or *set* of values — extracted from one authoritative `source` file must hold a `relation` to the values extracted from each of one or more `targets`. `targets` is either a `{ files: <glob>, extract }` map (one query applied per glob match) or a sequence of `{ file, extract }` (heterogeneous pins); `extract` is the same one-of as `registry_paths_resolve` (`toml`/`json`/`yaml` JSONPath, `lines`, `regex` group 1). `relation` (default `equals`) selects the assertion, checked independently per target:

| `relation` | source ⇒ | asserts (per target) |
|---|---|---|
| `equals` | exactly one value `v` | every target value `== v` |
| `subset` | a set `S` | `S ⊆ T` (singleton `S` = membership) |
| `superset` | a set `S` | `S ⊇ T` (the source covers every target value) |
| `set_equals` | a set `S` | `S == T` |

`normalize` relaxes the comparison (`trim` / `lower` / `semver-major` — same major band, the dotnet SDK-band shape). Non-literal extracted values (interpolation / antiquotation) are skipped, not failed; `allow_missing_target` controls absent files/values. The released `cross_file_value_equals` is a **byte-compatible alias** (`relation` defaults to `equals`). Cross-file.

```yaml
# equals (the default; the cross_file_value_equals shape)
- id: workspace-versions-coherent
  kind: cross_file
  source:  { file: Cargo.toml, extract: { toml: "$.workspace.package.version" } }
  targets: { files: "crates/*/Cargo.toml", extract: { toml: "$.package.version" } }
  relation: equals
  level: error

# subset — every catalog reference must resolve to a declared catalog key
- id: pnpm-catalog-refs-resolve
  kind: cross_file
  source:  { file: pnpm-workspace.yaml, extract: { yaml: "$.catalog.*" } }
  targets: { files: "packages/**/package.json", extract: { regex: 'catalog:(\S+)' } }
  relation: subset
  level: error
```

