---
title: Scoping
description: "How a rule narrows from the whole index to the files it judges: the when: fact gate, the paths: glob, and scope_filter: predicates, applied in a fixed order."
sidebar:
  order: 6
---

A rule never judges the whole repository. It narrows from the walked index to a specific set of files through three gates applied in a fixed order: `when:` decides whether the rule runs at all, `paths:` selects files by glob, and `scope_filter:` refines that selection per file. Only what survives all three (and the optional `git_tracked_only:` narrowing) is evaluated.

<svg class="alint-scope" viewBox="0 0 460 340" role="img" aria-labelledby="scope-t scope-d" xmlns="http://www.w3.org/2000/svg">
<title id="scope-t">A rule narrows the file index through gates in a fixed order</title>
<desc id="scope-d">A when: gate decides whether the rule runs. Then the walked index of 40 files narrows through the paths: glob to 12, through scope_filter: to 8, and through git_tracked_only to 8, leaving 8 files evaluated. Each gate is a narrower bar than the one above it.</desc>
<style>
  .alint-scope { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-scope { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-scope { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-scope .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-scope .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-scope .tx { fill:var(--tx); } .alint-scope .mut { fill:var(--mut); } .alint-scope .ac { fill:var(--ac); }
  .alint-scope .bar { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-scope .gate { fill:var(--card); stroke:var(--ac); stroke-width:1.6; }
  .alint-scope .final { fill:var(--card); stroke:var(--ac); stroke-width:1.8; }
  .alint-scope .flow { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:6 6; opacity:.6; animation:scopeflow 1s linear infinite; }
  .alint-scope .token { fill:var(--ac); animation:scopetok 3.4s cubic-bezier(.5,0,.5,1) infinite; }
  @keyframes scopeflow { to { stroke-dashoffset:-12; } }
  @keyframes scopetok { 0%{transform:translateY(0);opacity:0} 8%{opacity:1} 90%{opacity:1} 100%{transform:translateY(190px);opacity:0} }
  @media (prefers-reduced-motion:reduce){ .alint-scope .flow{animation:none;stroke-dasharray:none} .alint-scope .token{animation:none;opacity:1;transform:translateY(190px)} }
</style>
<rect class="gate" x="126" y="22" width="208" height="28" rx="14"/>
<text class="tag ac" x="230" y="40" text-anchor="middle">when: facts.has_rust</text>
<text class="tag mut" x="230" y="66" text-anchor="middle">true: the rule runs. false: dropped before any file is read.</text>
<path class="flow" d="M 230 74 V 286"/>
<rect class="bar" x="30" y="80" width="400" height="30" rx="6"/><text class="tag mut" x="42" y="99">walked index</text><text class="tag tx" x="418" y="99" text-anchor="end">40 files</text>
<rect class="bar" x="64" y="124" width="332" height="30" rx="6"/><text class="tag tx" x="76" y="143">paths: src/**/*.rs</text><text class="tag mut" x="384" y="143" text-anchor="end">12</text>
<rect class="bar" x="98" y="168" width="264" height="30" rx="6"/><text class="tag tx" x="110" y="187">scope_filter: has_ancestor</text><text class="tag mut" x="350" y="187" text-anchor="end">8</text>
<rect class="bar" x="132" y="212" width="196" height="30" rx="6"/><text class="tag tx" x="144" y="231">git_tracked_only</text><text class="tag mut" x="316" y="231" text-anchor="end">8</text>
<rect class="final" x="160" y="256" width="140" height="30" rx="6"/><text class="tag ac" x="172" y="275">evaluate</text><text class="tag ac" x="288" y="275" text-anchor="end">8</text>
<circle class="token" cx="230" cy="86" r="5"/>
<text class="tag mut" x="230" y="316" text-anchor="middle">each gate ANDs onto the last; the set only ever shrinks</text>
</svg>

## The three gates

`when:` is a run-wide switch, not a per-file filter. It is a bounded boolean expression over four namespaces (`facts.`, `vars.`, `env.`, and `iter.` inside iteration contexts), evaluated once per run against facts computed once. If it is false, the whole rule is dropped before a single file is read, so a rule gated on an absent ecosystem costs nothing. A missing fact reads as `null` (falsy).

`paths:` is the primary selector: a glob, a list of globs, or an `{include, exclude}` pair matched against the walked index. It answers "which files is this rule about."

`scope_filter:` refines that per file, for the cases a glob cannot express. Its predicates **AND-compose**: when more than one is set, a file must satisfy all of them. At least one predicate must be present. Cross-file rules (`pair`, `for_each_dir`, `file_exists`, and their siblings) reject `scope_filter:` at build time, with a pointer to the `for_each_dir` + `when_iter:` pattern instead; the rule-major per-file kinds (`filename_case`, `file_max_size`, and their siblings) honor it as of v0.15.

## scope_filter predicates

- **`has_ancestor:`** keeps a file only when a named manifest sits somewhere in its ancestor directory chain. The engine walks `Path::parent()` upward (the file's own directory counts) and stops at the first match, so a content rule scopes to just its ecosystem's subtree in a polyglot monorepo. The bundled ecosystem rulesets use it to confine per-file rules to their package subtrees.
- **`changed_since: <git-ref>`** keeps only files in the `<ref>...HEAD` diff, the same merge-base diff as [`--changed`](/docs/concepts/changed-mode/). It accepts `{{env.X}}` interpolation and resolves the diff once per run.
- **`include_manifest_paths:` / `exclude_manifest_paths:`** scope by membership in a path set a manifest declares (a `Cargo.toml` `workspace.members`, a `package.json` `bin`), so the manifest that owns the truth and the rule that depends on it stay in one place. An optional **`derive_target: { from, to }`** regex maps a declared build output back to its source (`dist/cli.js` back to `src/cli.ts`); `expect_nonempty:` (default `true`) warns when an include set resolves to nothing rather than silently matching no files.

A manifest **value** only gates which files a rule sees, never what it decides about them: extraction is pure parsing (no spawn, so it is safe inside an `extends:`'d ruleset), and `alint explain <rule>` prints the resolved set.

## In practice

Require an SPDX header, but only on the Rust files a PR actually touched, and only in a repo that has Rust at all:

```yaml
version: 1
facts:
  - id: has_rust
    any_file_exists: [Cargo.toml]
rules:
  - id: spdx-on-changed-rust
    kind: file_header
    when: facts.has_rust
    paths: "**/*.rs"
    pattern: "^// SPDX-License-Identifier:"
    scope_filter:
      has_ancestor: Cargo.toml
      changed_since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
    level: error
    message: "add an SPDX-License-Identifier header"
```

On a PR that adds one unheadered `crates/core/src/new.rs`:

```
error  spdx-on-changed-rust  crates/core/src/new.rs: add an SPDX-License-Identifier header
```

Unchanged Rust files, and every file outside a `Cargo.toml` subtree, are never considered.

## Going deeper

- [Configuration](/docs/configuration/#scope_filter-per-file-rules-v096) is the field reference for every `scope_filter:` predicate and its options.
- [The walker and git](/docs/concepts/walker-and-gitignore/) is the index these gates narrow, and where `git_tracked_only:` is defined.
- [Changed mode](/docs/concepts/changed-mode/) is the run-wide `--changed` counterpart to the per-rule `changed_since:` predicate.
