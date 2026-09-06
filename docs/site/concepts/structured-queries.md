---
title: Structured queries
description: "One JSONPath query language reads a value out of any of eight config formats, because alint parses each into one common Value tree first, with equals, matches, and absent ops."
sidebar:
  order: 11
---

alint reads *inside* config files, not just at them. It parses JSON, YAML, TOML, XML, dotenv, properties, INI, and HCL into one common Value tree, then runs a single JSONPath query language over that tree. So the same `$.engines.node` query works whether the value lives in a `package.json`, a `pyproject.toml`, or an `.ini`, and one mental model covers all eight formats.

<svg class="alint-sq" viewBox="0 0 460 388" role="img" aria-labelledby="sq-t sq-d" xmlns="http://www.w3.org/2000/svg">
<title id="sq-t">Eight config formats parse into one Value tree that a single JSONPath query walks</title>
<desc id="sq-d">Eight formats (json, yaml, toml, xml, dotenv, properties, ini, hcl) parse into one common Value tree. A JSONPath query, $.version, selects the version node, and an op (equals, matches, or absent) checks it.</desc>
<style>
  .alint-sq { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-sq { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-sq { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-sq .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-sq .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-sq .tx { fill:var(--tx); } .alint-sq .mut { fill:var(--mut); } .alint-sq .ac { fill:var(--ac); }
  .alint-sq .chip { fill:var(--card); stroke:var(--bd); stroke-width:1.2; }
  .alint-sq .card { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-sq .hit { fill:var(--card); stroke:var(--ac); stroke-width:1.8; }
  .alint-sq .flow { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:6 6; opacity:.7; animation:sqflow 1s linear infinite; }
  .alint-sq .pulse { animation:sqpulse 2.4s ease-in-out infinite; }
  @keyframes sqflow { to { stroke-dashoffset:-12; } }
  @keyframes sqpulse { 0%,100%{opacity:1} 50%{opacity:.5} }
  @media (prefers-reduced-motion:reduce){ .alint-sq .flow{animation:none;stroke-dasharray:none} .alint-sq .pulse{animation:none} }
</style>
<text class="ui ac" x="18" y="16">eight formats</text>
<text class="ui mut" x="442" y="16" text-anchor="end">one query language</text>
<rect class="chip" x="20" y="26" width="100" height="24" rx="6"/><text class="tag tx" x="70" y="42" text-anchor="middle">json</text>
<rect class="chip" x="124" y="26" width="100" height="24" rx="6"/><text class="tag tx" x="174" y="42" text-anchor="middle">yaml</text>
<rect class="chip" x="236" y="26" width="100" height="24" rx="6"/><text class="tag tx" x="286" y="42" text-anchor="middle">toml</text>
<rect class="chip" x="340" y="26" width="100" height="24" rx="6"/><text class="tag tx" x="390" y="42" text-anchor="middle">xml</text>
<rect class="chip" x="20" y="54" width="100" height="24" rx="6"/><text class="tag tx" x="70" y="70" text-anchor="middle">dotenv</text>
<rect class="chip" x="124" y="54" width="100" height="24" rx="6"/><text class="tag tx" x="174" y="70" text-anchor="middle">properties</text>
<rect class="chip" x="236" y="54" width="100" height="24" rx="6"/><text class="tag tx" x="286" y="70" text-anchor="middle">ini</text>
<rect class="chip" x="340" y="54" width="100" height="24" rx="6"/><text class="tag tx" x="390" y="70" text-anchor="middle">hcl</text>
<path class="flow" d="M 230 82 V 108"/>
<text class="tag mut" x="242" y="100">parse + coerce</text>
<rect class="card" x="60" y="114" width="340" height="120" rx="10"/>
<text class="ui ac" x="76" y="136">one Value tree</text>
<rect class="hit pulse" x="76" y="146" width="308" height="24" rx="6"/><text class="tag tx" x="90" y="162">version: "1.2"</text><text class="tag ac" x="370" y="162" text-anchor="end">matched</text>
<rect class="chip" x="76" y="176" width="308" height="24" rx="6"/><text class="tag mut" x="90" y="192">engines: { node: "20" }</text>
<rect class="chip" x="76" y="206" width="308" height="24" rx="6"/><text class="tag mut" x="90" y="222">deps: [ ... ]</text>
<text class="ui ac" x="60" y="264">$.version</text>
<text class="tag mut" x="150" y="264">walks the tree; the op checks each match</text>
<rect class="chip" x="60" y="276" width="120" height="24" rx="6"/><text class="tag tx" x="120" y="292" text-anchor="middle">equals "1.2"</text>
<rect class="chip" x="188" y="276" width="120" height="24" rx="6"/><text class="tag tx" x="248" y="292" text-anchor="middle">matches /1\./</text>
<rect class="chip" x="316" y="276" width="120" height="24" rx="6"/><text class="tag tx" x="376" y="292" text-anchor="middle">absent</text>
<text class="tag mut" x="230" y="332" text-anchor="middle">one query can match 0, 1, or many nodes; a value keeps its type</text>
</svg>

## Eight formats, one tree

Every structured-query rule is named `<format>_path_<op>`: `json_path_equals`, `toml_path_matches`, `yaml_path_absent`, and so on across the eight formats. Under the hood they all do the same two things: parse the file into one common Value tree (JSON and YAML natively, TOML through serde, XML via an xmltodict-style mapping, and the flat formats as key-value maps), then evaluate an RFC 9535 JSONPath against it. Learn the query language once and it transfers to every format.

## The three ops

- **`equals`** asserts the value at the path equals a literal. The literal keeps its type, so `equals: 8080` matches the number and `equals: "8080"` matches the string; they are not interchangeable.
- **`matches`** applies a regex, and only to **string** values. A numeric or boolean node is not a match target.
- **`absent`** asserts the query selects **nothing**. Any match is a violation. It is the mirror of `equals`/`matches`, for "this key must not be set."

## The two footguns

**Cardinality.** A JSONPath can select zero, one, or many nodes. For `equals` and `matches`, **every** selected node must satisfy the op, and selecting **zero** nodes is itself a "path not found" violation (unless you set `if_present: true`, which makes zero matches pass silently and checks only the nodes that do exist). So `$.deps[*].version` checks every dependency, and a typo'd path fails loudly rather than passing vacuously.

**Keys with dashes or dots.** JSONPath dot notation stops at a dashed or dotted key, so `$.scripts.pre-commit` does not resolve. Use bracket notation for those: `$.scripts['pre-commit']`.

## In practice

Require that `package.json` pins the Node engine to a major line:

```yaml
version: 1
rules:
  - id: node-engine-pinned
    kind: json_path_matches
    paths: ["package.json"]
    path: "$.engines.node"
    matches: '^>=?\d+'
    level: error
    message: "package.json must pin engines.node"
```

On a `package.json` whose `engines.node` is missing or unpinned:

```
error  node-engine-pinned  package.json must pin engines.node
```

## Going deeper

- [Cross-file rules](/docs/concepts/cross-file-rules/) reuse this query engine through the shared `extract:` extractor.
- [Rules](/docs/rules/) lists every `<format>_path_<op>` kind and its options.
- [Configuration](/docs/configuration/) covers `paths:` and the common rule fields.
