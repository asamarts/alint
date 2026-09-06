---
title: Cross-file rules
description: "Rules whose verdict for one file depends on other files: the relational family, file_graph and its require: modes, and cross_file value relations, all sharing the extract: extractor."
sidebar:
  order: 10
---

Most rules judge one file at a time. **Cross-file rules** judge a relationship between files, so the verdict for one file depends on what else is in the tree. That is why they always read the whole index and never run on a `--changed` subset: deleting one file can break an invariant about another the diff never touched.

<svg class="alint-xfile" viewBox="0 0 460 432" role="img" aria-labelledby="xf-t xf-d" xmlns="http://www.w3.org/2000/svg">
<title id="xf-t">file_graph builds a reference graph, then checks it with a require: mode</title>
<desc id="xf-d">A reference graph of four files. Three form an import cycle (auth, db, cache), which the acyclic mode catches; a fourth (util) is unreferenced, which the no_orphans mode catches. The five require: modes are acyclic, forbidden_edges, no_dangling, no_orphans, and fresh.</desc>
<style>
  .alint-xfile { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-xfile { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-xfile { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-xfile .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-xfile .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-xfile .tx { fill:var(--tx); } .alint-xfile .mut { fill:var(--mut); } .alint-xfile .ac { fill:var(--ac); }
  .alint-xfile .node { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-xfile .bad { stroke:#ef4444; stroke-width:1.8; }
  .alint-xfile .edge { fill:none; stroke:#ef4444; stroke-width:2; }
  .alint-xfile .cyc { fill:none; stroke:#ef4444; stroke-width:2; stroke-dasharray:6 5; animation:xfflow 1s linear infinite; }
  .alint-xfile .pill { fill:var(--card); stroke:var(--ac); stroke-width:1.4; }
  @keyframes xfflow { to { stroke-dashoffset:-11; } }
  @media (prefers-reduced-motion:reduce){ .alint-xfile .cyc{animation:none;stroke-dasharray:none} }
  .alint-xfile .head { fill:var(--tx); }
</style>
<defs><marker id="xfarr" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="#ef4444"/></marker></defs>
<text class="ui ac" x="18" y="16">reference graph</text>
<text class="ui mut" x="442" y="16" text-anchor="end">edges parsed from content</text>
<path class="cyc" d="M 268 60 C 330 78, 356 108, 350 132" marker-end="url(#xfarr)"/>
<path class="cyc" d="M 322 158 C 260 172, 200 172, 158 162" marker-end="url(#xfarr)"/>
<path class="cyc" d="M 120 132 C 112 104, 150 78, 190 62" marker-end="url(#xfarr)"/>
<rect class="node bad" x="180" y="34" width="104" height="30" rx="7"/><text class="tag tx" x="232" y="53" text-anchor="middle">auth.rs</text>
<rect class="node bad" x="300" y="134" width="104" height="30" rx="7"/><text class="tag tx" x="352" y="153" text-anchor="middle">db.rs</text>
<rect class="node bad" x="62" y="134" width="104" height="30" rx="7"/><text class="tag tx" x="114" y="153" text-anchor="middle">cache.rs</text>
<rect class="node" x="40" y="34" width="104" height="30" rx="7" stroke-dasharray="4 3"/><text class="tag mut" x="92" y="53" text-anchor="middle">util.rs</text>
<text class="tag" x="92" y="80" text-anchor="middle" fill="#f59e0b">orphan</text>
<text class="tag" x="232" y="104" text-anchor="middle" fill="#ef4444">import cycle</text>
<text class="ui ac" x="18" y="196">require: one property of the graph</text>
<rect class="pill" x="18" y="206" width="128" height="24" rx="12"/><text class="tag ac" x="82" y="222" text-anchor="middle">acyclic</text><text class="tag mut" x="156" y="222">no import cycle</text>
<rect class="pill" x="18" y="238" width="128" height="24" rx="12"/><text class="tag ac" x="82" y="254" text-anchor="middle">forbidden_edges</text><text class="tag mut" x="156" y="254">block edges across a layer</text>
<rect class="pill" x="18" y="270" width="128" height="24" rx="12"/><text class="tag ac" x="82" y="286" text-anchor="middle">no_dangling</text><text class="tag mut" x="156" y="286">every edge resolves to a file</text>
<rect class="pill" x="18" y="302" width="128" height="24" rx="12"/><text class="tag ac" x="82" y="318" text-anchor="middle">no_orphans</text><text class="tag mut" x="156" y="318">every node is referenced</text>
<rect class="pill" x="18" y="334" width="128" height="24" rx="12"/><text class="tag ac" x="82" y="350" text-anchor="middle">fresh</text><text class="tag mut" x="156" y="350">generated files stay current</text>
<text class="tag mut" x="230" y="392" text-anchor="middle">file_graph spans the whole tree, never a changed subset</text>
</svg>

## The relational family

The oldest cross-file rules ask about neighbours. `pair` requires that every file matching `primary` has a matching `partner` (every `foo.c` a `foo.h`). `for_each_dir` and `for_each_file` iterate directories or files and run nested `require:` rules against each, gated by `when_iter:`. `every_matching_has` asserts that every file in a set contains something; `unique_by` forbids duplicate extracted values across files; `dir_contains` and `dir_only_contains` constrain a directory's membership. All of them read the whole index, because the answer for one path depends on the others.

## file_graph

`file_graph` builds a **reference graph** of your files, then asserts one property of it. For the four reference-graph modes the edges are parsed **from file content** (an `import`, an `include`, a link): `acyclic` forbids dependency cycles, `forbidden_edges` blocks an edge that crosses a layering boundary (a firewall), `no_dangling` requires every edge to resolve to a real file, and `no_orphans` requires every node to be referenced (except declared `roots:`). The fifth mode, `fresh`, uses **name-derived** edges instead (a `derive_target` template maps a source to the file it should generate) to assert that a generated file is up to date with its source, checked by content, not mtime, so it holds on a fresh clone.

```yaml
- id: no-import-cycles
  kind: file_graph
  nodes: "src/**/*.rs"
  edges:
    from_content:
      extract: { regex: '^use crate::([\w:]+)' }
      resolve: relative_to_repo_root
  require: acyclic
  level: error
```

## cross_file

`cross_file` asserts a **value relation** between one file's extracted value and one or more targets: `equals`, `subset`, `superset`, `set_equals`, or `identical` (a whole-file byte match). It pulls the values through the shared `extract:` extractor and an optional `normalize`, so you can say "the `engines.node` in every `package.json` equals the one in the root," or "the workspace member list is a superset of what the CI matrix names." (`cross_file_value_equals` is a registered alias with `relation: equals`.)

## The `extract:` extractor

Three of these kinds (and `scope_filter`'s manifest predicates, and `registry_paths_resolve`) share one extractor. It reads a value out of a file in exactly one of three ways: a **structured query** (an RFC 9535 JSONPath, keyed by format: `json`, `yaml`, `toml`, `xml`, `dotenv`, `properties`, `ini`, or `hcl`), a **`lines`** list, or a **`regex`** capture. The structured form is the same machinery [structured queries](/docs/concepts/structured-queries/) use standalone.

## In practice

Forbid import cycles in a Rust crate, the capability no single-file rule can express:

```yaml
version: 1
rules:
  - id: no-import-cycles
    kind: file_graph
    nodes: "src/**/*.rs"
    edges:
      from_content:
        extract: { regex: '^use crate::([\w:]+)' }
        resolve: relative_to_repo_root
    require: acyclic
    level: error
    message: "import cycle: src/auth.rs -> src/db.rs -> src/cache.rs -> src/auth.rs"
```

On a tree where those three modules import in a loop:

```
error  no-import-cycles  import cycle: src/auth.rs -> src/db.rs -> src/cache.rs -> src/auth.rs
```

## Going deeper

- [Structured queries](/docs/concepts/structured-queries/) is the `extract:` JSONPath machinery in depth.
- [Changed mode](/docs/concepts/changed-mode/) explains why these rules stay whole-tree under `--changed`.
- [Rules](/docs/rules/) is the per-kind reference for `file_graph`, `cross_file`, `pair`, and the rest.
