---
title: 'git_commit_gpg_signed'
description: 'Assert every commit in scope has a verifying signature (git verify-commit exits 0). alint git_commit_gpg_signed rule, git hygiene family.'
sidebar:
  order: 10
categories: ['git-hygiene', 'security-unicode-sanity']
---

Assert every commit in scope has a verifying signature (`git verify-commit` exits 0). A commit that is unsigned — or signed with a key that doesn't verify against the local keyring — fires one violation. Demand: kernel maintainers, security-sensitive OSS, anyone using GitHub's "Require signed commits" branch protection.

The rule reflects git's own verdict and deliberately does **not** distinguish "unsigned" from "signed with an untrusted key" — trust is git's GPG config / `.git/allowed_signers`, not this rule's job. No configuration knobs. Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `include_merges` | boolean |  | `false` | When validating a range (`since:` set), include merge commits. Has no effect when `since:` is unset; combining `include_merges: true` with no `since:` is a load-time error. |
| `since` | string |  | `null` | Git ref to use as the base of the commit range. When set, validates every commit in `<since>..HEAD` instead of just HEAD. Accepts anything `git rev-parse` does. Use the canonical `{{env.X}}` interpolation to pass a SHA via an env var, e.g. `since: "{{env.ALINT_BASE_SHA \| default('origin/main')}}"`. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### An unsigned HEAD commit

The rule fires on this repository:

```text
README.md
```

`README.md`:

```markdown
# demo
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: signed-commits
    kind: git_commit_gpg_signed
    level: error
```

committed with this history (oldest first):

```text
2024-01-15T09:00:00+00:00  feat: an unsigned change
```

`alint check` reports:

```ansi
[2m--- Repository-level -----------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2msigned-commits[0m
              commit 57cf94a: is not signed (or its signature did not verify)
              (subject: "feat: an unsigned change")

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

