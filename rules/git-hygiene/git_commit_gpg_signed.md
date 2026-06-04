---
title: 'git_commit_gpg_signed'
description: 'Assert every commit in scope has a verifying signature (git verify-commit exits 0). alint git_commit_gpg_signed rule, git hygiene family.'
sidebar:
  order: 9
---

Assert every commit in scope has a verifying signature (`git verify-commit` exits 0). A commit that is unsigned — or signed with a key that doesn't verify against the local keyring — fires one violation. Demand: kernel maintainers, security-sensitive OSS, anyone using GitHub's "Require signed commits" branch protection.

```yaml
# Every commit in the PR must carry a verifying signature.
- id: signed-commits
  kind: git_commit_gpg_signed
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

The rule reflects git's own verdict and deliberately does **not** distinguish "unsigned" from "signed with an untrusted key" — trust is git's GPG config / `.git/allowed_signers`, not this rule's job. No configuration knobs. Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes.

