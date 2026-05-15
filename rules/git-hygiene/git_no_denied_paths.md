---
title: 'git_no_denied_paths'
description: 'alint rule kind `git_no_denied_paths` (Git hygiene family).'
sidebar:
  order: 4
---

Fire when any tracked file matches a configured glob denylist. The absence-axis companion of `git_tracked_only`: instead of asking "does this tracked path exist?", it asks "is anything tracked that matches my denylist?" One rule covers what would otherwise need one `file_absent` per pattern. Reports every matching denylist entry per offending path so a single file hitting two patterns surfaces both.

```yaml
- id: no-secrets-or-keys
  kind: git_no_denied_paths
  denied:
    - "*.env"
    - ".env*"
    - "*.pem"
    - "id_rsa"
    - "secrets/**"
  level: error
  message: "Don't commit secrets or credentials."
```

Outside a git repo (or when `git` isn't on `PATH`) the rule silently no-ops — the rule's intent only makes sense inside a tracked working tree. Check-only — `git rm --cached` is too destructive to automate.

