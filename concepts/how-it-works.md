---
title: How alint works
description: "A technical overview of alint's execution pipeline: one declarative config, one parallel pass over the repository, one report in your pipeline's format."
sidebar:
  order: 2
---

alint reads one declarative `.alint.yml`, makes a single parallel pass over your repository, and emits one report in the format your pipeline wants. Here is the whole pipeline, end to end:

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
