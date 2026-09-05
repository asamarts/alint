---
title: How alint works
description: "A deep look at alint's execution pipeline: assemble one config, evaluate facts once, filter rules, walk the repository in parallel, dispatch per-file and cross-file rules over a single read of each file, and emit one report."
sidebar:
  order: 2
---

alint reads one declarative config, makes a single parallel pass over your repository, and emits one report in the format your pipeline wants. The diagram below traces the whole run top to bottom: the command, the config it assembles, the facts and `when:` filter that decide which rules survive, the walk that indexes the repository, the scanner that reads each file once, and the report that comes back with an exit code.

<svg class="alint-check" viewBox="0 0 460 876" role="img" aria-labelledby="chk-t chk-d" xmlns="http://www.w3.org/2000/svg">
<title id="chk-t">alint check: the execution pipeline</title>
<desc id="chk-d">A vertical pipeline. The command runs alint check. The config assembles from .alint.yml and its extends, with a spawn-gate blocking process-spawning rules from extended configs, facts evaluated once, and rules filtered by when. Four active rules survive. The repository is walked once into a sorted index, then scanned file by file: a glowing scanner reads each file once, marking four passes with green checks and two failures with red crosses, while a cross-file rule scans the whole index. The report shows four passed, two failed, and exit code 1 in any of eight formats.</desc>
<style>
  .alint-check { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --repo:#eaeefb; --bd:#c7cfe0; --ac:#4f46e5; --term:#1e1b3a; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-check { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --repo:#1c2130; --bd:#3b4254; --ac:#8b93f8; --term:#0f1120; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-check { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --repo:#1c2130; --bd:#3b4254; --ac:#8b93f8; --term:#0f1120; } }
  .alint-check .mono { font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-check .ui   { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-check .tx { fill:var(--tx); } .alint-check .mut { fill:var(--mut); } .alint-check .ac { fill:var(--ac); }
  .alint-check .card { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-check .repo { fill:var(--repo); stroke:var(--bd); stroke-width:1.4; }
  .alint-check .accent-card { fill:var(--card); stroke:var(--ac); stroke-width:1.6; }
  .alint-check .flow { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:6 6; opacity:.75; animation:chkf 1s linear infinite; }
  .alint-check .scan { animation:chks 6s ease-in-out infinite; }
  .alint-check .glow { fill:var(--ac); opacity:.16; }
  .alint-check .small { font:600 11px system-ui, -apple-system, sans-serif; }
  .alint-check .exit { font:700 17px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-check .off { opacity:.45; }
  @keyframes chkf { to { stroke-dashoffset:-12; } }
  @keyframes chks { 0%{transform:translateY(0);opacity:0} 5%{opacity:1} 92%{transform:translateY(220px);opacity:1} 100%{transform:translateY(220px);opacity:0} }
  @media (prefers-reduced-motion:reduce){ .alint-check .flow{animation:none;stroke-dasharray:none} .alint-check .scan{animation:none;transform:translateY(88px)} }
</style>
<rect class="card" x="20" y="18" width="420" height="34" rx="6" style="fill:var(--term);stroke:var(--term)"/>
<text class="mono" x="34" y="40" fill="#22c55e">$</text><text class="mono" x="48" y="40" fill="#e6e8ef">alint check .</text>
<path class="flow" d="M 230 52 V 66"/>
<rect class="accent-card" x="20" y="66" width="420" height="94" rx="9"/>
<text class="mono ac" x="34" y="88">.alint.yml</text>
<text class="mono mut" x="34" y="107" font-size="12">extends: rust@v1</text>
<rect x="30" y="114" width="182" height="20" rx="10" fill="#ef4444" opacity=".14"/>
<text class="small" x="40" y="128" fill="#ef4444">spawn-gate blocks command &#10007;</text>
<text class="ui mut" x="34" y="151">facts once &#183; when filters rules</text>
<text class="ui ac" x="20" y="180">active rules</text>
<rect class="card" x="20" y="188" width="84" height="24" rx="12" style="stroke:var(--ac)"/><text class="mono tx" x="31" y="204" font-size="12">no_bidi</text>
<rect class="card" x="112" y="188" width="126" height="24" rx="12" style="stroke:var(--ac)"/><text class="mono tx" x="124" y="204" font-size="12">filename_case</text>
<rect class="card" x="20" y="220" width="64" height="24" rx="12" style="stroke:#7c3aed"/><text class="mono" x="31" y="236" font-size="12" fill="#7c3aed">pair</text>
<rect class="card off" x="92" y="220" width="150" height="24" rx="12"/><text class="mono mut off" x="103" y="236" font-size="11">win (when: false)</text>
<path class="flow" d="M 230 250 V 278"/><text class="ui mut" x="242" y="263">walk</text>
<rect class="repo" x="20" y="280" width="420" height="344" rx="14"/>
<text class="ui ac" x="34" y="303">repository</text><text class="ui mut" x="116" y="303">walked once, sorted</text>
<g class="scan">
  <rect class="glow" x="28" y="308" width="384" height="48" rx="10"/>
</g>
<rect class="card" x="34" y="312" width="372" height="40" rx="7"/><rect x="34" y="312" width="6" height="40" rx="2" fill="#3b82f6"/><text class="mono tx" x="56" y="337">README.md</text><circle cx="388" cy="332" r="10" fill="#22c55e"/><path d="M383 332 l3.5 3.5 l6 -7" stroke="#fff" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
<rect class="card" x="34" y="356" width="372" height="40" rx="7"/><rect x="34" y="356" width="6" height="40" rx="2" fill="#f59e0b"/><text class="mono tx" x="56" y="381">Cargo.toml</text><circle cx="388" cy="376" r="10" fill="#22c55e"/><path d="M383 376 l3.5 3.5 l6 -7" stroke="#fff" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
<rect class="card" x="34" y="400" width="372" height="40" rx="7"/><rect x="34" y="400" width="6" height="40" rx="2" fill="#f97316"/><text class="mono tx" x="56" y="425">src/main.rs</text><circle cx="388" cy="420" r="10" fill="#ef4444"/><path d="M384 416 l8 8 M392 416 l-8 8" stroke="#fff" stroke-width="2" stroke-linecap="round"/>
<rect class="card" x="34" y="444" width="372" height="40" rx="7"/><rect x="34" y="444" width="6" height="40" rx="2" fill="#f97316"/><text class="mono tx" x="56" y="469">src/Utils.rs</text><circle cx="388" cy="464" r="10" fill="#ef4444"/><path d="M384 460 l8 8 M392 460 l-8 8" stroke="#fff" stroke-width="2" stroke-linecap="round"/>
<rect class="card" x="34" y="488" width="372" height="40" rx="7"/><rect x="34" y="488" width="6" height="40" rx="2" fill="#06b6d4"/><text class="mono tx" x="56" y="513">ci.yml</text><circle cx="388" cy="508" r="10" fill="#22c55e"/><path d="M383 508 l3.5 3.5 l6 -7" stroke="#fff" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
<rect class="card" x="34" y="532" width="372" height="40" rx="7"/><rect x="34" y="532" width="6" height="40" rx="2" fill="#94a3b8"/><text class="mono tx" x="56" y="557">LICENSE</text><circle cx="388" cy="552" r="10" fill="#22c55e"/><path d="M383 552 l3.5 3.5 l6 -7" stroke="#fff" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
<line x1="34" y1="584" x2="426" y2="584" stroke="var(--bd)" stroke-width="1" opacity=".5"/>
<path d="M 40 600 h -6 v 14 h 6" fill="none" stroke="#7c3aed" stroke-width="2"/>
<text class="ui" x="50" y="611"><tspan fill="#7c3aed">cross-file</tspan><tspan class="mut"> rules scan the whole index (pair)</tspan></text>
<path class="flow" d="M 230 624 V 640"/><text class="ui mut" x="242" y="635">aggregate</text>
<rect class="accent-card" x="20" y="640" width="420" height="220" rx="12"/>
<text class="ui ac" x="34" y="663">report</text>
<rect x="34" y="672" width="258" height="14" rx="7" fill="#22c55e"/><rect x="296" y="672" width="130" height="14" rx="7" fill="#ef4444"/>
<text class="ui" x="34" y="704" fill="#22c55e">4 passed</text><text class="ui" x="426" y="704" fill="#ef4444" text-anchor="end">2 failed</text>
<circle cx="44" cy="720" r="8" fill="#ef4444"/><path d="M40.5 716.5 l7 7 M47.5 716.5 l-7 7" stroke="#fff" stroke-width="1.7" stroke-linecap="round"/>
<text class="mono tx" x="60" y="724" font-size="12">no_bidi</text>
<text class="mono mut" x="60" y="741" font-size="11">src/main.rs:12</text>
<circle cx="44" cy="760" r="8" fill="#ef4444"/><path d="M40.5 756.5 l7 7 M47.5 756.5 l-7 7" stroke="#fff" stroke-width="1.7" stroke-linecap="round"/>
<text class="mono tx" x="60" y="764" font-size="12">filename_case</text>
<text class="mono mut" x="60" y="781" font-size="11">src/Utils.rs</text>
<line x1="34" y1="788" x2="426" y2="788" stroke="var(--bd)" stroke-width="1"/>
<text class="exit" x="34" y="822" fill="#ef4444">exit 1</text>
<rect class="card" x="120" y="808" width="60" height="20" rx="10"/><text class="ui mut" x="130" y="822">human</text>
<rect class="card" x="186" y="808" width="46" height="20" rx="10"/><text class="ui mut" x="194" y="822">json</text>
<text class="ui mut" x="34" y="848">SARIF, GitHub, and 5 more formats</text>
</svg>

The design goal is one config, one pass, one report: predictable, fast, and easy to wire into CI. The stages below trace the run in order.

## 1. Assemble the config

alint discovers the `.alint.yml` at the repository root and builds the **effective config**: it resolves every `extends:` source (a local file, an `https://` URL pinned by a SHA-256 hash, or a bundled ruleset resolved offline), caches and cycle-checks them, and field-merges each layer by rule `id`. This is also the trust boundary: a process-spawning rule (`kind: command` and its siblings) or a `custom:` fact that arrives through `extends:` is rejected at load, so adopting someone else's ruleset can never make your machine run their commands. The [config model](/docs/concepts/the-config-model/) covers assembly and precedence in full.

## 2. Evaluate facts, once

Any `facts:` you declared are evaluated a single time, in order, before any rule runs, and the results are reused for the rest of the run. Facts answer questions about the repository (does a file exist, how many match a glob, what does a command print) that rules gate on.

## 3. Filter rules by `when:`

Each rule's `when:` expression is evaluated against the facts. A rule whose condition is false is dropped **before a single file is read**, so a config that layers in the Rust, Node, Python, and Go rulesets costs almost nothing in a repository that is only one of them.

## 4. Walk the repository

alint walks the tree once, in parallel, honoring `.gitignore` (and `.ignore`, git excludes, and your `ignore:` globs), and builds one deterministic, sorted `FileIndex`. Sorting the index up front is what makes a run reproducible: the same repository produces byte-identical output every time.

## 5. Dispatch: scan each file once

Rules split into two classes, and both run over that single index:

- **Per-file rules** run file-major. alint reads each matched file's bytes **at most once**, no matter how many rules apply to it, and hands that one read to every matching rule (`no_bidi`, `filename_case`, and the rest). This read-coalescing is why adding more per-file rules barely changes a run's cost.
- **Cross-file rules** run rule-major: each one scans the whole index itself to assert a relationship no single file can (every crate has a README, no two manifests disagree, a reference graph is acyclic).

Both classes run in parallel across cores; their violations are merged and re-sorted so the output stays deterministic.

## 6. Aggregate and emit

The violations collect into one `Report`. alint renders it in your chosen format (human, JSON, SARIF, and five more) and returns an exit code your pipeline can gate on. With `alint fix`, auto-fixable violations are applied to the working tree, serially, and the check re-runs.

## Going deeper

- [The config model](/docs/concepts/the-config-model/) is the language this pipeline evaluates.
- The interactive model below lets you explore every component and edge of the run:

<likec4-view view-id="checkFlow"></likec4-view>

- [Architecture](/docs/about/architecture/) covers the engine, the crate-level design, and the security boundaries.
- [Architecture diagrams](/docs/about/architecture-diagrams/) is the interactive gallery of every flow: config load, fix, facts, the walker, the LSP, CI, and more.
