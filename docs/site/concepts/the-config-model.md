---
title: The config model
description: "How alint turns one .alint.yml, plus the configs it extends, drops in, and nests, into a single effective config, and then into a report."
sidebar:
  order: 3
---

alint is driven by a small declarative language. You describe the checks you want as **rules**; alint merges them with every other config source in play into one **effective config**, and then reads your repository to turn that config into a report. The root `.alint.yml` is the entry point, not the whole story: alint also reads the configs it `extends:`, any `.alint.d/` drop-ins, per-directory nested configs, and of course every repository file each rule checks.

<svg class="alint-config" viewBox="0 0 460 470" role="img" aria-labelledby="cfg-t cfg-d" xmlns="http://www.w3.org/2000/svg">
<title id="cfg-t">alint assembles one effective config from many sources</title>
<desc id="cfg-d">Four config sources (bundled, extends, root, drop-in) merge by rule id from low to high precedence into one effective config. The bundled ruleset sets readme-exists to warning; the drop-in overrides it to error, which wins. Nested configs add subtree-scoped rules.</desc>
<style>
  .alint-config { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-config { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-config { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-config .mono { font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-config .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-config .tx { fill:var(--tx); } .alint-config .mut { fill:var(--mut); } .alint-config .ac { fill:var(--ac); }
  .alint-config .card { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-config .eff { fill:var(--card); stroke:var(--ac); stroke-width:1.7; }
  .alint-config .old { fill:var(--mut); text-decoration:line-through; }
  .alint-config .flow { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:6 6; opacity:.75; animation:cfgf 1s linear infinite; }
  @keyframes cfgf { to { stroke-dashoffset:-12; } }
  @media (prefers-reduced-motion:reduce){ .alint-config .flow{animation:none;stroke-dasharray:none} }
</style>
<text class="ui ac" x="18" y="15">one effective config, from many sources</text>
<text class="ui mut" x="18" y="32">low to high precedence &#8595;</text>
<rect class="card" x="20" y="42"  width="420" height="44" rx="8"/><rect x="20" y="42"  width="6" height="44" rx="2" fill="#3b82f6"/>
<text class="ui" x="36" y="60" fill="#3b82f6">bundled</text><text class="mono tx" x="36" y="78">oss-baseline@v1</text><text class="mono mut" x="424" y="69" font-size="11" text-anchor="end">readme-exists: warning</text>
<rect class="card" x="20" y="98"  width="420" height="44" rx="8"/><rect x="20" y="98"  width="6" height="44" rx="2" fill="#f59e0b"/>
<text class="ui" x="36" y="116" fill="#f59e0b">extends</text><text class="mono tx" x="36" y="134">./team.yml</text><text class="mono mut" x="424" y="125" font-size="11" text-anchor="end">+ team rules</text>
<rect class="card" x="20" y="154" width="420" height="44" rx="8"/><rect x="20" y="154" width="6" height="44" rx="2" fill="#4f46e5"/>
<text class="ui ac" x="36" y="172">root</text><text class="mono tx" x="36" y="190">.alint.yml</text><text class="mono mut" x="424" y="181" font-size="11" text-anchor="end">+ your rules</text>
<rect class="card" x="20" y="210" width="420" height="44" rx="8"/><rect x="20" y="210" width="6" height="44" rx="2" fill="#7c3aed"/>
<text class="ui" x="36" y="228" fill="#7c3aed">drop-in</text><text class="mono tx" x="36" y="246">99-local.yml</text><text class="mono" x="424" y="237" font-size="11" text-anchor="end" fill="#7c3aed">readme-exists: error</text>
<path class="flow" d="M 230 254 V 272"/><text class="ui mut" x="242" y="267">merge by id</text>
<text class="ui ac" x="20" y="290">effective config</text>
<rect class="eff" x="20" y="298" width="420" height="156" rx="12"/>
<text class="mono ac" x="36" y="326" font-weight="700">readme-exists</text>
<text class="mono tx" x="36" y="352">kind: file_exists</text>
<text class="mono tx" x="36" y="378">level: <tspan class="old">warning</tspan> <tspan class="tx">&#8594;</tspan> <tspan class="ac" font-weight="700">error</tspan></text>
<line x1="36" y1="396" x2="424" y2="396" stroke="var(--bd)" stroke-width="1" opacity=".5"/>
<text class="ui mut" x="36" y="418">drop-in wins (highest precedence)</text>
<text class="ui mut" x="36" y="440">+ nested configs add subtree-scoped rules</text>
</svg>

## A rule is a record

The atom of the language is the rule record. Three fields are always required: `id` (a stable kebab-case name), `kind` (which built-in check to run), and `level` (`error`, `warning`, `info`, or `off`). The rest are optional: `paths` (which files, as a glob, a list, or an `{include, exclude}` pair), `when` (an expression that gates the rule on facts), `fix` (one of twelve repair ops), `message`, `scope_filter`, and any fields specific to the `kind`.

```yaml
- id: readme-exists      # names it (stable; used to override or disable)
  kind: file_exists      # which built-in check
  when: facts.has_rust   # gate: only in a Rust project
  paths: [README.md]     # which path the rule is about
  level: error           # severity
  message: "README.md is required at the repo root"
```

alint reads those fields in a fixed order, and that order is the pipeline in miniature. `when` is checked first, against facts computed once per run, so a gated-out rule is dropped before a single file is read. `paths` then selects the files, `kind` runs its check, and `level` and `message` shape what lands in the report. The `when:` expression is a deliberately bounded little language, with boolean logic, comparisons, `in`, and `matches` over four namespaces (`facts.`, `vars.`, `iter.`, `env.`) and no arbitrary code; a missing fact reads as `null` (falsy), so a rule gated on an absent fact simply never runs.

Around the rules sit the rest of the top-level fields: `extends:` inherits other configs, `vars:` and `facts:` supply values the rules gate and interpolate on, `ignore:` and `respect_gitignore:` shape the walk, `templates:` factor out repeated rule shapes, and a few knobs (`fix_size_limit`, `nested_configs`, `allow_out_of_root`, `baseline`) tune a run. Only `version: 1` is strictly required.

## Many sources, one effective config

Most repositories never write every rule by hand. A config is *assembled* from several sources, and alint merges them into one effective config before anything runs:

- **`extends:`** inherits from other configs, each entry a local file, an `https://` URL pinned by a SHA-256 hash, or a bundled ruleset resolved offline from the binary (`alint://bundled/...`). Entries resolve left to right, each overriding the ones before it, and your own file overrides everything it extends. Fetched and bundled configs are leaf nodes: they cannot themselves declare `extends:`.
- **Drop-ins** (`.alint.d/*.yml`) are discovered next to the root config and merged alphabetically; the last one wins. It is the `/etc/*.d/` pattern for alint: ops layer `50-policy.yml`, a developer gitignores `99-local.yml`.
- **Nested configs** (opt in with `nested_configs: true`) let a subdirectory carry its own `.alint.yml`. Its rules are *added*, with their path scopes automatically prefixed with that subtree, so a rule in `packages/web/.alint.yml` only ever looks at `packages/web/`.

Overrides happen **by rule `id`, field by field**: a later layer that re-declares `readme-exists` with `level: error` changes only that field and inherits `kind`, `paths`, and `message` from below. (Nested structures like a whole `fix:` block replace wholesale rather than deep-merging, so you re-state a `fix:` to change part of it.) The precedence, lowest to highest, is: extended configs (left to right), then your root config, then drop-ins. Nested configs are not an override layer at all: their rules are new, subtree-scoped additions, and a duplicate `id` anywhere is a load error, never a silent override.

Sources are not equally trusted. Your own `.alint.yml` and its drop-ins are trust-equivalent: they may declare `custom:` facts and process-spawning rules (`kind: command` and its siblings). A config reached through `extends:`, especially a fetched or bundled one, cannot: spawning rules, custom facts, `allow_out_of_root`, and `baseline` are all rejected at load if they arrive that way. Adopting someone else's ruleset can tighten your checks; it can never make your machine run their commands.

## From config to verdicts

Assembling the config is only the first half. Once the effective config exists, alint validates it against the schema, then evaluates it: it computes your `facts:` once, in order; drops every rule whose `when:` is false; walks the repository a single time (honoring `.gitignore` and your `ignore:` globs) into one deterministic, sorted index; and dispatches each rule. Cross-file rules scan the whole index; per-file rules run against each matched file, and every file's bytes are read at most once no matter how many rules match it. The violations aggregate into one report. See [How alint works](/docs/concepts/how-it-works/) for that evaluation pipeline in full.

Three interpolation layers thread through both halves, each resolving at a different time: `{{env.X}}` at config load (from the process environment, in local configs only), `{{vars.X}}` when a `templates:` body expands, and `{{ctx.X}}` per violation, inside a rule's `message`.

## In practice

A root config sets a rule at `warning`; a drop-in bumps just its severity:

```yaml
# .alint.yml  (root)
version: 1
rules:
  - id: readme-exists
    kind: file_exists
    paths: [README.md]
    level: warning
    message: "README.md is required at the repo root"
```

```yaml
# .alint.d/99-local.yml  (merged after the root; last layer wins)
version: 1
rules:
  - id: readme-exists
    level: error          # same id, so this re-declares just one field
```

In a repo with no README, `alint check` reports the merged rule:

```
error  readme-exists  README.md is required at the repo root
```

The drop-in supplied only `level`; `kind`, `paths`, and `message` came from the root config, and the drop-in's `error` won because it is the highest-precedence layer.

## Going deeper

- [Configuration](/docs/configuration/) is the field-by-field reference for all twelve top-level fields, every rule field, and the JSON Schema.
- [Drop-in configs](/docs/concepts/drop-ins/) covers `.alint.d/` layering and its trust posture in depth.
- [Variable interpolation](/docs/concepts/variable-interpolation/) details the three interpolation timings and `{{env.X | default(...)}}`.
- [How alint works](/docs/concepts/how-it-works/) traces the assembly-then-evaluation pipeline end to end.
