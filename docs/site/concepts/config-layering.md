---
title: Config layering
description: "How one effective config is assembled from drop-ins and nested configs, and how the three interpolation timings resolve values at load, at template expansion, and per violation."
sidebar:
  order: 9
---

Your root `.alint.yml` is rarely the whole config. Drop-ins layer over it, per-directory nested configs add subtree-scoped rules, and three interpolation timings fill in values at three different moments. Knowing which layer resolves when is the difference between a config that behaves and one that surprises you.

<svg class="alint-layer" viewBox="0 0 460 392" role="img" aria-labelledby="layer-t layer-d" xmlns="http://www.w3.org/2000/svg">
<title id="layer-t">The three interpolation timings resolve at three different moments</title>
<desc id="layer-d">A timeline with three stages. At config load, {{env.PKG_ROOT}} resolves to packages. At template expansion, {{vars.dir}} resolves to packages. Per violation, {{ctx.path}} resolves to packages/README.md. Each resolves at its own moment.</desc>
<style>
  .alint-layer { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-layer { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-layer { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-layer .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-layer .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-layer .tx { fill:var(--tx); } .alint-layer .mut { fill:var(--mut); } .alint-layer .ac { fill:var(--ac); }
  .alint-layer .card { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-layer .node { fill:var(--ac); }
  .alint-layer .spine { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:6 6; opacity:.6; animation:layerflow 1s linear infinite; }
  .alint-layer .token { fill:var(--ac); animation:layertok 3.6s cubic-bezier(.5,0,.5,1) infinite; }
  @keyframes layerflow { to { stroke-dashoffset:-12; } }
  @keyframes layertok { 0%{transform:translateY(0);opacity:0} 8%{opacity:1} 90%{opacity:1} 100%{transform:translateY(200px);opacity:0} }
  @media (prefers-reduced-motion:reduce){ .alint-layer .spine{animation:none;stroke-dasharray:none} .alint-layer .token{animation:none;opacity:1;transform:translateY(200px)} }
</style>
<text class="ui ac" x="18" y="16">three interpolation timings</text>
<text class="ui mut" x="442" y="16" text-anchor="end">three moments</text>
<path class="spine" d="M 54 60 V 322"/>
<circle class="node" cx="54" cy="74" r="6"/>
<circle class="node" cx="54" cy="174" r="6"/>
<circle class="node" cx="54" cy="274" r="6"/>
<circle class="token" cx="54" cy="74" r="4.5"/>
<rect class="card" x="82" y="52" width="338" height="76" rx="10"/>
<text class="ui ac" x="98" y="74">config load</text>
<text class="tag tx" x="98" y="98">{{env.PKG_ROOT}} -&gt; packages</text>
<text class="tag mut" x="98" y="118">from the process environment; local configs only</text>
<rect class="card" x="82" y="152" width="338" height="76" rx="10"/>
<text class="ui ac" x="98" y="174">template expansion</text>
<text class="tag tx" x="98" y="198">{{vars.dir}} -&gt; packages</text>
<text class="tag mut" x="98" y="218">only inside a templates: body</text>
<rect class="card" x="82" y="252" width="338" height="76" rx="10"/>
<text class="ui ac" x="98" y="274">per violation</text>
<text class="tag tx" x="98" y="298">{{ctx.path}} -&gt; packages/README.md</text>
<text class="tag mut" x="98" y="318">in a rule's message, once per finding</text>
<text class="tag mut" x="230" y="362" text-anchor="middle">each resolves at its own moment; no layer sees another's values</text>
</svg>

## Drop-ins: `.alint.d/`

When a `.alint.d/` directory sits next to your root `.alint.yml`, alint discovers every `*.yml` (or `*.yaml`) inside it and merges them in **alphabetical order, last wins** on a field-level conflict. It is the `/etc/*.d/` pattern applied to config: ops layer `50-policy.yml` through provisioning, a developer gitignores `99-local.yml`. Each drop-in is a complete config (its own `version: 1`) and can add rules, override existing ones by id, add `extends:`, or layer more `facts:` and `vars:`.

Drop-ins are **trust-equivalent to your root config**: they live in the same workspace under your control, so they may declare spawning rules and `custom:` facts, unlike anything reached through `extends:`. Only the root config gets `.alint.d/` discovery; a config reached via `extends:` does not carry its own drop-ins.

## Nested configs

Opt in with `nested_configs: true` in the root config, and alint walks the tree (respecting `.gitignore` and `ignore:`) and picks up a `.alint.yml` in any subdirectory. A nested config's rules are **added, not overridden**, and each rule's path-like scope is **auto-prefixed with that subtree**, so a rule in `packages/web/.alint.yml` only ever looks at `packages/web/`.

The guardrails keep nesting predictable: a nested config may declare only `version:` and `rules:`; every nested rule needs at least one scope field; absolute and `..`-escaping paths are rejected; and a duplicate rule `id` anywhere is a load error, never a silent override. Nesting is untrusted in the same way an `extends:`'d ruleset is (no spawning rules), and only the top-level config may turn it on, one level deep.

## Three interpolation timings

The same `{{...}}` syntax resolves at three distinct times, and each layer only ever sees its own inputs:

- **`{{env.X}}`** resolves at **config load**, from the process environment, in local config files only. A `{{env.X | default('...')}}` filter supplies a fallback.
- **`{{vars.X}}`** resolves when a **`templates:` body expands** into its instances. A plain, non-template rule does not expand `{{vars.X}}` in its fields; a bare `when: vars.X` reads a top-level var directly.
- **`{{ctx.X}}`** resolves **per violation**, inside a rule's `message`, so each finding can name its own file or match.

Because they resolve at different moments, they never cross: `{{ctx.path}}` is meaningless at load, and `{{env.X}}` is long since resolved by the time a violation is formatted.

## In practice

One config threads all three timings in order, an env value feeding a template var, the var scoping a path, and the violation naming it:

```yaml
version: 1
templates:
  - id: dir-readme
    kind: file_exists
    paths: ["{{vars.dir}}/README.md"]        # {{vars}} fills at template expansion
    level: error
    message: "{{ctx.path}} is required"        # {{ctx}} fills per violation
rules:
  - extends_template: dir-readme
    id: pkg-readme
    vars: { dir: "{{env.PKG_ROOT | default('packages')}}" }   # {{env}} fills at load
```

With `PKG_ROOT` unset and no `packages/README.md`, the default resolves at load, the template expands to `packages/README.md`, and the violation fills the message:

```
error  pkg-readme  packages/README.md is required
```

## Going deeper

- [The config model](/docs/concepts/the-config-model/) is the whole assembly picture these layers feed into.
- [Composition and trust](/docs/concepts/composition-and-trust/) covers `extends:`, the other way configs combine.
- [Variable interpolation](/docs/concepts/variable-interpolation/) and [Rule templates](/docs/concepts/templates/) are the field-level references for the timings above.
