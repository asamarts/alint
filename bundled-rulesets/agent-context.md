---
title: 'agent-context@v1'
description: Bundled alint ruleset at alint://bundled/agent-context@v1.
---

Hygiene rules for the agent-instruction files coding agents
read on every session — `AGENTS.md` (the cross-tool standard
backed by agents.md / OpenAI Codex), `CLAUDE.md`, GitHub
Copilot's `.github/copilot-instructions.md`, Cursor's
`.cursorrules`, Gemini's `GEMINI.md`. These files share a
failure mode: they outlive the code they describe, accumulate
stale references, and bloat past the point where agents can
usefully consume them.

Adopt with:

```yaml
extends:
  - alint://bundled/agent-context@v1
```

The ruleset is gated by `facts.has_agent_context`, so it's a
safe no-op in repos that don't ship any agent-context file —
extend it unconditionally even from polyglot / mixed configs.

Defaults are non-blocking (`info` for the existence and bloat
checks, `warning` for the stub guard) so the ruleset nudges
without gating merges. Override severity once you've
normalised your context-file shape.

Sourcing for the bloat threshold: Augment Code's 2026-03
research on AGENTS.md effectiveness found that context files
beyond ~200-300 lines correlate with worse agent performance
(cited in InfoQ "New Research Reassesses the Value of
AGENTS.md" and the ctxlint linter's `max-lines` default).

## Rules

### `agent-context-recommended`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **policy**: <https://agents.md>

> Consider adding an AGENTS.md (or CLAUDE.md / .cursorrules) so coding agents have shared, versioned instructions.

### `agent-context-non-stub`

- **kind**: [`file_min_lines`](/docs/rules/content/file_min_lines/)
- **level**: `warning`
- **when**: `facts.has_agent_context`

> Agent-context file is suspiciously short. Either fill it in with real guidance or remove it — empty context files mislead agents that load them.

### `agent-context-not-bloated`

- **kind**: [`file_max_lines`](/docs/rules/content/file_max_lines/)
- **level**: `info`
- **when**: `facts.has_agent_context`
- **policy**: <https://www.augmentcode.com/blog/how-to-write-good-agents-dot-md-files>

> Agent-context file is large. Consider splitting into focused sub-docs and linking them from the root file — bloated context crowds the agent's prompt budget.

### `agent-context-no-stale-paths`

- **kind**: [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/)
- **level**: `info`
- **when**: `facts.has_agent_context`

> Agent-context file references workspace paths in backticks. Verify each path still resolves — context files commonly outlive the code they describe. (The v0.7 `markdown_paths_resolve` rule kind will validate this precisely.)

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/agent-context.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/agent-context.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/agent-context@v1
#
# Hygiene rules for the agent-instruction files coding agents
# read on every session — `AGENTS.md` (the cross-tool standard
# backed by agents.md / OpenAI Codex), `CLAUDE.md`, GitHub
# Copilot's `.github/copilot-instructions.md`, Cursor's
# `.cursorrules`, Gemini's `GEMINI.md`. These files share a
# failure mode: they outlive the code they describe, accumulate
# stale references, and bloat past the point where agents can
# usefully consume them.
#
# Adopt with:
#
#     extends:
#       - alint://bundled/agent-context@v1
#
# The ruleset is gated by `facts.has_agent_context`, so it's a
# safe no-op in repos that don't ship any agent-context file —
# extend it unconditionally even from polyglot / mixed configs.
#
# Defaults are non-blocking (`info` for the existence and bloat
# checks, `warning` for the stub guard) so the ruleset nudges
# without gating merges. Override severity once you've
# normalised your context-file shape.
#
# Sourcing for the bloat threshold: Augment Code's 2026-03
# research on AGENTS.md effectiveness found that context files
# beyond ~200-300 lines correlate with worse agent performance
# (cited in InfoQ "New Research Reassesses the Value of
# AGENTS.md" and the ctxlint linter's `max-lines` default).

version: 1

facts:
  - id: has_agent_context
    any_file_exists:
      - AGENTS.md
      - CLAUDE.md
      - .cursorrules
      - GEMINI.md
      - .github/copilot-instructions.md

rules:
  # --- Existence (recommended, not required) ----------------------
  # Most agent-heavy repos benefit from a single shared context
  # file. Stay info-level so the rule is a nudge, not a gate —
  # plenty of fine repos don't (yet) ship one.
  - id: agent-context-recommended
    kind: file_exists
    paths:
      - AGENTS.md
      - CLAUDE.md
      - .cursorrules
    root_only: true
    level: info
    message: >-
      Consider adding an AGENTS.md (or CLAUDE.md / .cursorrules)
      so coding agents have shared, versioned instructions.
    policy_url: "https://agents.md"

  # --- Stub guard --------------------------------------------------
  # An empty AGENTS.md is worse than no AGENTS.md — it implies
  # the file is authoritative when it actually contains no
  # guidance. 10 lines is a generous floor; most useful context
  # files run 50-200.
  - id: agent-context-non-stub
    when: facts.has_agent_context
    kind: file_min_lines
    paths:
      - AGENTS.md
      - CLAUDE.md
      - .cursorrules
      - GEMINI.md
      - .github/copilot-instructions.md
    min_lines: 10
    level: warning
    message: >-
      Agent-context file is suspiciously short. Either fill it
      in with real guidance or remove it — empty context files
      mislead agents that load them.

  # --- Bloat guard -------------------------------------------------
  # Context files compete for the agent's prompt budget. Past
  # ~300 lines, they crowd out the actual task and correlate
  # with worse agent performance. Use `info` severity since the
  # ceiling is heuristic and some teams legitimately ship larger
  # context (e.g. complex DSL grammars to teach an agent).
  - id: agent-context-not-bloated
    when: facts.has_agent_context
    kind: file_max_lines
    paths:
      - AGENTS.md
      - CLAUDE.md
      - .cursorrules
      - GEMINI.md
      - .github/copilot-instructions.md
    max_lines: 300
    level: info
    message: >-
      Agent-context file is large. Consider splitting into
      focused sub-docs and linking them from the root file —
      bloated context crowds the agent's prompt budget.
    policy_url: "https://www.augmentcode.com/blog/how-to-write-good-agents-dot-md-files"

  # --- Stale-path heuristic ---------------------------------------
  # Context files reference source paths in backticks. Those
  # paths drift as the codebase evolves. This is a heuristic
  # reminder rule — the precise check (validate every backticked
  # path resolves to a real file) ships in v0.7 as the
  # `markdown_paths_resolve` rule kind.
  #
  # The pattern matches backticked tokens that look like
  # workspace-relative source paths (`src/`, `crates/`,
  # `packages/`, `apps/`, `services/`, `docs/`). Info-level so
  # users self-audit when running `alint check`, not gate
  # commits.
  - id: agent-context-no-stale-paths
    when: facts.has_agent_context
    kind: file_content_forbidden
    paths:
      - AGENTS.md
      - CLAUDE.md
    pattern: '(?m)`(?:src|crates|packages|apps|services|docs)/[^`]*`'
    level: info
    message: >-
      Agent-context file references workspace paths in
      backticks. Verify each path still resolves — context
      files commonly outlive the code they describe. (The
      v0.7 `markdown_paths_resolve` rule kind will validate
      this precisely.)
```
