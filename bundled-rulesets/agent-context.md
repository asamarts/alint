---
title: 'agent-context@v1'
description: 'agent-context@v1 bundled alint ruleset: Hygiene rules for the agent-instruction files coding agents read on every session, AGENTS.md...'
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

## What it checks

4 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [agent-context-recommended](#agent-context-recommended)<br>`info` | Add AGENTS.md / CLAUDE.md / .cursorrules so coding agents share versioned instructions. |
| [agent-context-non-stub](#agent-context-non-stub)<br>`warning` | Agent-context file is suspiciously short. |
| [agent-context-not-bloated](#agent-context-not-bloated)<br>`info` | Agent-context file is large. |
| [agent-context-no-stale-paths](#agent-context-no-stale-paths)<br>`info` | Agent-context file references a workspace path that no longer resolves. |

3 rules run only when `facts.has_agent_context` holds.

## Rules

### `agent-context-recommended`

Most agent-heavy repos benefit from a single shared context file. Stay info-level so the rule is a nudge, not a gate — plenty of fine repos don't (yet) ship one.

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **policy**: <https://agents.md>

> Add AGENTS.md / CLAUDE.md / .cursorrules so coding agents share versioned instructions.

```yaml
- id: agent-context-recommended
  kind: file_exists
  paths:
    - AGENTS.md
    - CLAUDE.md
    - .cursorrules
  root_only: true
  level: info
  message: >-
    Add AGENTS.md / CLAUDE.md / .cursorrules so coding
    agents share versioned instructions.
  policy_url: "https://agents.md"
```

### `agent-context-non-stub`

An empty AGENTS.md is worse than no AGENTS.md — it implies the file is authoritative when it actually contains no guidance. 10 lines is a generous floor; most useful context files run 50-200.

- **kind**: [`file_min_lines`](/docs/rules/content/file_min_lines/)
- **level**: `warning`
- **when**: `facts.has_agent_context`

> Agent-context file is suspiciously short. Either fill it in with real guidance or remove it. Empty context files mislead agents that load them.

```yaml
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
    in with real guidance or remove it. Empty context files
    mislead agents that load them.
```

### `agent-context-not-bloated`

Context files compete for the agent's prompt budget. Past \~300 lines, they crowd out the actual task and correlate with worse agent performance. Use `info` severity since the ceiling is heuristic and some teams legitimately ship larger context (e.g. complex DSL grammars to teach an agent).

- **kind**: [`file_max_lines`](/docs/rules/content/file_max_lines/)
- **level**: `info`
- **when**: `facts.has_agent_context`
- **policy**: <https://www.augmentcode.com/blog/how-to-write-good-agents-dot-md-files>

> Agent-context file is large. Consider splitting into focused sub-docs and linking them from the root file. Bloated context crowds the agent's prompt budget.

```yaml
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
    focused sub-docs and linking them from the root file.
    Bloated context crowds the agent's prompt budget.
  policy_url: "https://www.augmentcode.com/blog/how-to-write-good-agents-dot-md-files"
```

### `agent-context-no-stale-paths`

Validate backticked workspace-relative paths instead of flagging every reference as a heuristic reminder. Info-level keeps stale context visible without making it a hard gate.

- **kind**: [`markdown_paths_resolve`](/docs/rules/git-hygiene/markdown_paths_resolve/)
- **level**: `info`
- **when**: `facts.has_agent_context`

> Agent-context file references a workspace path that no longer resolves. Context files commonly outlive the code they describe.

```yaml
- id: agent-context-no-stale-paths
  when: facts.has_agent_context
  kind: markdown_paths_resolve
  paths:
    - AGENTS.md
    - CLAUDE.md
  prefixes: ["src/", "crates/", "packages/", "apps/", "services/", "docs/"]
  level: info
  message: >-
    Agent-context file references a workspace path that no longer
    resolves. Context files commonly outlive the code they describe.
```

## Facts

The `when:` clauses above read these facts. Each is resolved once per run; [`alint facts`](/docs/cli/facts/) prints what they resolved to in your repository.

```yaml
facts:
  - id: has_agent_context
    any_file_exists:
      - AGENTS.md
      - CLAUDE.md
      - .cursorrules
      - GEMINI.md
      - .github/copilot-instructions.md
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/agent-context@v1
rules:
  - id: agent-context-recommended
    level: off
  - id: agent-context-non-stub
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/agent-context@v1
    except: [agent-context-recommended]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/agent-context.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/agent-context.yml) in the alint repo.
