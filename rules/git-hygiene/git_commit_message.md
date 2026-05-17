---
title: 'git_commit_message'
description: 'Validate commit-message shape via regex, max-subject-length, or required-body. alint git_commit_message rule, git hygiene family.'
sidebar:
  order: 5
---

Validate commit-message shape via regex, max-subject-length, or required-body. At least one of the three must be set; combine all three for full Conventional-Commits-style enforcement. Subject length counts characters, not bytes (a 50-char emoji subject is 50, not 200).

Two modes, selected by the optional `since:` field:

- **HEAD-only** (default, `since:` omitted): validate the tip commit. Right shape for push-trigger CI and post-commit hooks.
- **Range** (`since:` set): validate every commit reachable from HEAD but not from `since`. Right shape for `pull_request`-trigger CI, where `actions/checkout` checks out a synthetic merge commit whose subject the rule would always flag.

```yaml
# HEAD-only. The tip commit must follow Conventional Commits and be <= 72 chars.
- id: conventional-commit
  kind: git_commit_message
  pattern: '^(feat|fix|chore|docs|refactor|test)(\([a-z-]+\))?: '
  subject_max_length: 72
  level: warning

# Range mode for PR CI. `ALINT_BASE_SHA` is exported in the workflow from
# `github.event.pull_request.base.sha`; on push-trigger or local runs where
# the env var is unset, falls back to `origin/main`.
- id: pr-conventional-commits
  kind: git_commit_message
  pattern: '^(feat|fix|chore|docs|refactor|test|build|ci|perf|style|revert)(\(.+\))?!?: '
  subject_max_length: 72
  since: ${ALINT_BASE_SHA:-origin/main}
  level: error
```

#### `since:` semantics

`since:` accepts anything `git rev-parse` resolves: a 40-char or abbreviated SHA, a branch (`origin/main`), a tag (`v1.2.3`), or a relative ref (`HEAD~5`). The rule walks `<since>..HEAD` oldest-first, validates each commit, and emits one violation per failing commit with the short SHA + a subject snippet so you know which to amend.

POSIX-style env-var interpolation is supported:

- `${VAR}` substitutes the value of `VAR`. Unset (or empty) is a hard error with a CI-friendly hint.
- `${VAR:-default}` substitutes `VAR`, or `default` when `VAR` is unset or empty.

The GitHub Actions double-brace template syntax `${{ ... }}` is **not** interpolated by alint; it has to be rendered by Actions before the YAML is read, which only works in workflow files, not in `.alint.yml`. Use the single-brace `${VAR}` form and export the var in a workflow step.

#### `include_merges:`

In range mode, merge commits are skipped by default (`include_merges: false`). Merge subjects in PR contexts are typically `actions/checkout`-generated or maintainer-resolved and uninteresting. Set `include_merges: true` to lint them too. Has no effect when `since:` is unset; combining `include_merges: true` with no `since:` is a load-time error.

#### Failure modes

- **No git, or `git` not on PATH**: silent no-op. The rule's intent only makes sense inside a git repo.
- **`since:` ref doesn't resolve**: hard error with a shallow-clone hint. The common cause is `actions/checkout@v4` with its default `fetch-depth: 1`, which doesn't fetch the base ref's commits. Use `fetch-depth: 0` to fetch full history.
- **Range is empty** (`since` == HEAD on a force-push, or no non-merge commits): silent no-op. No commits, no policy to apply.

#### GitHub Actions PR-validation recipe

```yaml
# .github/workflows/lint.yml
name: lint
on:
  pull_request:
    branches: [main]
jobs:
  alint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          # Range mode walks <base>..HEAD. fetch-depth: 0 makes
          # both refs reachable; the default depth of 1 leaves
          # the base ref out of local objects and the rule errors.
          fetch-depth: 0
      - name: alint check
        env:
          ALINT_BASE_SHA: ${{ github.event.pull_request.base.sha }}
        uses: asamarts/alint@v0.9.21
```

