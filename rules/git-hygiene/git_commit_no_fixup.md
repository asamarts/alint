---
title: 'git_commit_no_fixup'
description: 'Fail on residual fixup! / squash! / amend! commits left in scope, the ones git commit --fixup / --squash produce, meant to be collapsed by.'
sidebar:
  order: 7
---

Fail on residual `fixup!` / `squash!` / `amend!` commits left in scope — the ones `git commit --fixup` / `--squash` produce, meant to be collapsed by `git rebase --autosquash` before merging. Forgetting to rebase is the universal case; this rule catches the leftover so it doesn't land on the main branch.

```yaml
# Range mode for PR CI: no un-squashed fixups may merge.
- id: no-fixup
  kind: git_commit_no_fixup
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

No configuration knobs — the matched subject prefixes are exactly what `--autosquash` understands. Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes.

