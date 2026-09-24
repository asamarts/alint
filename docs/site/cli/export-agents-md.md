---
title: 'alint export-agents-md'
description: 'alint export-agents-md turns the active lint rules into an AGENTS.md section, so coding agents read the same rules alint enforces.'
---

Coding agents read `AGENTS.md` before they write code. `alint export-agents-md`
generates a section listing the rules your config enforces, so the agent's
instructions and the lint gate can't drift apart. Re-run it after changing
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

## See also

- [The agent surface](/docs/concepts/agents/the-agent-surface/)
