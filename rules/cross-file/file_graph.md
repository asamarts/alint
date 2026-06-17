---
title: 'file_graph'
description: 'Assemble the repo''s *file → file* reference graph and assert a global structural property the 1-level cross-file kinds can''t express.'
sidebar:
  order: 5
---

Assemble the repo's *file → file* reference graph and assert a global structural property the 1-level cross-file kinds can't express. `nodes` (a glob) selects the graph's files. The `edges` block takes one of two extractors: `from_content` (extract one reference per match — `extract` is the same one-of as `registry_paths_resolve`: `toml` / `json` / `yaml` JSONPath, `lines`, `regex` capture group 1 — then `resolve` it to a path, `relative_to_file` default or `relative_to_repo_root`) for the reference-graph modes, or `derive_target` (`{ from: <regex on the node path>, to: <template, e.g. $1.pb.go> }`) for the `fresh` codegen-freshness mode **or** the `no_dangling` derived-sibling-existence mode. Bare module names, absolute paths, URLs, and computed/interpolated references are **dropped, not mis-resolved** (resolving module *names* is the package-graph non-goal — nodes stay path-based). `require` is a closed set — three bare-string modes and three configured map modes: `acyclic` (no dependency cycle among the nodes, each reported once as a rotation-canonical path list); `no_dangling` (every path-shaped edge must resolve to a path that exists on disk — the doc-cross-link / generic `markdown_paths_resolve` integrity check; with `edges.derive_target` it instead asserts each node's *derived* sibling exists, e.g. every `licenses/X-LICENSE.txt` needs an `X-NOTICE.txt`); `no_orphans` (no node is unreferenced by another node, except those matching a `roots:` glob — the registry / staging orphan detector); `{ forbidden_edges: [{ from, to }] }` (one violation per edge whose source matches `from` and resolved target matches `to` — the whole-repo layering firewall, where `import_gate` is the cheap per-file version); `{ no_orphans: { roots: [...] } }` (the `no_orphans` form with declared entry points); and `{ fresh: { hash, marker } }` (needs `edges.derive_target`: the generated file must embed the source's current `hash` digest, captured by `marker` group 1 — content-hash, never mtime; the alint-native form of generate-then-`git diff`, with no generator run). Pure-parse and extraction-based: it never shells out. Cross-file (whole-index).

```yaml
# Layering: domain code must not reach into infra (file → file).
- id: domain-not-depend-on-infra
  kind: file_graph
  nodes: "src/**/*.ts"
  edges:
    from_content:
      extract: { regex: 'from\s+"(\.[^"]+)"' }
      resolve: relative_to_file
  require:
    forbidden_edges:
      - { from: "src/domain/**", to: "src/infra/**" }
  level: error

# Acyclicity: the clearest capability gap — no current kind detects cycles.
- id: no-proto-import-cycles
  kind: file_graph
  nodes: "proto/**/*.proto"
  edges:
    from_content:
      extract: { regex: 'import\s+"([^"]+)"' }
      resolve: relative_to_repo_root
  require: acyclic

# Integrity: every doc cross-link resolves, and no doc is unreferenced
# except the declared entry points.
- id: docs-links-resolve
  kind: file_graph
  nodes: "docs/**/*.md"
  edges:
    from_content:
      extract: { regex: '\]\((\.[^)]+\.md)\)' }
      resolve: relative_to_file
  require: no_dangling

- id: no-orphan-docs
  kind: file_graph
  nodes: "docs/**/*.md"
  edges:
    from_content:
      extract: { regex: '\]\((\.[^)]+\.md)\)' }
      resolve: relative_to_file
  require:
    no_orphans:
      roots: ["docs/index.md", "docs/README.md"]

# Freshness: each generated *.pb.go must embed the sha256 of its .proto
# source (the alint-native, no-spawn form of `make gen && git diff`).
- id: generated-stays-fresh
  kind: file_graph
  nodes: "proto/**/*.proto"
  edges:
    derive_target:
      from: '(.*)\.proto'
      to: '$1.pb.go'
  require:
    fresh:
      hash: sha256
      marker: 'sha256:([0-9a-f]{64})'
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `edges` | object | yes |  | Exactly one edge extractor: `from_content` (content references → the reference-graph modes) or `derive_target` (a name template → `fresh` codegen-freshness or `no_dangling` derived-sibling existence). |
| `nodes` | string | yes |  | Glob selecting the graph's node files. |
| `require` | one of `acyclic` \| `no_dangling` \| `no_orphans` or object | yes |  | A bare-string mode (`acyclic` \| `no_dangling` \| `no_orphans`), or a map for a configured mode (`{ forbidden_edges: [{from,to}] }`, `{ no_orphans: { roots: [...] } }`, or `{ fresh: { hash, marker } }`). `no_dangling` = every path-shaped edge resolves to an existing path (with `edges.derive_target`, each node's derived sibling must exist — e.g. every `X-LICENSE.txt` needs an `X-NOTICE.txt`); `no_orphans` = no node is unreferenced except those matching a `roots` glob; `fresh` (needs `edges.derive_target`) = the derived output carries the source's current `hash` digest, captured by `marker` (group 1). |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
