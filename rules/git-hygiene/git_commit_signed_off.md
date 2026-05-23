---
title: 'git_commit_signed_off'
description: 'Assert every commit in scope carries a DCO (Developer Certificate of Origin) Signed-off-by: trailer, required by every CNCF / Linux.'
sidebar:
  order: 6
---

Assert every commit in scope carries a DCO (Developer Certificate of Origin) `Signed-off-by:` trailer — required by every CNCF / Linux Foundation / kernel-style project. A commit lacking the trailer fires one violation, with the short SHA + subject snippet so you know which to amend (`git commit --amend -s` or `git rebase --signoff`).

```yaml
# HEAD-only: the tip commit must be signed off.
- id: dco
  kind: git_commit_signed_off
  level: error

# Range mode for PR CI: every commit in the PR must be signed off.
- id: pr-dco
  kind: git_commit_signed_off
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

The default `pattern:` is the canonical DCO shape `(?m)^Signed-off-by: .+ <.+@.+>$`. Override `pattern:` to enforce a stricter form (e.g. a corporate-domain email). Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes (silent outside a git repo; a bad `since:` ref hard-fails with a shallow-clone hint). See [variable interpolation](/docs/concepts/variable-interpolation/) for the `{{env.X}}` form.

