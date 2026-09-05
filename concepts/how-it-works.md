---
title: How alint works
description: "A technical overview of alint's execution pipeline: one declarative config, one parallel pass over the repository, one report in your pipeline's format."
sidebar:
  order: 2
---

alint reads one declarative `.alint.yml`, makes a single parallel pass over your repository, and emits one report in the format your pipeline wants. Here is the whole pipeline, end to end:

<svg class="alint-pipeline" viewBox="0 0 680 120" role="img" aria-labelledby="pipe-t pipe-d" xmlns="http://www.w3.org/2000/svg">
  <title id="pipe-t">alint's execution pipeline</title>
  <desc id="pipe-d">One token flows left to right through four stages: config load, walk, dispatch, and report.</desc>
  <style>
    .alint-pipeline { max-width: 100%; height: auto; font: 600 15px system-ui, -apple-system, "Segoe UI", sans-serif; }
    .alint-pipeline .wire { fill: none; stroke: var(--sl-color-gray-4, #9aa0b4); stroke-width: 2.5; stroke-dasharray: 7 7; animation: alint-flow 1.1s linear infinite; }
    .alint-pipeline .stage { fill: var(--sl-color-gray-6, #eef1f8); stroke: var(--sl-color-accent, #4338ca); stroke-width: 1.5; }
    .alint-pipeline .lbl { fill: var(--sl-color-text, #1f2233); }
    .alint-pipeline .token { fill: var(--sl-color-accent, #4338ca); animation: alint-travel 4s cubic-bezier(.55, 0, .45, 1) infinite; }
    @keyframes alint-flow { to { stroke-dashoffset: -14; } }
    @keyframes alint-travel {
      0% { transform: translateX(0); opacity: 0; }
      6% { opacity: 1; }
      94% { opacity: 1; }
      100% { transform: translateX(510px); opacity: 0; }
    }
    @media (prefers-reduced-motion: reduce) {
      .alint-pipeline .wire { animation: none; stroke-dasharray: none; }
      .alint-pipeline .token { animation: none; transform: translateX(510px); opacity: 1; }
    }
  </style>
  <path class="wire" stroke="#9aa0b4" d="M 85 60 H 595" />
  <g class="lbl" fill="#1f2233" text-anchor="middle">
    <rect class="stage" fill="#eef1f8" stroke="#4338ca" x="20" y="38" width="130" height="44" rx="8" /><text x="85" y="65">config</text>
    <rect class="stage" fill="#eef1f8" stroke="#4338ca" x="190" y="38" width="130" height="44" rx="8" /><text x="255" y="65">walk</text>
    <rect class="stage" fill="#eef1f8" stroke="#4338ca" x="360" y="38" width="130" height="44" rx="8" /><text x="425" y="65">dispatch</text>
    <rect class="stage" fill="#eef1f8" stroke="#4338ca" x="530" y="38" width="130" height="44" rx="8" /><text x="595" y="65">report</text>
  </g>
  <circle class="token" fill="#4338ca" cx="85" cy="60" r="7" />
</svg>

The numbered steps below trace each stage; the interactive model beneath them lets you explore every component and edge.

<likec4-view view-id="checkFlow"></likec4-view>

1. **Config load.** alint reads the `.alint.yml` at the repository root and resolves any `extends:` (local files, HTTPS sources pinned by a subresource-integrity hash, or bundled rulesets), then validates the merged config against its JSON schema.
2. **Facts.** Any `facts:` you declared are evaluated once, sequentially, and cached. Facts answer questions about the repo (does a file exist, how many match a glob, what does a command print) that rules can gate on.
3. **Rule filter.** Each rule's `when:` condition is evaluated against the facts; rules whose condition is false are dropped before a single file is read.
4. **Walk.** alint walks the repository once, in parallel, honoring `.gitignore` and your `ignore:` globs, and builds a deterministic, sorted index of the files.
5. **Dispatch.** Rules split into two classes: cross-file rules scan the whole index, and per-file rules run against each matched file. Either way, every matched file's bytes are read at most once.
6. **Evaluate and aggregate.** Rules produce violations, which are collected into one report. With `alint fix`, auto-fixable violations are applied to the working tree and the check is re-run.
7. **Emit.** The report is rendered in your chosen format (human, JSON, SARIF, and the rest) with an exit code your pipeline can gate on.

The design goal is one config, one pass, one report: predictable, fast, and easy to wire into CI.

## Going deeper

- [Architecture](/docs/about/architecture/) covers the engine and crate-level design, the dispatch model, and the security boundaries.
- [Architecture diagrams](/docs/about/architecture-diagrams/) is the interactive gallery of every flow: config load, fix, facts, the walker, the LSP, CI, and more.
