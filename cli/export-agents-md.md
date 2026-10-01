---
title: 'alint export-agents-md'
description: 'alint export-agents-md turns the active lint rules into an AGENTS.md section, so coding agents read the same rules alint enforces.'
---

Coding agents read `AGENTS.md` before they write code. `alint export-agents-md`
generates a section listing the rules your config enforces, so the agent's
instructions match what the lint gate checks. Re-run it after changing
`.alint.yml`.

## Examples

Print the section to stdout:

```bash
alint export-agents-md
```

Keep `AGENTS.md` in sync between the `alint:start` / `alint:end` markers:

```bash
alint export-agents-md --inline --output AGENTS.md
```

## Reference

```
Generate an `AGENTS.md` section from the active rule set.

Keeps the agent's pre-prompt directives in sync with the lint config. Outputs to stdout by default;
use `--output PATH` to write a file, or `--inline --output PATH` to splice between `<!-- alint:start
-->` / `<!-- alint:end -->` markers.

Usage: alint export-agents-md [OPTIONS]

Options:
      --output <PATH>
          Output destination. Without `--inline`, the file is overwritten. Omit for stdout

      --inline
          Splice the generated section between `<!-- alint:start -->` and `<!-- alint:end -->`
          markers in `--output PATH`. Markers are auto-created (with a stderr warning) when the
          target file lacks them

      --section-title <TEXT>
          Heading text for the generated section. Default: "Lint rules enforced by alint"

      --include-info
          Include `level: info` rules. Default omits them - info-level rules are nudges, not
          directives

  -f, --format <FORMAT>
          Output format. `markdown` (default) is the canonical `AGENTS.md` shape; `json` is parallel
          to `suggest`'s JSON envelope for agent consumption

          [default: markdown]
          [possible values: markdown, json]
```

The [global options](/docs/cli/#global-options) apply to `alint export-agents-md` too, where they are relevant. Its own `--format` above replaces the global one.

## See also

- [The agent surface](/docs/concepts/agents/the-agent-surface/)
