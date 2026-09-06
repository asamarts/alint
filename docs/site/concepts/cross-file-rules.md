---
title: Cross-file rules
description: "Rules whose verdict for one file depends on other files: the relational rules, file_graph and its require: modes, cross_file value relations, and the shared extract: extractor."
sidebar:
  order: 10
---

Most rules judge one file at a time. **Cross-file rules** judge a relationship between files, so the verdict for one file depends on what else is in the tree. That is why they always read the whole index and never run on a `--changed` subset: deleting one file can break an invariant about another the diff never touched.

<svg class="alint-xfile" viewBox="0 0 460 432" role="img" aria-labelledby="xf-t xf-d" xmlns="http://www.w3.org/2000/svg">
<title id="xf-t">file_graph builds a reference graph, then checks it with a require: mode</title>
<desc id="xf-d">A reference graph of four proto files. Three form an import cycle (order, item, user), which the acyclic mode catches; a fourth (util) is unreferenced, which the no_orphans mode catches. The five require: modes are acyclic, forbidden_edges, no_dangling, no_orphans, and fresh.</desc>
<style>
  .alint-xfile { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-xfile { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-xfile { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-xfile .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-xfile .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-xfile .tx { fill:var(--tx); } .alint-xfile .mut { fill:var(--mut); } .alint-xfile .ac { fill:var(--ac); }
  .alint-xfile .node { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-xfile .bad { stroke:#ef4444; stroke-width:1.8; }
  .alint-xfile .cyc { fill:none; stroke:#ef4444; stroke-width:2; stroke-dasharray:6 5; animation:xfflow 1s linear infinite; }
  .alint-xfile .pill { fill:var(--card); stroke:var(--ac); stroke-width:1.4; }
  @keyframes xfflow { to { stroke-dashoffset:-11; } }
  @media (prefers-reduced-motion:reduce){ .alint-xfile .cyc{animation:none;stroke-dasharray:none} }
</style>
<defs><marker id="xfarr" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="#ef4444"/></marker></defs>
<text class="ui ac" x="18" y="16">reference graph</text>
<text class="ui mut" x="442" y="16" text-anchor="end">edges parsed from content</text>
<path class="cyc" d="M 286 62 C 320 82, 342 108, 336 130" marker-end="url(#xfarr)"/>
<path class="cyc" d="M 300 150 C 250 172, 210 172, 170 150" marker-end="url(#xfarr)"/>
<path class="cyc" d="M 114 132 C 108 100, 150 74, 188 66" marker-end="url(#xfarr)"/>
<rect class="node bad" x="176" y="34" width="112" height="30" rx="7"/><text class="tag tx" x="232" y="53" text-anchor="middle">order.proto</text>
<rect class="node bad" x="298" y="134" width="112" height="30" rx="7"/><text class="tag tx" x="354" y="153" text-anchor="middle">item.proto</text>
<rect class="node bad" x="58" y="134" width="112" height="30" rx="7"/><text class="tag tx" x="114" y="153" text-anchor="middle">user.proto</text>
<rect class="node" x="36" y="34" width="112" height="30" rx="7" stroke-dasharray="4 3"/><text class="tag mut" x="92" y="53" text-anchor="middle">util.proto</text>
<text class="tag" x="92" y="80" text-anchor="middle" fill="#f59e0b">orphan</text>
<text class="tag" x="232" y="104" text-anchor="middle" fill="#ef4444">import cycle</text>
<text class="ui ac" x="18" y="196">require: one property of the graph</text>
<rect class="pill" x="18" y="206" width="128" height="24" rx="12"/><text class="tag ac" x="82" y="222" text-anchor="middle">acyclic</text><text class="tag mut" x="156" y="222">no import cycle</text>
<rect class="pill" x="18" y="238" width="128" height="24" rx="12"/><text class="tag ac" x="82" y="254" text-anchor="middle">forbidden_edges</text><text class="tag mut" x="156" y="254">block edges across a layer</text>
<rect class="pill" x="18" y="270" width="128" height="24" rx="12"/><text class="tag ac" x="82" y="286" text-anchor="middle">no_dangling</text><text class="tag mut" x="156" y="286">every edge resolves to a path</text>
<rect class="pill" x="18" y="302" width="128" height="24" rx="12"/><text class="tag ac" x="82" y="318" text-anchor="middle">no_orphans</text><text class="tag mut" x="156" y="318">every node is referenced</text>
<rect class="pill" x="18" y="334" width="128" height="24" rx="12"/><text class="tag ac" x="82" y="350" text-anchor="middle">fresh</text><text class="tag mut" x="156" y="350">generated files stay current</text>
<text class="tag mut" x="230" y="392" text-anchor="middle">file_graph spans the whole tree, never a changed subset</text>
</svg>

## The relational rules

The oldest cross-file rules ask about neighbours. `pair` requires that every file matching `primary` has a matching `partner` (every `foo.c` a `foo.h`). `for_each_dir` and `for_each_file` iterate directories or files and run nested `require:` rules against each, gated by `when_iter:`. `every_matching_has` runs its nested `require:` rules against every file or directory that matches `select:`, a lightweight sibling of `pair`. `unique_by` forbids two files from sharing a **path-template key** (the default `{basename}` catches the same filename in different directories, a case-insensitive-filesystem hazard). `dir_contains` and `dir_only_contains` constrain a directory's membership. All of them read the whole index, because the answer for one path depends on the others.

## file_graph

`file_graph` builds a **reference graph** of your files, then asserts one property of it. The four reference-graph modes take edges parsed **from file content**: `acyclic` forbids dependency cycles, `forbidden_edges` blocks an edge that crosses a layering boundary (a firewall), `no_dangling` requires every edge to resolve to an existing path (a file or a directory), and `no_orphans` requires every node to be referenced (except declared `roots:`). The fifth mode, `fresh`, takes **name-derived** edges instead: a `derive_target` template maps a source to the file it should generate, and the rule asserts the generated file is up to date, checked by content rather than mtime so it holds on a fresh clone. (`no_dangling` accepts derived edges too, asserting each derived sibling exists.)

Edges resolve **as paths**, which is the point: a `from_content` regex captures a reference, and only genuinely path-shaped ones are followed. A bare module name (`crate::db`), an absolute path, a URL, or a `..`-escaping ref is dropped, not chased, so `file_graph` stays a file graph and never tries to be a package resolver. `resolve:` picks the base: `relative_to_file` (the default; only refs starting with `.`) or `relative_to_repo_root`.

## cross_file

`cross_file` asserts a **relation** between one file's extracted value and one or more targets. Six relations: the value relations `equals`, `subset`, `superset`, and `set_equals`; `identical`, a whole-file byte match; and `resolves`, which requires each extracted path to exist on disk. It pulls values through the shared `extract:` extractor and an optional `normalize`, so `normalize: semver-minor` reconciles `4.36-dev`, `4.36.0`, and `>=4.36` to the same `4.36` band, letting the `engines.node` in every `package.json` be checked against the root without version-string noise. `allow_missing_target` tolerates an absent target, and `cross_file_value_equals` is a registered alias with `relation: equals`.

## The `extract:` extractor

`file_graph` and `cross_file` (plus `registry_paths_resolve` and `scope_filter`'s manifest predicates) share one extractor, which reads a value out of a file in exactly one of four ways: a **structured query** (an RFC 9535 JSONPath, keyed by format: `json`, `yaml`, `toml`, `xml`, `dotenv`, `properties`, `ini`, or `hcl`), a **`lines`** list, a **`regex`** capture, or **`whole_file`** (the entire content as one value, for byte comparisons). The structured form is the same machinery [structured queries](/docs/concepts/structured-queries/) use standalone.

## In practice

Forbid import cycles among Protobuf files, the capability no single-file rule can express:

```yaml
version: 1
rules:
  - id: no-proto-import-cycles
    kind: file_graph
    nodes: "proto/**/*.proto"
    edges:
      from_content:
        extract: { regex: 'import "([^"]+)"' }
        resolve: relative_to_repo_root
    require: acyclic
    level: error
    message: "import cycle: order.proto -> item.proto -> user.proto -> order.proto"
```

On a tree where three protos import in a loop, the rule fires (without a `message:` override, `file_graph` auto-names the cycle for you):

```
error  no-proto-import-cycles  import cycle: order.proto -> item.proto -> user.proto -> order.proto
```

## Going deeper

- [Structured queries](/docs/concepts/structured-queries/) is the `extract:` JSONPath machinery in depth.
- [Changed mode](/docs/concepts/changed-mode/) explains why these rules stay whole-tree under `--changed`.
- [Rules](/docs/rules/) is the per-kind reference for `file_graph`, `cross_file`, `pair`, and the rest.
