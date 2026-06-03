---
title: 'git_commit_subject_matches'
description: 'Each commit''s subject line (the first line of its message) must match the matches: regex, the subject-grammar member of the commit family.'
sidebar:
  order: 8
---

Each commit's subject line (the first line of its message) must match the `matches:` regex — the subject-grammar member of the commit family. Enforces a prefix + shape convention like go / Gerrit's `pkg/path: lowercase summary`, node's `subsystem: description`, or conventional-commit types. The regex is anchored to the **subject alone** (so `^…$` describes the first line exactly), unlike `git_commit_message`'s `pattern:` which matches the whole subject + body; for a subject-length cap use `git_commit_message`'s `subject_max_length:`. Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes (HEAD-only when `since:` is unset, `<since>..HEAD` when set; silent outside a git repo; a bad `since:` ref hard-fails with a shallow-clone hint).

```yaml
- id: subject-grammar
  kind: git_commit_subject_matches
  matches: '^[a-z0-9_/.-]+: [a-z].{0,70}$'   # `component: lowercase summary`
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

