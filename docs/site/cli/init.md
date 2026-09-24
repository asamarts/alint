---
title: 'alint init'
description: 'alint init writes a starter .alint.yml for the ecosystem it detects, extending the matching bundled rulesets. Add --monorepo for workspace overlays.'
---

`alint init` detects the repository's ecosystem and writes a starter
`.alint.yml` that extends the matching bundled rulesets. It never overwrites
an existing config. Pair it with [`alint suggest`](/docs/cli/suggest/) to
find rules the starter doesn't cover.

## Examples

Write a starter config for the detected ecosystem:

```bash
alint init
```

In a workspace: add the monorepo overlays and enable nested configs:

```bash
alint init --monorepo
```

## See also

- [Quickstart](/docs/getting-started/quickstart/)
- [Bundled rulesets](/docs/bundled-rulesets/)
