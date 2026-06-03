---
title: 'file_graph'
description: 'Assemble the repo''s *file → file* reference graph and assert a global structural property the 1-level cross-file kinds can''t express.'
sidebar:
  order: 5
---

Assemble the repo's *file → file* reference graph and assert a global structural property the 1-level cross-file kinds can't express. `nodes` (a glob) selects the graph's files; `edges.from_content` extracts one reference per match — `extract` is the same one-of as `registry_paths_resolve` (`toml` / `json` / `yaml` JSONPath, `lines`, `regex` capture group 1) — and `resolve`s it to a path (`relative_to_file`, default, or `relative_to_repo_root`). Bare module names, absolute paths, URLs, and computed/interpolated references are **dropped, not mis-resolved** (resolving module *names* is the package-graph non-goal — nodes stay path-based). `require` is a closed set — three bare-string modes and two configured map modes: `acyclic` (no dependency cycle among the nodes, each reported once as a rotation-canonical path list); `no_dangling` (every path-shaped edge must resolve to a path that exists on disk — the doc-cross-link / generic `markdown_paths_resolve` integrity check); `no_orphans` (no node is unreferenced by another node, except those matching a `roots:` glob — the registry / staging orphan detector); `{ forbidden_edges: [{ from, to }] }` (one violation per edge whose source matches `from` and resolved target matches `to` — the whole-repo layering firewall, where `import_gate` is the cheap per-file version); and `{ no_orphans: { roots: [...] } }` (the `no_orphans` form with declared entry points). Pure-parse and extraction-based: it never shells out. Cross-file (whole-index).

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
```

