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

`--untrusted` loads the config as [`alint check --untrusted`](/docs/cli/check/) does: a remote
`extends:` and `allow_out_of_root:` are refused and `{{env.NAME}}` reads an empty environment.
With it or `--no-spawn`, a config whose rules or facts would start a process or run Git fails to
load, as it does for `alint list`, though `export-agents-md` runs no rule.

## Version 2 selectors and relations

For an [experimental version 2 config](/docs/configuration/version-2/), the section also lists
each named selector and relation with the config file that declares it, followed by the
`extends:` aliases that reach those files, once per file. `--format json` adds the same
`sources`, `selectors` and `relations` lists as
[`alint list --format json`](/docs/cli/list/#version-2-selectors-and-relations).

## See also

- [The agent surface](/docs/concepts/agents/the-agent-surface/)
- [Version 2 (experimental)](/docs/configuration/version-2/)
