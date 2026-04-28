---
title: 'markdown_paths_resolve'
description: 'alint rule kind `markdown_paths_resolve` (Git hygiene family).'
sidebar:
  order: 2
---

Validate that backticked workspace paths in markdown files resolve to real files or directories in the repo. Targets the AGENTS.md / CLAUDE.md / `.cursorrules` staleness problem: agent-context files reference paths in inline backticks (`` `src/api/users.ts` ``), and those paths drift as the codebase evolves. The `agent-context-no-stale-paths` rule shipped in v0.6 surfaces *candidates* via a regex; this rule does the precise existence check.

```yaml
- id: agents-md-paths-resolve
  kind: markdown_paths_resolve
  paths:
    - AGENTS.md
    - CLAUDE.md
    - .cursorrules
    - "docs/**/*.md"
  prefixes:
    - src/
    - crates/
    - docs/
  level: warning
```

The `prefixes` list is **required** — a backticked token must start with one of these to be considered a path candidate. No defaults: every project's layout differs, and a missing prefix is silent while a wrong default trips false positives.

The scanner skips fenced code blocks (```` ``` ```` / `~~~`) and 4-space-indented blocks; those contain code samples, not factual claims about the tree. Trailing `:line` / `#L<n>` location suffixes are stripped before lookup, as are trailing punctuation and trailing slashes. Glob characters (`*`, `?`, `[`) trigger globset matching against the file index — pass if at least one file matches.

By default the rule skips backticked tokens containing template-variable markers (`{{ }}`, `${ }`, `<…>`). Set `ignore_template_vars: false` to validate them as literal paths.

Check-only — auto-fixing a stale path means guessing the new location, which is unsafe.

