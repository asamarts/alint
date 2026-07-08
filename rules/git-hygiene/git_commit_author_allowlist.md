---
title: 'git_commit_author_allowlist'
description: 'Assert every commit author in scope matches an allowed email and/or name pattern. alint git_commit_author_allowlist rule, git hygiene family.'
sidebar:
  order: 9
categories: ['git-hygiene']
---

Assert every commit author in scope matches an allowed email and/or name pattern. At least one of `email_pattern:` / `name_pattern:` is required; specifying both means BOTH must match (AND). A commit whose author fails any specified pattern fires one violation. Demand: enterprise repos enforcing contributor identity against a corporate domain; OSS projects catching commits from sock-puppet or compromised accounts.

```yaml
# Every commit in the PR must be authored from the corporate domain.
- id: org-authors-only
  kind: git_commit_author_allowlist
  email_pattern: '^.+@example\.com$'
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

`email_pattern:` matches `git log %ae`; `name_pattern:` matches `git log %an`. Both are Rust regexes. Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes (silent outside a git repo; a bad `since:` ref hard-fails with a shallow-clone hint).

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `email_pattern` | string |  | `null` | Rust-regex the author email (`git log %ae`) must match, e.g. `^.+@example\.com$`. |
| `include_merges` | boolean |  | `false` | When validating a range (`since:` set), include merge commits. Has no effect when `since:` is unset; combining `include_merges: true` with no `since:` is a load-time error. |
| `name_pattern` | string |  | `null` | Rust-regex the author name (`git log %an`) must match. |
| `since` | string |  | `null` | Git ref to use as the base of the commit range. When set, validates every commit in `<since>..HEAD` instead of just HEAD. Accepts anything `git rev-parse` does. Use the canonical `{{env.X}}` interpolation to pass a SHA via an env var, e.g. `since: "{{env.ALINT_BASE_SHA \| default('origin/main')}}"`. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
