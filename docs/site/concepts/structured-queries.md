---
title: Structured queries
description: "One JSONPath query language reads a value out of any of eight config formats, because alint parses each into one common Value tree first, with equals, matches, and absent ops."
sidebar:
  order: 11
---

alint reads *inside* config files, not just at them. It parses JSON, YAML, TOML, XML, dotenv, properties, INI, and HCL into one common Value tree, then runs a single RFC 9535 JSONPath query over that tree. The payoff: the same query mental model covers all eight formats. A top-level `version` of `"1.2"` in a `.json`, a `.toml`, or a `.yaml` file is all read by `$.version`, even though the three files share not one character of syntax.

<svg class="alint-sq" viewBox="0 0 460 404" role="img" aria-labelledby="sq-t sq-d" xmlns="http://www.w3.org/2000/svg">
<title id="sq-t">Three file syntaxes parse into one Value tree that a single JSONPath query walks</title>
<desc id="sq-d">The same value written three ways (JSON, TOML, YAML) parses into one common Value tree with a node version equal to 1.2, which the query $.version reads. An op (equals, matches, or absent) then checks it.</desc>
<style>
  .alint-sq { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-sq { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-sq { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-sq .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-sq .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-sq .tx { fill:var(--tx); } .alint-sq .mut { fill:var(--mut); } .alint-sq .ac { fill:var(--ac); }
  .alint-sq .chip { fill:var(--card); stroke:var(--bd); stroke-width:1.2; }
  .alint-sq .fmt { fill:var(--card); stroke:var(--ac); stroke-width:1.3; }
  .alint-sq .hit { fill:var(--card); stroke:var(--ac); stroke-width:1.8; }
  .alint-sq .flow { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:6 6; opacity:.7; animation:sqflow 1s linear infinite; }
  .alint-sq .pulse { animation:sqpulse 2.4s ease-in-out infinite; }
  @keyframes sqflow { to { stroke-dashoffset:-12; } }
  @keyframes sqpulse { 0%,100%{opacity:1} 50%{opacity:.5} }
  @media (prefers-reduced-motion:reduce){ .alint-sq .flow{animation:none;stroke-dasharray:none} .alint-sq .pulse{animation:none} }
</style>
<text class="ui ac" x="18" y="16">same value, three syntaxes</text>
<text class="ui mut" x="442" y="16" text-anchor="end">one query</text>
<rect class="fmt" x="18" y="26" width="58" height="24" rx="6"/><text class="tag ac" x="47" y="42" text-anchor="middle">json</text>
<rect class="chip" x="84" y="26" width="358" height="24" rx="6"/><text class="tag tx" x="98" y="42">{ "version": "1.2" }</text>
<rect class="fmt" x="18" y="56" width="58" height="24" rx="6"/><text class="tag ac" x="47" y="72" text-anchor="middle">toml</text>
<rect class="chip" x="84" y="56" width="358" height="24" rx="6"/><text class="tag tx" x="98" y="72">version = "1.2"</text>
<rect class="fmt" x="18" y="86" width="58" height="24" rx="6"/><text class="tag ac" x="47" y="102" text-anchor="middle">yaml</text>
<rect class="chip" x="84" y="86" width="358" height="24" rx="6"/><text class="tag tx" x="98" y="102">version: "1.2"</text>
<path class="flow" d="M 200 50 C 200 122, 230 108, 230 140"/>
<path class="flow" d="M 230 80 V 140"/>
<path class="flow" d="M 260 110 C 260 122, 230 122, 230 140"/>
<text class="tag mut" x="245" y="132">parse + coerce</text>
<rect class="hit" x="120" y="146" width="220" height="52" rx="10"/><text class="ui ac" x="134" y="167">one Value tree</text><text class="tag tx" x="134" y="187">version = "1.2"</text>
<path class="flow" d="M 230 198 V 224"/>
<text class="ui ac" x="120" y="242">$.version</text><text class="tag mut" x="210" y="242">reads "1.2" from all three</text>
<rect class="chip" x="18" y="256" width="134" height="26" rx="6"/><text class="tag tx" x="85" y="273" text-anchor="middle">equals "1.2"</text>
<rect class="chip" x="163" y="256" width="134" height="26" rx="6"/><text class="tag tx" x="230" y="273" text-anchor="middle">matches /1\./</text>
<rect class="chip" x="308" y="256" width="134" height="26" rx="6"/><text class="tag tx" x="375" y="273" text-anchor="middle">absent</text>
<text class="tag mut" x="230" y="316" text-anchor="middle">XML nests under its root element: $.pkg.version</text>
<text class="tag mut" x="230" y="338" text-anchor="middle">XML, dotenv, properties, and INI give string-typed leaves</text>
</svg>

## Eight formats, one tree

Every structured-query rule is named `<format>_path_<op>`: `json_path_equals`, `toml_path_matches`, `yaml_path_absent`, and so on across the eight formats. Under the hood they all parse the file into one common Value tree, then evaluate a JSONPath against it. JSON, YAML, and TOML coerce through serde; XML maps in an xmltodict style; dotenv, properties, and INI become key-value maps; and HCL nests blocks by type and labels. The query language is learned once and transfers, but the *path* depends on how each format shapes the tree:

| format | file content | query |
|---|---|---|
| JSON / YAML / TOML / HCL | `version: "1.2"` (in that format) | `$.version` |
| XML | `<pkg><version>1.2</version></pkg>` | `$.pkg.version` |
| INI | `[pkg]` then `version = 1.2` | `$.pkg.version` |
| dotenv | `VERSION=1.2` | `$.VERSION` |
| properties | `version=1.2` | `$.version` |

Four things do not transfer unchanged: **XML wraps everything in its root element**, so the root name is the first path segment; **INI sections are a level** (keys before any section hoist to the top); **dotenv keys keep their casing** (usually upper); and, as below, the flat formats and XML give you **string-typed** leaves.

## The three ops

- **`equals`** asserts the value at the path equals a literal, keeping its type: `equals: 8080` matches the number and `equals: "8080"` matches the string. They are not interchangeable.
- **`matches`** applies a regex, and only to **string** values. A numeric or boolean node is not a match target.
- **`absent`** asserts the query selects **nothing**; any match is a violation. It is the mirror of `equals` / `matches`, for "this key must not be set."

## The footguns

**String-typed leaves.** In JSON, YAML, TOML, and HCL a value keeps its type, so `equals: 8080` matches the number `8080`. But XML, dotenv, properties, and INI have no type system: every leaf is a **string**, so there `equals: 8080` (a number) silently never matches and you must write `equals: "8080"`. This is the one place the "one mental model" leaks, and it bites quietly.

**Cardinality.** A JSONPath can select zero, one, or many nodes. For `equals` and `matches`, **every** selected node must satisfy the op, and selecting **zero** is itself a "path not found" violation, unless you set `if_present: true` (which passes silently on zero matches and checks only the nodes that exist). In XML a single child is a scalar, not a one-element list, so `$.items.item[*]` reads nothing when there is exactly one `<item>`; reach it with recursive descent (`$..item`) instead.

**Keys with dashes or dots.** Dot notation stops at a dashed or dotted key, so `$.scripts.pre-commit` and `$.db.host` (a single dotted properties key) do not resolve. Use bracket notation: `$.scripts['pre-commit']`, `$['db.host']`.

**Parsing.** A `.json` file may carry comments and trailing commas (JSONC, for `tsconfig.json` and friends) and still parses. An empty file parses to `{}`, which matters for `*_path_absent` and `if_present`. And a file that will not parse is one **parse-error violation** for that file, never a silent skip.

## In practice

Require that `package.json` pins the Node engine to a major line, and that `.nvmrc` is not left at a floating tag:

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
- [Rules](/docs/rules/) lists every `<format>_path_<op>` kind, its options, and the per-format sharp edges.
- [Configuration](/docs/configuration/) covers `paths:` and the common rule fields.
