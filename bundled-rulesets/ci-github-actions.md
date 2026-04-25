---
title: 'ci/github-actions@v1'
description: Bundled alint ruleset at alint://bundled/ci/github-actions@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/ci/github-actions@v1
```

## Rules

### `gha-workflow-contents-read`

- **kind**: `yaml_path_equals`
- **level**: `warning`
- **policy**: <https://docs.github.com/en/actions/security-guides/automatic-token-authentication#permissions-for-the-github_token>

> GitHub workflows should declare `permissions.contents: read` at the workflow level. Workflows that truly need write can override this rule or set per-job permissions.

### `gha-pin-actions-to-sha`

- **kind**: `yaml_path_matches`
- **level**: `warning`
- **policy**: <https://docs.github.com/en/actions/security-guides/security-hardening-for-github-actions#using-third-party-actions>

> Third-party action is not pinned to a commit SHA. Pin with `@<40-char-sha>  # v4.1.1` so a compromised tag can't silently change what runs.

### `gha-workflow-has-name`

- **kind**: `yaml_path_matches`
- **level**: `info`

> Workflow has no `name:` field; the Actions UI will show the filename instead. Add a human-readable `name:` at the top.

