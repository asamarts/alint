---
title: 'alint baseline'
description: 'alint baseline snapshots the violations a repo has today, so CI fails only on new ones: the way to adopt alint as a blocking gate on an existing codebase.'
---

On an existing codebase, turning alint on usually surfaces violations nobody
will fix in one sitting. `alint baseline` records them in a file you commit;
`alint check --baseline` then fails only on violations that aren't in it, so
the gate blocks new debt from day one while the old debt is paid down.

## Examples

Snapshot today's violations (writes `.alint-baseline.json`):

```bash
alint baseline
```

Gate CI on the delta:

```bash
alint check --baseline .alint-baseline.json
```

After fixing some entries, re-run to prune them:

```bash
alint baseline
```

Re-running `alint baseline` drops the entries that no longer fail. If the tree
has any violation the file doesn't already hold, it writes nothing and exits 2
unless you pass `--accept-new`, so a refresh can't quietly grandfather fresh
violations.

## See also

- [Baseline](/docs/concepts/adoption/baseline/): the full adoption workflow
