---
title: 'registry_paths_resolve'
description: 'A manifest file enumerates path entries; each must resolve to an on-disk artefact. alint registry_paths_resolve rule, cross-file family.'
sidebar:
  order: 3
---

A manifest file enumerates path entries; each must resolve to an on-disk artefact. `extract` pulls the entry list via a structured query (`toml` / `json` / `yaml` RFC 9535 JSONPath), a line list (`lines`, optional `comment` prefix), or a regex capture (`regex`, group 1). `expect` (`any` / `file` / `dir`) and `must_contain` constrain the resolved kind; `exclude_query` subtracts entries; `entries_are_globs` expands each entry as a glob. Non-literal entries (interpolation / antiquotation) are skipped, not failed. Optional `orphans` adds the reverse-completeness check: on-disk artefacts under the `space` glob that no entry references (the "new crate not wired into the workspace" detector). Cross-file: reads one manifest, resolves against the file index.

```yaml
- id: workspace-members-resolve
  kind: registry_paths_resolve
  source: Cargo.toml
  extract: { toml: "$.workspace.members[*]" }
  expect: dir
  must_contain: Cargo.toml
  orphans: { space: "crates/*/Cargo.toml", unreferenced: warn }
  level: error
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `base` | string |  |  | Resolve entries relative to: registry_dir (default), lint_root, or an explicit path. |
| `entries_are_globs` | boolean |  |  |  |
| `exclude_query` | string |  |  |  |
| `expect` | one of `any` \| `file` \| `dir` |  |  |  |
| `extract` | object | yes |  | Exactly one of: toml/json/yaml (RFC 9535 JSONPath string), lines (object; optional `comment` prefix, default `#`), regex (string; capture group 1 is the path). |
| `must_contain` | string |  |  |  |
| `orphans` | object |  |  |  |
| `source` | string | yes |  | The manifest/registry file (path, or a glob to run once per matching manifest) that enumerates the path entries. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
