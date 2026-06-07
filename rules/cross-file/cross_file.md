---
title: 'cross_file'
description: 'A source must hold a relation to one or more targets (or, for resolves, the filesystem). alint cross_file rule, cross-file family.'
sidebar:
  order: 4
---

A `source` must hold a `relation` to one or more `targets` (or, for `resolves`, the filesystem). The `source` is a single `{ file, extract }` — **or**, for the set relations only (`subset` / `superset` / `set_equals`), `{ files: <glob>, extract }`, whose matches are read and whose extracted values are **unioned into one set** (the "every `*hl-X*` across `runtime/doc/*.txt` must equal the `default link X` set in `highlight.c`" shape — symbol-set / cross-language parity). For the value relations, `targets` is either a `{ files: <glob>, extract }` map (one query applied per glob match) or a sequence of `{ file, extract }` (heterogeneous pins); `extract` is the same one-of as `registry_paths_resolve` (`toml`/`json`/`yaml` JSONPath, `lines`, `regex` group 1) plus `whole_file: {}` (the entire file content as one value — for byte-level content equality without `identical`'s no-`extract`/no-`normalize` constraint). `relation` (default `equals`) selects the assertion, checked independently per target; the *shape* (which of `source.file`/`source.files`, `source.extract`, `targets` is present) follows the relation and is validated at load:

| `relation` | source ⇒ | asserts (per target) |
|---|---|---|
| `equals` | exactly one value `v` | every target value `== v` |
| `subset` | a set `S` | `S ⊆ T` (singleton `S` = membership) |
| `superset` | a set `S` | `S ⊇ T` (the source covers every target value) |
| `set_equals` | a set `S` | `S == T` |
| `identical` | whole file content | the target is byte-identical (no `extract`; optional `skip_header_lines`) |
| `resolves` | a set of paths | each path exists on disk (no `targets`; the forward half of `registry_paths_resolve`) |

`normalize` relaxes the value comparison — a single transform or an ordered list applied left-to-right (`normalize: [trim, semver-minor]`): `trim`, `lower`, `semver-major` (the leading `MAJOR` band — the dotnet SDK shape), and `semver-minor` (the leading `MAJOR.MINOR` band, each token's leading digits with a non-digit prefix stripped — so `4.36-dev`, `4.36.0`, `pnpm@11.3.0` and `>=22.13` reconcile; the protobuf / pnpm version-format case). `identical` reads whole files byte-for-byte (`normalize`/`extract` do not apply), with an optional `skip_header_lines` to ignore a differing license/header; `resolves` extracts paths from the `source` (no `targets`) and checks each exists relative to the source file's directory. Non-literal extracted values (interpolation / antiquotation) are skipped, not failed — **except** a `whole_file: {}` source/target, whose single value (the entire file content) is compared verbatim even when it embeds `${…}`/`{{…}}` markers (those mark interpolated *paths*, not content); `whole_file` still honours `normalize`, so it sits between a query-extract and a no-normalize `identical`. `allow_missing_target` controls absent files/values. `equals` requires the `source` to extract **exactly one** value; to pull the latest entry from a multi-match file (e.g. the newest version in a multi-release `CHANGELOG.md`), anchor the regex so it captures only the first match — `regex: '(?s)\A.*?## (\d+\.\d+\.\d+)'` (`(?s)` makes `.` cross newlines, `\A` anchors at the start, `.*?` reaches the first heading lazily) yields a single value (a leading `## Unreleased` is skipped because the version pattern doesn't match it). The released `cross_file_value_equals` is a **byte-compatible alias** (`relation` defaults to `equals`). Cross-file.

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

# identical — each crate README must mirror the workspace README byte-for-byte
- id: readme-mirrors-root
  kind: cross_file
  source:  { file: README.md }
  targets: { files: "crates/*/README.md" }
  relation: identical
  level: error

# resolves — every declared workspace member path must exist on disk
- id: workspace-members-exist
  kind: cross_file
  source: { file: Cargo.toml, extract: { toml: "$.workspace.members[*]" } }
  relation: resolves
  level: error
```

