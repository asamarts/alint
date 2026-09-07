---
title: Concepts
description: A map of alint's model, from the rule record and how files are targeted, through composition and cross-file rules, to fixing, baselines, and the agent surface.
sidebar:
  order: 1
---

The Concepts section is the conceptual foundation that the rest of the docs assume. Skim it once; come back when something downstream confuses you.

Everything here is one idea seen from six angles. You **declare** rules (fact-gated, and composed from other configs); alint **targets** each rule at a set of files, **evaluates** them (per file and across files), and emits a **report** whose exit code gates CI; then **fix**, **baseline**, and an **agent** surface carry it into a real repo. This map is the whole section in one picture, and every box below is a page in the sidebar:

<svg class="alint-map" viewBox="0 0 460 656" role="img" aria-labelledby="map-t map-d" xmlns="http://www.w3.org/2000/svg">
<title id="map-t">The alint mental model: a rule record, and the pipeline it drives</title>
<desc id="map-d">A rule record (id, kind, level, paths, when, fix, message) takes facts and extends as inputs, then drives a five-stage pipeline: target, evaluate, report with an exit code, adopt via fix and baseline, and an agent surface. Each stage maps to a Concepts group.</desc>
<style>
  .alint-map { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-map { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-map { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-map .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-map .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-map .eye { font:700 10px system-ui, -apple-system, sans-serif; letter-spacing:.07em; fill:var(--ac); }
  .alint-map .tx { fill:var(--tx); } .alint-map .mut { fill:var(--mut); } .alint-map .ac { fill:var(--ac); }
  .alint-map .card { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-map .atom { fill:var(--card); stroke:var(--ac); stroke-width:1.8; }
  .alint-map .chip { fill:var(--card); stroke:var(--bd); stroke-width:1.2; }
  .alint-map .node { fill:var(--ac); }
  .alint-map .stub { stroke:var(--bd); stroke-width:1.5; }
  .alint-map .spine { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:6 6; opacity:.55; animation:mapflow 1s linear infinite; }
  .alint-map .feed { fill:none; stroke:var(--ac); stroke-width:1.4; stroke-dasharray:4 4; opacity:.6; animation:mapflow 1s linear infinite; }
  @keyframes mapflow { to { stroke-dashoffset:-12; } }
  @media (prefers-reduced-motion:reduce){ .alint-map .spine,.alint-map .feed{animation:none;stroke-dasharray:none} }
</style>
<text class="ui ac" x="18" y="16">anatomy of a rule</text>
<text class="ui mut" x="442" y="16" text-anchor="end">inputs feed the rule</text>
<text class="eye" x="224" y="26">COMPOSITION</text>
<rect class="chip" x="40" y="30" width="164" height="26" rx="13"/><text class="tag tx" x="122" y="47" text-anchor="middle">facts: has_docs</text>
<rect class="chip" x="220" y="30" width="180" height="26" rx="13"/><text class="tag tx" x="310" y="47" text-anchor="middle">extends: rust@v1</text>
<path class="feed" d="M122 56 V 72"/><path fill="var(--ac)" d="M118 72 L122 78 L126 72 Z"/>
<path class="feed" d="M310 56 V 72"/><path fill="var(--ac)" d="M306 72 L310 78 L314 72 Z"/>
<rect class="atom" x="40" y="80" width="380" height="214" rx="10"/>
<text class="ui ac" x="56" y="102">rule</text>
<text class="tag mut" x="404" y="102" text-anchor="end">the rule model &middot; START HERE</text>
<line x1="56" y1="112" x2="404" y2="112" stroke="var(--bd)" stroke-width="1" opacity=".5"/>
<text class="tag tx" x="56" y="136">id: readme-exists</text><text class="tag mut" x="404" y="136" text-anchor="end">stable handle</text>
<text class="tag tx" x="56" y="161">kind: file_exists</text><text class="tag mut" x="404" y="161" text-anchor="end">which check runs</text>
<text class="tag" x="56" y="186"><tspan fill="var(--tx)">level: </tspan><tspan fill="#ef4444">error</tspan></text><text class="tag mut" x="404" y="186" text-anchor="end">&#8594; exit code</text>
<text class="tag tx" x="56" y="211">paths: [README.md]</text><text class="tag mut" x="404" y="211" text-anchor="end">which files</text>
<text class="tag tx" x="56" y="236">when: facts.has_docs</text><text class="tag mut" x="404" y="236" text-anchor="end">fact gate</text>
<text class="tag tx" x="56" y="261">fix: file_create</text><text class="tag mut" x="404" y="261" text-anchor="end">auto-repair</text>
<text class="tag tx" x="56" y="286">message &middot; policy_url</text><text class="tag mut" x="404" y="286" text-anchor="end">shown on failure</text>
<text class="ui ac" x="18" y="324">the flow it drives</text>
<text class="ui mut" x="442" y="324" text-anchor="end">six ideas, one pipeline</text>
<path class="spine" d="M58 360 V 590"/>
<line class="stub" x1="58" y1="358" x2="84" y2="358"/><circle class="node" cx="58" cy="358" r="5"/>
<line class="stub" x1="58" y1="416" x2="84" y2="416"/><circle class="node" cx="58" cy="416" r="5"/>
<line class="stub" x1="58" y1="474" x2="84" y2="474"/><circle class="node" cx="58" cy="474" r="5"/>
<line class="stub" x1="58" y1="532" x2="84" y2="532"/><circle class="node" cx="58" cy="532" r="5"/>
<line class="stub" x1="58" y1="590" x2="84" y2="590"/><circle class="node" cx="58" cy="590" r="5"/>
<rect class="card" x="84" y="334" width="336" height="48" rx="9"/><text class="eye" x="100" y="355">TARGETING</text><text class="tag tx" x="100" y="373">target: the walk &middot; globs &middot; when: &middot; changed</text>
<rect class="card" x="84" y="392" width="336" height="48" rx="9"/><text class="eye" x="100" y="413">BEYOND SINGLE FILES</text><text class="tag tx" x="100" y="431">evaluate: cross-file + structured queries</text>
<rect class="card" x="84" y="450" width="336" height="48" rx="9"/><text class="eye" x="100" y="471">START HERE &middot; SEVERITY</text><text class="tag tx" x="100" y="489">report: error &middot; warning &middot; info &#8594; exit code</text>
<rect class="card" x="84" y="508" width="336" height="48" rx="9"/><text class="eye" x="100" y="529">ADOPTION</text><text class="tag tx" x="100" y="547">adopt: fix repairs &middot; baseline grandfathers</text>
<rect class="card" x="84" y="566" width="336" height="48" rx="9"/><text class="eye" x="100" y="587">AGENTS</text><text class="tag tx" x="100" y="605">agents: rules &#8594; instruction + fix_command</text>
<text class="tag mut" x="230" y="640" text-anchor="middle">one config in, one gated report out</text>
</svg>

## What alint is

alint is a static Rust binary that lints the *shape* of a repository. Where ESLint lints code and Semgrep lints semantics, alint lints the things in between: required files (READMEs, LICENSEs, SECURITY.md), filename conventions, content patterns, the values inside `package.json` / `Cargo.toml` / GitHub workflows, and cross-file relationships like "every package has a README" or "every header has a matching source file."

## How this section is organized

The section is six groups, each a stage of the mental model above. Read it start to finish for the whole picture, or jump to the group that owns your question:

- **Start here** is the model itself: [how a run works](/docs/concepts/start-here/how-alint-works/), the [config model](/docs/concepts/start-here/the-config-model/), the [kinds, families, and categories](/docs/concepts/start-here/kinds-families-categories/) a rule draws on, and how [severity maps to exit codes](/docs/concepts/start-here/severity-and-exit-codes/).
- **How rules target files** covers what a rule sees: [the walker and git](/docs/concepts/targeting/the-walker-and-git/), [scoping](/docs/concepts/targeting/scoping/) with `paths:`, `when:`, and `scope_filter:`, and [changed mode](/docs/concepts/targeting/changed-mode/) for diffs.
- **Composition and trust** is how configs combine: [extends and the trust boundary](/docs/concepts/composition/composition-and-trust/), the [bundled rulesets](/docs/concepts/composition/bundled-rulesets/) you inherit, and [config layering](/docs/concepts/composition/config-layering/) with drop-ins and nested configs.
- **Beyond single files** is the relational half: [cross-file rules](/docs/concepts/multi-file/cross-file-rules/) and [structured-config queries](/docs/concepts/multi-file/structured-queries/) into JSON, YAML, TOML, and five more formats.
- **Adoption and fixing** lands alint on a real repo: [auto-fix](/docs/concepts/adoption/fixing/) and [baselines](/docs/concepts/adoption/baseline/) that grandfather existing debt.
- **Working with agents** is the [agent surface](/docs/concepts/agents/the-agent-surface/): the same rules become an agent's standing instructions and its fix loop.

The rest of this page is a quick tour of the load-bearing ideas; each links into the group that covers it in depth.

## The rule model

Every rule has:

- **`id`**: a stable, kebab-case identifier. Required. Used to override or disable the rule from a child config.
- **`kind`**: which built-in rule implementation to invoke (e.g. `file_exists`, `json_path_equals`, `for_each_dir`). Required.
- **`level`**: `error`, `warning`, `info`, or `off`. Required.
- **`paths`**: a glob, list of globs, or `{include, exclude}` pair selecting which files the rule applies to. Required for most kinds.
- **`when`**: an expression gating the rule on facts or vars. Optional.
- **`fix`**: a fix-op declaration that turns this rule into an auto-fixable one. Optional.
- **`message`** / **`policy_url`**: display fields shown when the rule fires. Optional.

Plus kind-specific options. For example, `file_min_lines` takes `min_lines: <int>`; `json_path_matches` takes `path` (a JSONPath) and `matches` (a regex).

## Composition

alint configs compose via `extends:`. A config can inherit from local files, HTTPS URLs (with SRI hashes), or `alint://bundled/<name>@<rev>`. Children override inherited rules **field-by-field** by id. You only declare the fields that change. `only:` / `except:` filters narrow the inherited rule set further. `nested_configs: true` opts in to discovering `.alint.yml` files in subdirectories and auto-scoping their rules to the subtree they live in.

`extends:` is also a **trust boundary**: a fetched or bundled ruleset can tighten your checks but is barred from declaring process-spawning rules, `custom:` facts, `allow_out_of_root`, or a `baseline`, so adopting someone's ruleset can never make your machine run their code.

See the [Cookbook](/docs/cookbook/) for composition patterns in practice.

## Facts and `when:` expressions

Facts evaluate properties of the repo *once per run* and surface them as named values:

```yaml
facts:
  - id: has_rust
    any_file_exists: [Cargo.toml]
```

Rules reference facts in `when:` to gate themselves conditionally:

```yaml
- id: rust-snake-case
  when: facts.has_rust
  kind: filename_case
  paths: "src/**/*.rs"
  case: snake
  level: error
```

The `when:` grammar supports boolean logic (`and` / `or` / `not`), comparison (`==` `!=` `<` `<=` `>` `>=`), `in` (list / substring), `matches` (regex), literal types, and `facts.X` / `vars.X` / `iter.X` / `env.X` identifiers. It's deliberately bounded: no arbitrary code, no dynamic evaluation.

## Auto-fix

Rules that opt in declare a `fix:` block. Twelve ops cover content edits (trim whitespace, append newline, normalize line endings, strip BOM / bidi / zero-width, collapse blank lines) and path-level changes (create / remove / rename / prepend / append).

```yaml
- id: trim-trailing-whitespace
  kind: no_trailing_whitespace
  paths: "**/*.md"
  level: info
  fix:
    file_trim_trailing_whitespace: {}
```

Preview with `alint fix --dry-run`; apply with `alint fix`. Content-editing ops honour `fix_size_limit` (default 1 MiB) and skip oversize files rather than rewriting them.

## Output formats

Eight formats: `human` (default; colorized; grouped by file), `json` (stable schema), `sarif` (SARIF 2.1.0 for GitHub Code Scanning), `github` (`::error::` / `::warning::` workflow commands for inline PR annotations), `markdown` (PR-comment tables), `junit` (CI test-report shape), `gitlab` (Code Quality), `agent` (LLM-shaped JSON with per-violation `agent_instruction`).

```bash
alint check --format json --compact
```

The `--compact` flag flips human output to one line per violation, suitable for piping to editors / grep / `wc -l`.
