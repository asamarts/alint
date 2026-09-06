---
title: Fixing
description: "How alint repairs violations: rules with a fix: block are auto-fixable, the twelve fix ops, content_from:, fix_size_limit, and why evaluation is parallel but fixes apply one rule at a time."
sidebar:
  order: 12
---

A rule that can mechanically repair its violation declares a `fix:` block. `alint check` only reports; `alint fix` applies the repairs. Evaluation runs in parallel across files, but the fix pass is a single sequential loop, one rule at a time, because each fixer writes to the tree that the next rule will read.

<svg class="alint-fix" viewBox="0 0 460 344" role="img" aria-labelledby="fx-t fx-d" xmlns="http://www.w3.org/2000/svg">
<title id="fx-t">alint evaluates files in parallel, then applies fixes one rule at a time</title>
<desc id="fx-d">Three files are evaluated in parallel and produce fixable violations. Those violations feed a single sequential lane where fixes are applied one rule at a time.</desc>
<style>
  .alint-fix { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-fix { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-fix { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-fix .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-fix .tag { font:600 11px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-fix .tx { fill:var(--tx); } .alint-fix .mut { fill:var(--mut); } .alint-fix .ac { fill:var(--ac); }
  .alint-fix .chip { fill:var(--card); stroke:var(--bd); stroke-width:1.2; }
  .alint-fix .lane { fill:var(--card); stroke:var(--ac); stroke-width:1.6; }
  .alint-fix .flow { fill:none; stroke:var(--ac); stroke-width:2; stroke-dasharray:6 6; opacity:.7; animation:fxflow 1s linear infinite; }
  .alint-fix .pulse { animation:fxpulse 2s ease-in-out infinite; }
  .alint-fix .tok { fill:var(--ac); animation:fxtok 3.6s cubic-bezier(.5,0,.5,1) infinite; }
  @keyframes fxflow { to { stroke-dashoffset:-12; } }
  @keyframes fxpulse { 0%,100%{opacity:1} 50%{opacity:.5} }
  @keyframes fxtok { 0%{transform:translateX(0);opacity:0} 8%{opacity:1} 90%{opacity:1} 100%{transform:translateX(300px);opacity:0} }
  @media (prefers-reduced-motion:reduce){ .alint-fix .flow{animation:none;stroke-dasharray:none} .alint-fix .pulse{animation:none} .alint-fix .tok{animation:none;opacity:1;transform:translateX(300px)} }
</style>
<text class="ui ac" x="18" y="16">evaluate</text>
<text class="ui mut" x="146" y="16">parallel, all files at once</text>
<rect class="chip pulse" x="30" y="26" width="120" height="30" rx="7"/><text class="tag tx" x="90" y="45" text-anchor="middle">README.md</text>
<rect class="chip pulse" x="170" y="26" width="120" height="30" rx="7"/><text class="tag tx" x="230" y="45" text-anchor="middle">Cargo.toml</text>
<rect class="chip pulse" x="310" y="26" width="120" height="30" rx="7"/><text class="tag tx" x="370" y="45" text-anchor="middle">main.rs</text>
<path class="flow" d="M 90 58 C 90 92, 230 92, 230 116"/>
<path class="flow" d="M 230 58 V 116"/>
<path class="flow" d="M 370 58 C 370 92, 230 92, 230 116"/>
<rect class="lane" x="40" y="122" width="380" height="56" rx="10"/>
<text class="ui ac" x="56" y="144">apply</text>
<text class="tag mut" x="110" y="144">sequential, one rule at a time</text>
<line x1="60" y1="162" x2="400" y2="162" stroke="var(--bd)" stroke-width="2"/>
<circle cx="100" cy="162" r="4" fill="var(--ac)"/><text class="tag mut" x="100" y="176" text-anchor="middle">fix 1</text>
<circle cx="230" cy="162" r="4" fill="var(--ac)"/><text class="tag mut" x="230" y="176" text-anchor="middle">fix 2</text>
<circle cx="360" cy="162" r="4" fill="var(--ac)"/><text class="tag mut" x="360" y="176" text-anchor="middle">fix 3</text>
<circle class="tok" cx="100" cy="162" r="6"/>
<text class="ui ac" x="18" y="214">two families of op</text>
<rect class="chip" x="18" y="224" width="424" height="44" rx="8"/><text class="tag tx" x="32" y="242">content edits (7)</text><text class="tag mut" x="32" y="258">trim, newline, line endings, BOM, bidi, zero-width, blanks</text>
<rect class="chip" x="18" y="278" width="424" height="44" rx="8"/><text class="tag tx" x="32" y="296">path + content (5)</text><text class="tag mut" x="32" y="312">create, remove, rename, prepend, append</text>
</svg>

## Fixable versus report-only

A rule is auto-fixable only if it declares a `fix:` block; otherwise its violation is report-only and you repair it by hand. `alint check --format human` marks the fixable ones, and the summary counts them. `alint fix` applies them, `alint fix --dry-run` prints what it would change without writing, `alint fix --only <rule-id>` applies just one rule's fixers (the command the [agent surface](/docs/concepts/the-agent-surface/) emits), and `alint fix --changed` restricts the pass to the diff ([cross-file](/docs/concepts/cross-file-rules/) and existence rules still see the whole tree). A violation with no fixer still fails the gate; fixing is an accelerant, not an escape hatch.

The fix pass runs **rule by rule in sequence** (evaluate the rule, then apply its fixers) rather than in parallel like `check`, because a fixer mutates files on disk and a later rule must see the result, not race it.

## The twelve ops

Seven ops edit content in place: `file_trim_trailing_whitespace`, `file_append_final_newline`, `file_normalize_line_endings`, `file_strip_bom`, `file_strip_bidi`, `file_strip_zero_width`, and `file_collapse_blank_lines`. Five more work at the path or prepend/append level: `file_create`, `file_remove`, `file_rename`, `file_prepend`, and `file_append`.

## content_from

The three content-providing ops, `file_create`, `file_prepend`, and `file_append`, take either an inline `content:` string or a `content_from: <path>` that reads the bytes from a file. Exactly one of the two must be set. The path resolves against the lint root and is read at fix-apply time, so a LICENSE or SPDX header can live in `.alint/templates/` under version control instead of being escaped into YAML. A missing `content_from:` source is reported as `Skipped`, never a half-written file.

These ops are careful about repeat runs: `file_prepend` and `file_append` are idempotent (a no-op when the content is already present) and `file_prepend` preserves a leading byte-order mark, while `file_create` skips a target that already exists.

## fix_size_limit

`fix_size_limit` (default 1 MiB) bounds the file a fixer **rewrites in place**: if that target is over the cap it is reported `Skipped` rather than rewritten. It covers the seven content edits **and** `file_prepend` / `file_append`, all of which read the target to splice content. The path ops fall outside it: `file_remove` and `file_rename` read no content at all, and while `file_create` does read its `content_from:` template, that read is not itself bounded by `fix_size_limit`.

## In practice

Trim trailing whitespace across Markdown, and create a missing LICENSE from a template:

```yaml
version: 1
rules:
  - id: md-trim
    kind: no_trailing_whitespace
    paths: "**/*.md"
    level: info
    fix:
      file_trim_trailing_whitespace: {}
  - id: license-present
    kind: file_exists
    paths: [LICENSE]
    root_only: true
    level: error
    fix:
      file_create:
        content_from: ".alint/templates/LICENSE-MIT.txt"
```

`alint fix` prints a block per rule and a final count:

```
error [license-present]:
  ✓ created LICENSE
info [md-trim]:
  ✓ docs/guide.md — trimmed trailing whitespace in docs/guide.md

2 applied, 0 skipped, 0 unfixable.
```

Each applied line is prefixed with the file it touched. A file over `fix_size_limit`, or a `content_from:` source that is missing, shows on its rule as `(skipped: <reason>)` and lands in the `skipped` count instead.

## Going deeper

- [Configuration](/docs/configuration/#fix_size_limit) documents `fix_size_limit` and the per-rule `fix:` field.
- [content_from](/docs/concepts/content-from/) is the file-backed alternative to an inline `content:` string, in depth.
- [Rules](/docs/rules/) lists which kinds ship a fixer and the options each op takes.
- [Changed mode](/docs/concepts/changed-mode/) covers `alint fix --changed`.
