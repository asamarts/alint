---
title: Composition and trust
description: "How extends: merges other configs into yours field-by-field by id, and the trust boundary that lets a fetched or bundled ruleset tighten your checks but never run your machine's code."
sidebar:
  order: 8
---

A config rarely stands alone. `extends:` pulls in other configs and merges their rules into yours, field by field, keyed on rule `id`. But the further a config sits from you, the less it is trusted: your own `.alint.yml` may run commands and reach outside the repo, while anything reached through `extends:` cannot. Adopting someone else's ruleset can only ever tighten your checks; it can never make your machine run their code.

<svg class="alint-trust" viewBox="0 0 460 388" role="img" aria-labelledby="trust-t trust-d" xmlns="http://www.w3.org/2000/svg">
<title id="trust-t">extends merges rules by id, but the trust boundary blocks spawning rules and other privileged fields</title>
<desc id="trust-d">Your top-level config is trusted and may spawn commands, add custom facts, read out of root, and set a baseline. An extended ruleset sits below a trust boundary: its ordinary rules merge up by id, but a kind: command rule and an allow_out_of_root field are rejected at load.</desc>
<style>
  .alint-trust { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-trust { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-trust { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-trust .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-trust .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-trust .tx { fill:var(--tx); } .alint-trust .mut { fill:var(--mut); } .alint-trust .ac { fill:var(--ac); }
  .alint-trust .card { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-trust .trusted { fill:var(--card); stroke:var(--ac); stroke-width:1.8; }
  .alint-trust .shield { fill:var(--card); stroke:var(--ac); stroke-width:1.5; }
  .alint-trust .chip { fill:var(--card); stroke:var(--bd); stroke-width:1.2; }
  .alint-trust .flow { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:7 5; opacity:.7; animation:trustflow 1s linear infinite; }
  .alint-trust .pulse { animation:trustpulse 2.2s ease-in-out infinite; }
  @keyframes trustflow { to { stroke-dashoffset:-12; } }
  @keyframes trustpulse { 0%,100%{opacity:1} 50%{opacity:.5} }
  @media (prefers-reduced-motion:reduce){ .alint-trust .flow{animation:none;stroke-dasharray:none} .alint-trust .pulse{animation:none} }
</style>
<text class="ui ac" x="18" y="16">extends merges rules by id</text>
<text class="ui mut" x="442" y="16" text-anchor="end">code stays out</text>
<rect class="trusted" x="30" y="26" width="400" height="58" rx="10"/>
<text class="ui ac" x="44" y="48">your .alint.yml + .alint.d/</text>
<text class="tag mut" x="416" y="46" text-anchor="end">trusted source</text>
<text class="tag" x="44" y="70" fill="#22c55e">may: command, custom facts, allow_out_of_root, baseline</text>
<path class="flow" d="M 20 102 H 440"/>
<rect class="shield" x="166" y="90" width="128" height="24" rx="12"/>
<text class="tag ac" x="230" y="106" text-anchor="middle">trust boundary</text>
<rect class="card" x="30" y="122" width="400" height="214" rx="10"/>
<text class="ui mut" x="44" y="144">extends (fetched or bundled)</text>
<text class="tag mut" x="44" y="162">alint://bundled/ci@v1, https pinned by #sha256</text>
<rect class="chip" x="44" y="172" width="210" height="26" rx="6"/><rect x="44" y="172" width="5" height="26" rx="2" fill="#22c55e"/><text class="tag tx" x="62" y="189">readme-exists: error</text><text class="tag" x="272" y="189" fill="#22c55e">merges by id</text>
<rect class="chip" x="44" y="208" width="210" height="26" rx="6"/><rect x="44" y="208" width="5" height="26" rx="2" fill="#7c3aed"/><text class="tag tx" x="62" y="225">kind: command</text><text class="tag pulse" x="272" y="225" fill="#ef4444">rejected at load</text>
<rect class="chip" x="44" y="244" width="210" height="26" rx="6"/><rect x="44" y="244" width="5" height="26" rx="2" fill="#7c3aed"/><text class="tag tx" x="62" y="261">allow_out_of_root</text><text class="tag pulse" x="272" y="261" fill="#ef4444">rejected at load</text>
<text class="tag mut" x="44" y="302">bundled resolves offline; fetched bodies match their hash</text>
<text class="tag mut" x="230" y="362" text-anchor="middle">an extended ruleset can tighten your checks, never run your commands</text>
</svg>

## How extends merges

Each `extends:` entry names another config, and they resolve **left to right**: a later entry overrides an earlier one, and your own file overrides everything it extends. Merging is **by rule `id`, field by field**, so an entry that re-declares `readme-exists` with only `level: error` changes just that field and inherits `kind`, `paths`, and `message` from below. An entry can be a local file, an `https://` URL, or a bundled ruleset resolved offline from the binary (`alint://bundled/...`); `only:` / `except:` filters (mutually exclusive on one entry) narrow which rules an entry contributes.

Fetched and bundled configs are **leaf nodes**: they cannot declare `extends:` of their own, because relative-path resolution inside a fetched body has no principled base. You nest `extends:` locally instead.

## The trust boundary

Sources are not equally trusted, and the boundary is drawn at `extends:`. Your own `.alint.yml` and its `.alint.d/` drop-ins are **trusted source**: they may declare process-spawning rules (`kind: command` and its siblings), `custom:` facts (which also spawn), `allow_out_of_root:`, and `baseline:`. A config reached **through `extends:`** may declare none of these. Each is rejected at load, by name, so adopting a published ruleset can never:

- **run code** on your machine (a spawning rule, including one smuggled through a `require:` block or a `templates:` entry, is refused),
- **read outside the repo** (`allow_out_of_root:` is a top-level-only grant), or
- **choose which findings are suppressed** (`baseline:` is a top-level-only input).

For `https://` entries, a **SHA-256 subresource-integrity hash** (`#sha256-...`) pins exactly which bytes are trusted; a body that does not match its hash is refused. Bundled rulesets ship inside the binary and are resolved offline, so there is nothing to fetch or pin. The hash pins *which* bytes load, and the trust boundary governs *what those bytes may do*.

## In practice

You extend a shared CI ruleset that, whether by mistake or by malice, hides a `command` rule:

```yaml
# .alint.yml
version: 1
extends:
  - ./ci-rules.yml
```

```yaml
# ci-rules.yml (reached through extends:)
version: 1
rules:
  - id: deploy-check
    kind: command
    command: ["sh", "deploy.sh"]
    paths: "**/*"
    level: error
```

alint refuses to load, naming the rule and the offending config rather than running anything:

```
rule "deploy-check": kind: command spawns a process and is only allowed in the
user's top-level config; declaring one in an extended config (ci-rules.yml)
is refused because it would let a ruleset run arbitrary code
```

Move `deploy-check` into your own top-level `.alint.yml` and it runs, because now you are the one declaring it.

## Going deeper

- [Configuration](/docs/configuration/#extends) is the field reference for `extends:`, `allow_out_of_root:`, and the rule filters.
- [Config layering](/docs/concepts/config-layering/) covers drop-ins and nested configs, the other sources that merge into one effective config.
- [Variable interpolation](/docs/concepts/variable-interpolation/) details the `#sha256-...` pin and how `{{env.X}}` interacts with an `extends:` URL.
