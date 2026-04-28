---
title: 'git_commit_message'
description: 'alint rule kind `git_commit_message` (Git hygiene family).'
sidebar:
  order: 4
---

Validate HEAD's commit-message shape via regex, max-subject-length, or required-body. At least one of the three must be set; combine all three for full Conventional-Commits-style enforcement. Subject length counts characters, not bytes (a 50-char emoji subject is 50, not 200).

```yaml
- id: conventional-commit
  kind: git_commit_message
  pattern: '^(feat|fix|chore|docs|refactor|test)(\([a-z-]+\))?: '
  subject_max_length: 72
  level: warning

- id: bug-fixes-need-context
  kind: git_commit_message
  pattern: '^fix:'
  requires_body: true
  level: error
  message: "fix: commits must explain what was broken in the body."
```

Outside a git repo, with no commits yet, or when `git` isn't on `PATH`, the rule silently no-ops. Pairs naturally with `alint check --changed` for per-PR enforcement: every PR's tip commit gets validated automatically.

---

