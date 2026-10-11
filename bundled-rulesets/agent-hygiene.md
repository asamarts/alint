---
title: 'agent-hygiene@v1'
description: 'agent-hygiene@v1 bundled alint ruleset: Hygiene rules for the AI-coding era, patterns that show up disproportionately in commits authored or co-authored...'
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

## What it checks

6 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [agent-no-versioned-duplicates](#agent-no-versioned-duplicates)<br>`warning` | Filename looks like a versioned duplicate (e.g. app\_old.js, api\_FINAL.py, utils\_copy.ts). |
| [agent-no-scratch-docs-at-root](#agent-no-scratch-docs-at-root)<br>`warning` | Scratch / planning documents should not be committed at the repo root. |
| [agent-no-affirmation-prose](#agent-no-affirmation-prose)<br>`info` | AI-style affirmation phrasing in committed content. |
| [agent-no-console-log](#agent-no-console-log)<br>`warning` | `console.log` / `.debug` / `.trace` left in non-test source. |
| [agent-no-debugger-statements](#agent-no-debugger-statements)<br>`error` | `debugger;` / `breakpoint()` must not be committed. |
| [agent-no-model-todos](#agent-no-model-todos)<br>`warning` | Agent-attributed TODO marker. |

## Rules

### `agent-no-versioned-duplicates`

Agents tend to write `app_old.js` / `api_FINAL.py` / `utils_copy.ts` instead of replacing the original. Combined with `hygiene-no-editor-backups` from `no-tracked-artifacts@v1` (which catches `*.bak` / `*~` / `*.swp`), this gives broad coverage of the leftover-artefact filename surface.

Deliberately not matched: `*_v[0-9]*` and `*-v[0-9]*`. Real codebases use those for legitimate versioning — API versions (`gitlab_v1_callback.py`), schema migrations (`076_add_v1_tables.py`), release notes (`release-notes-v1.md`), and versioned tests (`test_v1_api.py`). The other suffixes below are unambiguous — `_old`, `_new`, `_FINAL`, `_copy`, `_backup`, `.copy.` are almost never legitimate part of a real filename's identity.

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`

> Filename looks like a versioned duplicate (e.g. app\_old.js, api\_FINAL.py, utils\_copy.ts). Replace the original instead of keeping a parallel copy.

```yaml
- id: agent-no-versioned-duplicates
  kind: file_absent
  paths:
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
    app_old.js, api_FINAL.py, utils_copy.ts). Replace the
    original instead of keeping a parallel copy.
```

### `agent-no-scratch-docs-at-root`

Agents spawn planning files (PLAN.md, NOTES.md, ANALYSIS.md, …) as part of their workflow and frequently forget to delete them before committing. Best-practice AGENTS.md templates explicitly tell agents to remove these post-merge — this rule enforces the discipline.

`root_only: true` so a legitimate `notes.md` deeper in the tree (e.g. `docs/notes.md` for a feature, or a per-package `packages/foo/NOTES.md`) does not trigger.

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`

> Scratch / planning documents should not be committed at the repo root. Move the content into a real doc (CHANGELOG, ADR, design doc, README) or delete it once the work is done.

```yaml
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
```

### `agent-no-affirmation-prose`

Reviewers consistently flag these stock phrases as "AI smell." The pattern is narrow enough that legitimate code shouldn't match — info-level so it's a soft nudge, not a hard gate.

The exclude list covers content that legitimately QUOTES AI-style text from upstream sources (CHANGELOG entries, roadmap rationale, cookbook examples) or captures it as test fixture output (snapshot tests, fixtures dirs).

- **kind**: [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/)
- **level**: `info`

> AI-style affirmation phrasing in committed content. These are characteristic of agent-authored prose; trim before merge.

```yaml
- id: agent-no-affirmation-prose
  kind: file_content_forbidden
  paths:
    include: ["**/*.{rs,ts,tsx,js,jsx,py,go,java,kt,rb,md}"]
    exclude:
      - "**/*test*/**"
      - "**/__tests__/**"
      - "**/fixtures/**"
      - "**/CHANGELOG*"
      - "**/ROADMAP*"
      - "**/*.snap"
  pattern: "(?i)(you'?re absolutely right|excellent question|happy to help|great (point|question)|let me know if you need)"
  level: info
  message: >-
    AI-style affirmation phrasing in committed content. These
    are characteristic of agent-authored prose; trim before
    merge.
```

### `agent-no-console-log`

`console.log` / `.debug` / `.trace` left in non-test JS / TS sources. The exclude list balances catching real production leftovers vs. legitimate logging in build tooling, browser demos, and vendored content.

The leading `(?:^|[\s;{(])` keeps the rule from matching `myconsole.log(...)` or other false positives where `console` is part of a longer identifier.

- **kind**: [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/)
- **level**: `warning`

> `console.log` / `.debug` / `.trace` left in non-test source. Route through the project logger or remove before merge.

```yaml
- id: agent-no-console-log
  kind: file_content_forbidden
  paths:
    include: ["**/*.{ts,tsx,js,jsx,mjs,cjs}"]
    exclude:
      # Test files and directories — `**/*test*/**` matches
      # any segment containing "test" (`tests/`, `e2e-tests/`,
      # `cross-sdk-tests/`, `test_helpers/`, …) — broader than
      # `**/test*/**` which only matches segments STARTING
      # with "test".
      - "**/*.{test,spec}.*"
      - "**/*test*/**"
      - "**/__tests__/**"
      - "**/fixtures/**"
      # Build / dev tooling config files (vite.config.ts,
      # rollup.config.mjs, etc.) often log intentionally.
      - "**/*.config.*"
      # Build / utility scripts — agent harnesses and CI
      # glue legitimately log; src/ is where this rule earns
      # its keep.
      - "**/scripts/**"
      # Browser-facing demos and websites — `console.log` is a
      # legitimate browser-debugging tool, and the codebase
      # may keep example logs intentionally.
      - "**/website/**"
      - "**/public/**"
      - "**/demo/**"
      - "**/demos/**"
      - "**/examples/**"
      # Vendored and third-party code the project doesn't own.
      - "**/vendor/**"
      - "**/vendored/**"
      - "**/third_party/**"
      - "**/3rdparty/**"
      # Agent worktrees and harness scratch space — these are
      # ephemeral copies of the working tree (e.g. Claude
      # Code's `/parallel` worktrees), not real source.
      - "**/.claude/**"
  pattern: '(?:^|[\s;{(])console\.(log|debug|trace)\s*\('
  level: warning
  message: >-
    `console.log` / `.debug` / `.trace` left in non-test
    source. Route through the project logger or remove
    before merge.
```

### `agent-no-debugger-statements`

Require `;` after `debugger` so the rule doesn't trip on the WORD "debugger" appearing in prose comments (`* called by the vscode debugger`). Same idea for `breakpoint()` — must include the parens, not just the word.

- **kind**: [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/)
- **level**: `error`

> `debugger;` / `breakpoint()` must not be committed. These halt execution at runtime. Remove before merge.

```yaml
- id: agent-no-debugger-statements
  kind: file_content_forbidden
  paths:
    include: ["**/*.{ts,tsx,js,jsx,mjs,cjs,py}"]
    exclude:
      - "**/*.{test,spec}.*"
      - "**/*test*/**"
      - "**/__tests__/**"
      - "**/fixtures/**"
      - "**/scripts/**"
      - "**/website/**"
      - "**/public/**"
      - "**/demo/**"
      - "**/demos/**"
      - "**/examples/**"
      - "**/vendor/**"
      - "**/vendored/**"
      - "**/third_party/**"
      - "**/3rdparty/**"
      - "**/.claude/**"
  pattern: '(?:^|[\s;{(])(debugger\s*;|breakpoint\s*\(\s*\))'
  level: error
  message: >-
    `debugger;` / `breakpoint()` must not be committed.
    These halt execution at runtime. Remove before merge.
```

### `agent-no-model-todos`

`TODO(claude:)`, `FIXME(cursor:)`, `XXX(gpt:)` etc. — TODO markers that name a coding agent. They outlive the session that wrote them and are typically actionable items the agent intended to come back to but never did.

Excludes documentation and changelogs — projects that \*describe\* these patterns (alint's own ROADMAP / CHANGELOG / cookbook are the canonical case) trip the rule on their own examples otherwise.

- **kind**: [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/)
- **level**: `warning`

> Agent-attributed TODO marker. Resolve, convert to a tracked issue, or remove the model attribution. These outlive the session that wrote them.

```yaml
- id: agent-no-model-todos
  kind: file_content_forbidden
  paths:
    include:
      ["**/*.{rs,ts,tsx,js,jsx,py,go,java,kt,rb,scala,c,cc,cpp,h,hpp,md}"]
    exclude:
      - "**/CHANGELOG*"
      - "**/ROADMAP*"
      - "**/cookbook/**"
      - "**/*test*/**"
      - "**/__tests__/**"
      - "**/fixtures/**"
  pattern: '(?i)\b(TODO|FIXME|XXX|HACK)\s*\(\s*(claude|gpt|copilot|cursor|gemini|codex|aider|chatgpt)\b'
  level: warning
  message: >-
    Agent-attributed TODO marker. Resolve, convert to a
    tracked issue, or remove the model attribution. These
    outlive the session that wrote them.
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/agent-hygiene@v1
rules:
  - id: agent-no-versioned-duplicates
    level: off
  - id: agent-no-scratch-docs-at-root
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/agent-hygiene@v1
    except: [agent-no-versioned-duplicates]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/agent-hygiene.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/agent-hygiene.yml) in the alint repo.
