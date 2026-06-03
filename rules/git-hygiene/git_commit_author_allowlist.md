---
title: 'git_commit_author_allowlist'
description: 'Assert every commit author in scope matches an allowed email and/or name pattern. alint git_commit_author_allowlist rule, git hygiene family.'
sidebar:
  order: 9
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

