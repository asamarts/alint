---
title: 'cross_file_value_equals'
description: 'A value extracted from one authoritative source file must equal a value extracted from one or more targets.'
sidebar:
  order: 3
---

A value extracted from one authoritative `source` file must equal a value extracted from one or more `targets`. `targets` is either a `{ files: <glob>, extract }` map (one query applied per glob match — the per-file `value_extractor` shape) or a sequence of `{ file, extract }` (heterogeneous pins). `extract` is the same one-of as `registry_paths_resolve` (`toml`/`json`/`yaml` JSONPath, `lines`, `regex` group 1). `normalize` relaxes the comparison: `trim` / `lower` / `semver-major` (same major band — the dotnet SDK-band shape). Non-literal extracted values (interpolation / antiquotation) are skipped, not failed; `allow_missing_target` controls absent files/values. Cross-file.

```yaml
- id: workspace-versions-coherent
  kind: cross_file_value_equals
  source:
    file: Cargo.toml
    extract: { toml: "$.workspace.package.version" }
  targets:
    files: "crates/*/Cargo.toml"
    extract: { toml: "$.package.version" }
  normalize: none
  level: error
```

