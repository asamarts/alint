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
violations. (Upgrading from v0.23 on Windows with a baseline covering
`for_each_*` nested rules that use `{path}` or `{dir}` needs one
`alint baseline --accept-new` regeneration because path tokens now render with
`/` on every platform.)

A baseline path that comes from the repository (the `baseline:` config key or
the default `.alint-baseline.json`) must stay inside the repository, outside any
Git directory (`.git`, or through a symlink to one), and alint refuses to write
through a symlink; any of these exits 2. Pass `--output` to write the file
somewhere else on purpose.

Analysis failures are not policy debt and cannot be grandfathered. If any rule
could not obtain a verdict, `alint baseline` writes nothing, reports each
failure, and exits `1`; `--accept-new` does not override this refusal.

## Reference date for time-dependent rules

`alint baseline --as-of YYYY-MM-DD` pins the strict UTC date used to evaluate
time-dependent findings before they are fingerprinted. The flag overrides
`ALINT_AS_OF`; without either, alint uses the current HEAD commit's committer
timestamp. An explicit date removes that HEAD-time lookup, but a rule such as
`git_blame_age` still requires a usable Git repository for tracked-file and blame
data.

## See also

- [Baseline](/docs/concepts/adoption/baseline/): the full adoption workflow
