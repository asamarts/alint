---
title: Bundled rulesets
description: "The 22 curated rulesets compiled into alint: one extends line adopts a set, each rule is fact-gated so it applies only where it belongs, and you override any of it field-by-field in your own config."
sidebar:
  order: 2
---

alint ships **22 bundled rulesets** compiled straight into the binary, with no network round-trip. They are the on-ramp: instead of hand-writing rules, you add one `extends:` line and inherit a curated set, from `oss-baseline` (a Repolinter migration starting point) to per-language, monorepo, CI, hygiene, compliance, and agent-aware sets. Two properties make them safe to adopt broadly: every rule is **fact-gated**, so a ruleset applies itself only where it belongs, and everything it declares is **overridable** field-by-field from your own config.

<svg class="alint-br" viewBox="0 0 460 330" role="img" aria-labelledby="br-t br-d" xmlns="http://www.w3.org/2000/svg">
<title id="br-t">A bundled ruleset's rules are fact-gated, so extending it is a no-op where the fact does not hold</title>
<desc id="br-d">One extends line pulls in three bundled rulesets. oss-baseline always applies. rust@v1 is gated on facts.has_rust and fires because the repo has Rust. go@v1 is gated on facts.has_go and stays dormant because the repo has no Go. Every inherited rule can be overridden in your own config.</desc>
<style>
  .alint-br { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; display:block; margin-inline:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-br { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-br { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-br .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-br .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-br .tx { fill:var(--tx); } .alint-br .mut { fill:var(--mut); } .alint-br .ac { fill:var(--ac); }
  .alint-br .card { fill:var(--card); stroke:var(--bd); stroke-width:1.2; }
  .alint-br .key { fill:var(--card); stroke:var(--ac); stroke-width:1.6; }
  .alint-br .on { stroke:#22c55e; stroke-width:1.8; }
  .alint-br .off { stroke-dasharray:4 3; opacity:.55; }
  .alint-br .flow { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:6 6; opacity:.7; animation:brflow 1s linear infinite; }
  .alint-br .pulse { animation:brpulse 2.4s ease-in-out infinite; }
  @keyframes brflow { to { stroke-dashoffset:-12; } }
  @keyframes brpulse { 0%,100%{opacity:1} 50%{opacity:.55} }
  @media (prefers-reduced-motion:reduce){ .alint-br .flow{animation:none;stroke-dasharray:none} .alint-br .pulse{animation:none} }
</style>
<text class="ui ac" x="18" y="16">one extends line</text>
<text class="ui mut" x="442" y="16" text-anchor="end">22 bundled rulesets</text>
<rect class="key" x="70" y="26" width="320" height="46" rx="9"/><text class="tag ac" x="230" y="45" text-anchor="middle">your .alint.yml</text><text class="tag mut" x="230" y="61" text-anchor="middle">extends: oss-baseline, rust@v1, go@v1</text>
<text class="tag mut" x="230" y="86" text-anchor="middle">repo facts: has_rust = true, has_go = false</text>
<path class="flow" d="M 150 72 C 150 96, 92 96, 92 112"/>
<path class="flow" d="M 230 96 V 112"/>
<path class="flow" d="M 310 72 C 310 96, 368 96, 368 112"/>
<rect class="card on" x="18" y="114" width="424" height="47" rx="9"/><text class="tag tx" x="32" y="133">oss-baseline@v1</text><text class="tag mut" x="32" y="150">always applies</text><text class="tag" x="424" y="142" text-anchor="end" fill="#22c55e">applies</text>
<rect class="card on pulse" x="18" y="166" width="424" height="47" rx="9"/><text class="tag tx" x="32" y="185">rust@v1</text><text class="tag mut" x="32" y="202">when facts.has_rust</text><text class="tag" x="424" y="194" text-anchor="end" fill="#22c55e">fires</text>
<rect class="card off" x="18" y="218" width="424" height="47" rx="9"/><text class="tag tx" x="32" y="237">go@v1</text><text class="tag mut" x="32" y="254">when facts.has_go</text><text class="tag mut" x="424" y="246" text-anchor="end">dormant</text>
<text class="tag mut" x="230" y="288" text-anchor="middle">a ruleset applies itself only where its facts hold</text>
<text class="tag mut" x="230" y="310" text-anchor="middle">override any inherited rule's level in your own config</text>
</svg>

## What they are

The 22 sets fall into a few families: `oss-baseline` (the migration starting point for the archived [Repolinter](https://github.com/todogroup/repolinter)); seven **language** sets (`rust`, `python`, `node`, `go`, `java`, `php`, `dotnet`); a `monorepo` base with Cargo, pnpm, and Yarn workspace overlays; `ci/github-actions`; `tooling/editorconfig`; hygiene sets (`hygiene/lockfiles`, `hygiene/no-tracked-artifacts`); compliance (`compliance/reuse`, `compliance/apache-2`) and `apache/governance`; `docs/adr`; and the two agent-aware sets (`agent-context`, `agent-hygiene`). Each is a hand-curated group of rules, compiled into the binary, referenced as `alint://bundled/<name>@v1`. `alint init` scaffolds a starting config for you: it always extends `oss-baseline` and adds a language set for each stack it auto-detects (Rust, Node, Python, Go, or Java). The `php` and `dotnet` sets ship too but are not auto-detected, so add those to `extends:` by hand.

## Fact-gating

The reason you can extend a language set without worrying whether it fits: each rule inside carries a `when:` gate over a per-run [fact](/docs/concepts/start-here/kinds-families-categories/). `rust@v1`'s rules are `when: facts.has_rust`, so extending it in a repo with no Rust is a silent no-op, not a wall of irrelevant findings. Adopt the whole set and it activates itself where it applies. This is why `oss-baseline` plus every language set is a reasonable default: the gates keep each one dormant until its stack shows up. In a polyglot monorepo the gating goes one level finer: a language set's per-file rules also carry a `scope_filter: { has_ancestor: <manifest> }`, so `rust@v1`'s rules fire only inside a directory that actually contains a `Cargo.toml`, never across an unrelated package.

## Adopting and overriding

Add the sets you want to `extends:`, then tune them in your own config. A rule you inherit is overridden **field-by-field by id**: name the same `id` and set just the fields you want to change (a level, a path, a message), and the rest of the rule stays as the bundle defined it, exactly like [config layering](/docs/concepts/composition/config-layering/) merges a drop-in. What a bundled ruleset does *not* gain is extra execution privilege: reached through `extends:`, it sits **below** the same [trust boundary](/docs/concepts/composition/composition-and-trust/) as any fetched ruleset, so it cannot declare spawning rules, `custom:` facts, `allow_out_of_root`, or a `baseline` any more than a remote one can. What it gains instead is *provenance*: shipping in the binary, it resolves offline and is byte-identical to the release, with nothing to fetch or hash-pin.

## In practice

Extend the OSS baseline and the Rust set, and downgrade one inherited rule to a warning while a repo catches up:

```yaml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/rust@v1
rules:
  - id: readme-exists
    level: warning
```

`alint list` shows the union of the inherited rules with your override applied; `alint check` runs the Rust rules only if the tree actually has Rust.

## Going deeper

- [Bundled rulesets reference](/docs/bundled-rulesets/) documents every ruleset and the rules it contains.
- [Composition and trust](/docs/concepts/composition/composition-and-trust/) is the `extends:` merge and trust boundary in depth.
- [Config layering](/docs/concepts/composition/config-layering/) is how overrides and drop-ins assemble one effective config.
