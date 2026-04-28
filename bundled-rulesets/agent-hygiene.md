---
title: 'agent-hygiene@v1'
description: Bundled alint ruleset at alint://bundled/agent-hygiene@v1.
---

Hygiene rules for the AI-coding era — patterns that show up
disproportionately in commits authored or co-authored by
Claude Code, Cursor, Copilot agent, Aider, Codex, and other
coding agents. Each rule targets a failure mode that happens
*more often* with agents than with humans, but the rules
themselves are agent-agnostic — they catch any commit
matching the pattern, no special-casing on author identity.

Composes with the existing hygiene rulesets — reach for
all three on agent-heavy projects:

```yaml
extends:
  - alint://bundled/hygiene/no-tracked-artifacts@v1
  - alint://bundled/hygiene/lockfiles@v1
  - alint://bundled/agent-hygiene@v1
```

`no-tracked-artifacts@v1` already covers OS / editor / build
junk (`.DS_Store`, `*.bak`, `*.swp`, `node_modules/`, `.env`,
10 MiB+ files); this ruleset focuses on the patterns that are
*distinctly* agent-shaped — versioned-duplicate filenames,
scratch / planning docs, AI-affirmation prose, debug residue,
and model-attributed TODO markers.

Defaults are non-blocking on the heuristic checks (`info` /
`warning`) and `error` only on unambiguous bugs (`debugger;`
in non-test source). Override severity per-rule in your own
config once you're ready to enforce.

## Rules

### `agent-no-versioned-duplicates`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`

> Filename looks like a versioned duplicate (e.g. utils_v2.ts, app_old.js). Replace the original instead of keeping a parallel copy.

### `agent-no-scratch-docs-at-root`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`

> Scratch / planning documents should not be committed at the repo root. Move the content into a real doc (CHANGELOG, ADR, design doc, README) or delete it once the work is done.

### `agent-no-affirmation-prose`

- **kind**: [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/)
- **level**: `info`

> AI-style affirmation phrasing in committed content. These are characteristic of agent-authored prose; trim before merge.

### `agent-no-console-log`

- **kind**: [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/)
- **level**: `warning`

> `console.log` / `.debug` / `.trace` left in non-test source. Route through the project logger or remove before merge.

### `agent-no-debugger-statements`

- **kind**: [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/)
- **level**: `error`

> `debugger;` / `breakpoint()` must not be committed — these halt execution at runtime. Remove before merge.

### `agent-no-model-todos`

- **kind**: [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/)
- **level**: `warning`

> Agent-attributed TODO marker. Resolve, convert to a tracked issue, or remove the model attribution — these outlive the session that wrote them.

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/agent-hygiene.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/agent-hygiene.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/agent-hygiene@v1
#
# Hygiene rules for the AI-coding era — patterns that show up
# disproportionately in commits authored or co-authored by
# Claude Code, Cursor, Copilot agent, Aider, Codex, and other
# coding agents. Each rule targets a failure mode that happens
# *more often* with agents than with humans, but the rules
# themselves are agent-agnostic — they catch any commit
# matching the pattern, no special-casing on author identity.
#
# Composes with the existing hygiene rulesets — reach for
# all three on agent-heavy projects:
#
#     extends:
#       - alint://bundled/hygiene/no-tracked-artifacts@v1
#       - alint://bundled/hygiene/lockfiles@v1
#       - alint://bundled/agent-hygiene@v1
#
# `no-tracked-artifacts@v1` already covers OS / editor / build
# junk (`.DS_Store`, `*.bak`, `*.swp`, `node_modules/`, `.env`,
# 10 MiB+ files); this ruleset focuses on the patterns that are
# *distinctly* agent-shaped — versioned-duplicate filenames,
# scratch / planning docs, AI-affirmation prose, debug residue,
# and model-attributed TODO markers.
#
# Defaults are non-blocking on the heuristic checks (`info` /
# `warning`) and `error` only on unambiguous bugs (`debugger;`
# in non-test source). Override severity per-rule in your own
# config once you're ready to enforce.

version: 1

rules:
  # --- Versioned-duplicate filenames -------------------------------
  # Agents tend to write `utils_v2.ts` / `app_old.js` /
  # `api_FINAL.py` instead of replacing the original. Combined
  # with `hygiene-no-editor-backups` from `no-tracked-artifacts@v1`
  # (which catches `*.bak` / `*~` / `*.swp`), this gives full
  # coverage of the leftover-artefact filename surface.
  - id: agent-no-versioned-duplicates
    kind: file_absent
    paths:
      - "**/*_v[0-9]*"
      - "**/*-v[0-9]*"
      - "**/*_old.*"
      - "**/*_old"
      - "**/*_new.*"
      - "**/*_final.*"
      - "**/*_FINAL.*"
      - "**/*_copy.*"
      - "**/*_backup.*"
      - "**/*.copy.*"
    level: warning
    message: >-
      Filename looks like a versioned duplicate (e.g.
      utils_v2.ts, app_old.js). Replace the original instead
      of keeping a parallel copy.

  # --- Planning / scratch docs at repo root ------------------------
  # Agents spawn planning files (PLAN.md, NOTES.md, ANALYSIS.md,
  # …) as part of their workflow and frequently forget to delete
  # them before committing. Best-practice AGENTS.md templates
  # explicitly tell agents to remove these post-merge — this rule
  # enforces the discipline.
  #
  # `root_only: true` so a legitimate `notes.md` deeper in the
  # tree (e.g. `docs/notes.md` for a feature, or a per-package
  # `packages/foo/NOTES.md`) does not trigger.
  - id: agent-no-scratch-docs-at-root
    kind: file_absent
    paths:
      - PLAN.md
      - NOTES.md
      - ANALYSIS.md
      - SUMMARY.md
      - FIX.md
      - DECISION.md
      - TODO.md
      - SCRATCH.md
      - DEBUG.md
      - TEMP.md
      - WIP.md
    root_only: true
    level: warning
    message: >-
      Scratch / planning documents should not be committed at
      the repo root. Move the content into a real doc
      (CHANGELOG, ADR, design doc, README) or delete it once
      the work is done.

  # --- AI-affirmation prose in source ------------------------------
  # Reviewers consistently flag these stock phrases as "AI smell."
  # The pattern is narrow enough that legitimate code shouldn't
  # match — info-level so it's a soft nudge, not a hard gate.
  #
  # CHANGELOG and snapshot tests are excluded because they may
  # legitimately quote AI-style text from upstream sources or
  # captured fixture output.
  - id: agent-no-affirmation-prose
    kind: file_content_forbidden
    paths:
      include: ["**/*.{rs,ts,tsx,js,jsx,py,go,java,kt,rb,md}"]
      exclude:
        - "**/test*/**"
        - "**/__tests__/**"
        - "**/CHANGELOG*"
        - "**/*.snap"
    pattern: "(?i)(you'?re absolutely right|excellent question|happy to help|great (point|question)|let me know if you need)"
    level: info
    message: >-
      AI-style affirmation phrasing in committed content. These
      are characteristic of agent-authored prose; trim before
      merge.

  # --- Debug residue ------------------------------------------------
  # `console.log` / `.debug` / `.trace` left in non-test JS / TS
  # sources. Excludes test files and dev-tooling configs that may
  # legitimately log.
  #
  # The leading `(?:^|[\s;{(])` ensures we don't match
  # `myconsole.log(...)` or other false positives where `console`
  # is part of a longer identifier.
  - id: agent-no-console-log
    kind: file_content_forbidden
    paths:
      include: ["**/*.{ts,tsx,js,jsx,mjs,cjs}"]
      exclude:
        - "**/*.{test,spec}.*"
        - "**/test*/**"
        - "**/__tests__/**"
        - "**/*.config.*"
    pattern: '(?:^|[\s;{(])console\.(log|debug|trace)\s*\('
    level: warning
    message: >-
      `console.log` / `.debug` / `.trace` left in non-test
      source. Route through the project logger or remove
      before merge.

  - id: agent-no-debugger-statements
    kind: file_content_forbidden
    paths:
      include: ["**/*.{ts,tsx,js,jsx,mjs,cjs,py}"]
      exclude:
        - "**/*.{test,spec}.*"
        - "**/test*/**"
        - "**/__tests__/**"
    pattern: '(?:^|[\s;{(])(debugger|breakpoint\(\))(?:[\s;)]|$)'
    level: error
    message: >-
      `debugger;` / `breakpoint()` must not be committed —
      these halt execution at runtime. Remove before merge.

  # --- Model-attributed TODOs --------------------------------------
  # `TODO(claude:)`, `FIXME(cursor:)`, `XXX(gpt:)` etc. — TODO
  # markers that name a coding agent. They outlive the session
  # that wrote them and are typically actionable items the agent
  # intended to come back to but never did.
  - id: agent-no-model-todos
    kind: file_content_forbidden
    paths:
      include:
        ["**/*.{rs,ts,tsx,js,jsx,py,go,java,kt,rb,scala,c,cc,cpp,h,hpp,md}"]
    pattern: '(?i)\b(TODO|FIXME|XXX|HACK)\s*\(\s*(claude|gpt|copilot|cursor|gemini|codex|aider|chatgpt)\b'
    level: warning
    message: >-
      Agent-attributed TODO marker. Resolve, convert to a
      tracked issue, or remove the model attribution — these
      outlive the session that wrote them.
```
