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

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `include_merges` | boolean |  | `false` | When validating a range (`since:` set), include merge commits. Has no effect when `since:` is unset; combining `include_merges: true` with no `since:` is a load-time error. |
| `pattern` | string |  | `null` | Trailer pattern each commit message must contain. Defaults to the canonical DCO sign-off shape `(?m)^Signed-off-by: .+ <.+@.+>$`. Override to enforce a stricter form (e.g. a corporate-domain email). |
| `since` | string |  | `null` | Git ref to use as the base of the commit range. When set, validates every commit in `<since>..HEAD` instead of just HEAD. Accepts anything `git rev-parse` does. Use the canonical `{{env.X}}` interpolation to pass a SHA via an env var, e.g. `since: "{{env.ALINT_BASE_SHA \| default('origin/main')}}"`. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
