---
title: Structured queries
description: "One JSONPath query language reads a value out of any of eight config formats, because alint parses each into one common Value tree first, with equals, matches, and absent ops."
sidebar:
  order: 11
---

alint reads *inside* config files, not just at them. It parses JSON, YAML, TOML, XML, dotenv, properties, INI, and HCL into one common Value tree, then runs a single RFC 9535 JSONPath query over that tree. The payoff: the same query mental model covers all eight formats. A `host` nested under a `server` table is read by `$.server.host` whether the file is JSON, YAML, TOML, or XML, even though those files share not one character of syntax.

<svg class="alint-sq" viewBox="0 0 460 532" role="img" aria-labelledby="sq-t sq-d" xmlns="http://www.w3.org/2000/svg">
<title id="sq-t">A JSONPath dissected, the same query resolving across five formats, and all eight supported formats</title>
<desc id="sq-d">A path like $.server.host breaks into a root token, a member token, and a leaf token, with extra selectors for index, wildcard, recursive descent, and bracketed keys. The one query $.server.host resolves the value db1 from json, yaml, toml, and xml, while a flat format like dotenv uses the whole key SERVER_HOST as the path. alint parses all eight formats -- json, yaml, toml, xml, ini, dotenv, properties, hcl -- into one Value tree, then asserts with equals, matches, or absent.</desc>
<style>
  .alint-sq { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-sq { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-sq { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-sq .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-sq .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-sq .tx { fill:var(--tx); } .alint-sq .mut { fill:var(--mut); } .alint-sq .ac { fill:var(--ac); }
  .alint-sq .card { fill:var(--card); stroke:var(--bd); stroke-width:1.2; }
  .alint-sq .tok, .alint-sq .fmt { fill:var(--card); stroke:var(--ac); stroke-width:1.3; }
  .alint-sq .chip { fill:var(--card); stroke:var(--bd); stroke-width:1.2; }
  .alint-sq .scan { animation:sqscan 3.2s ease-in-out infinite; }
  .alint-sq .pp { animation:sqpulse 2.8s ease-in-out infinite; }
  .alint-sq .d1 { animation-delay:.3s; } .alint-sq .d2 { animation-delay:.6s; } .alint-sq .d3 { animation-delay:.9s; } .alint-sq .d4 { animation-delay:1.2s; }
  @keyframes sqscan { 0%{transform:translateX(0);opacity:0} 12%{opacity:1} 88%{opacity:1} 100%{transform:translateX(158px);opacity:0} }
  @keyframes sqpulse { 0%,100%{opacity:1} 50%{opacity:.4} }
  @media (prefers-reduced-motion:reduce){ .alint-sq .scan{animation:none;opacity:0} .alint-sq .pp{animation:none} }
</style>
<text class="ui ac" x="18" y="16">one query, any format</text>
<text class="ui mut" x="442" y="16" text-anchor="end">RFC 9535 JSONPath</text>
<rect class="card" x="18" y="26" width="424" height="118" rx="12"/>
<text class="ui ac" x="32" y="46">anatomy of a path</text>
<rect class="tok" x="151" y="56" width="26" height="24" rx="6"/><text class="tag tx" x="164" y="73" text-anchor="middle">$</text>
<rect class="tok" x="185" y="56" width="64" height="24" rx="6"/><text class="tag tx" x="217" y="73" text-anchor="middle">.server</text>
<rect class="tok" x="257" y="56" width="52" height="24" rx="6"/><text class="tag tx" x="283" y="73" text-anchor="middle">.host</text>
<circle class="scan" cx="151" cy="86" r="3" fill="var(--ac)"/>
<text class="tag mut" x="164" y="100" text-anchor="middle">root</text>
<text class="tag mut" x="217" y="100" text-anchor="middle">member</text>
<text class="tag mut" x="283" y="100" text-anchor="middle">leaf</text>
<line x1="32" y1="112" x2="428" y2="112" stroke="var(--bd)" stroke-width="1"/>
<text class="tag mut" x="32" y="131">also: [0] nth, [*] all, .. any depth, ['a-b'] odd keys</text>
<text class="ui ac" x="18" y="168">the same query, any file shape</text>
<rect class="fmt" x="18" y="178" width="52" height="20" rx="6"/><text class="tag ac" x="44" y="192" text-anchor="middle">json</text>
<text class="tag mut" x="80" y="192">{"server":{"host":<tspan fill="#22c55e">"db1"</tspan>}}</text>
<text class="tag ac pp" x="442" y="192" text-anchor="end">$.server.host</text>
<rect class="fmt" x="18" y="206" width="52" height="20" rx="6"/><text class="tag ac" x="44" y="220" text-anchor="middle">yaml</text>
<text class="tag mut" x="80" y="220">server: {host: <tspan fill="#22c55e">db1</tspan>}</text>
<text class="tag ac pp d1" x="442" y="220" text-anchor="end">$.server.host</text>
<rect class="fmt" x="18" y="234" width="52" height="20" rx="6"/><text class="tag ac" x="44" y="248" text-anchor="middle">toml</text>
<text class="tag mut" x="80" y="248">server.host = <tspan fill="#22c55e">"db1"</tspan></text>
<text class="tag ac pp d2" x="442" y="248" text-anchor="end">$.server.host</text>
<rect class="fmt" x="18" y="262" width="52" height="20" rx="6"/><text class="tag ac" x="44" y="276" text-anchor="middle">xml</text>
<text class="tag mut" x="80" y="276">&lt;server&gt;&lt;host&gt;<tspan fill="#22c55e">db1</tspan>&lt;/host&gt;</text>
<text class="tag ac pp d3" x="442" y="276" text-anchor="end">$.server.host</text>
<rect class="fmt" x="18" y="290" width="52" height="20" rx="6"/><text class="tag ac" x="44" y="304" text-anchor="middle">dotenv</text>
<text class="tag mut" x="80" y="304">SERVER_HOST=<tspan fill="#22c55e">db1</tspan></text>
<text class="tag ac pp d4" x="442" y="304" text-anchor="end">$.SERVER_HOST</text>
<text class="tag" x="230" y="328" text-anchor="middle" fill="#f59e0b">dotenv and properties are flat: the key is the path</text>
<text class="ui ac" x="18" y="356">eight parsers, one Value tree</text>
<rect class="fmt" x="22" y="366" width="98" height="24" rx="7"/><text class="tag ac" x="71" y="382" text-anchor="middle">json</text>
<rect class="fmt" x="128" y="366" width="98" height="24" rx="7"/><text class="tag ac" x="177" y="382" text-anchor="middle">yaml</text>
<rect class="fmt" x="234" y="366" width="98" height="24" rx="7"/><text class="tag ac" x="283" y="382" text-anchor="middle">toml</text>
<rect class="fmt" x="340" y="366" width="98" height="24" rx="7"/><text class="tag ac" x="389" y="382" text-anchor="middle">xml</text>
<rect class="fmt" x="22" y="396" width="98" height="24" rx="7"/><text class="tag ac" x="71" y="412" text-anchor="middle">dotenv</text>
<rect class="fmt" x="128" y="396" width="98" height="24" rx="7"/><text class="tag ac" x="177" y="412" text-anchor="middle">properties</text>
<rect class="fmt" x="234" y="396" width="98" height="24" rx="7"/><text class="tag ac" x="283" y="412" text-anchor="middle">ini</text>
<rect class="fmt" x="340" y="396" width="98" height="24" rx="7"/><text class="tag ac" x="389" y="412" text-anchor="middle">hcl</text>
<text class="tag mut" x="230" y="436" text-anchor="middle">JSON, YAML, TOML, HCL keep number and boolean types</text>
<text class="tag mut" x="230" y="454" text-anchor="middle">XML, INI, dotenv, properties give string-typed leaves</text>
<text class="ui ac" x="18" y="484">then assert</text>
<rect class="chip" x="18" y="494" width="134" height="26" rx="6"/><text class="tag tx" x="85" y="511" text-anchor="middle">equals "db1"</text>
<rect class="chip" x="163" y="494" width="134" height="26" rx="6"/><text class="tag tx" x="230" y="511" text-anchor="middle">matches /db\d/</text>
<rect class="chip" x="308" y="494" width="134" height="26" rx="6"/><text class="tag tx" x="375" y="511" text-anchor="middle">absent</text>
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

Pin two versions the same way across two formats: the Node engine in `package.json` (JSON) and the Python floor in `pyproject.toml` (TOML), reaching a dashed key with bracket notation.

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
  - id: python-requires-pinned
    kind: toml_path_matches
    paths: ["pyproject.toml"]
    path: "$.project['requires-python']"
    matches: '>='
    level: error
    message: "pyproject.toml must pin requires-python"
```

One JSONPath dialect, two file formats. On a repo where both are unpinned:

```
error  node-engine-pinned      package.json must pin engines.node
error  python-requires-pinned  pyproject.toml must pin requires-python
```

## Going deeper

- [Cross-file rules](/docs/concepts/cross-file-rules/) reuse this query engine through the shared `extract:` extractor.
- [Rules](/docs/rules/) lists every `<format>_path_<op>` kind, its options, and the per-format sharp edges.
- [Configuration](/docs/configuration/) covers `paths:` and the common rule fields.
