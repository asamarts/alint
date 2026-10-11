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

A baseline path that comes from the repository (the `baseline:` config key or
the default `.alint-baseline.json`) must stay inside the repository, and alint
refuses to write through a symlink; either case exits 2. Pass `--output` to
write the file somewhere else on purpose.

## Reference

```
Snapshot current violations so later runs fail only on new ones.

Writes them to a baseline file, so a later `alint check --baseline <file>` fails only on NEW
violations. The one-step way to adopt alint as a blocking gate on a legacy repo: `alint baseline`
(commit it), then gate on the delta. The baseline is whole-tree; `--changed` is not accepted.

Usage: alint baseline [OPTIONS] [PATH]

Arguments:
  [PATH]
          Root of the repository to snapshot. Defaults to the current directory

          [default: .]

Options:
      --output <FILE>
          Where to write the baseline. Default: `.alint-baseline.json` at the repo root

      --accept-new
          Allow the regenerated baseline to grandfather violations not already present in the
          existing file. Without it, `alint baseline` refuses to ADD new entries (and prints a `+N /
          -M` summary) so re-running it to prune fixed entries can't silently accept new debt.
          Stale-entry removal never needs it
```

The [global options](/docs/cli/#global-options) apply to `alint baseline` too, where they are relevant.

## See also

- [Baseline](/docs/concepts/adoption/baseline/): the full adoption workflow
