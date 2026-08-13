---
title: 'git_commit_message'
description: 'Validate commit-message shape via regex, max-subject-length, or required-body. alint git_commit_message rule, git hygiene family.'
sidebar:
  order: 5
categories: ['git-hygiene']
---

Validate commit-message shape via regex, max-subject-length, or required-body. At least one of the three must be set; combine all three for full Conventional-Commits-style enforcement. Subject length counts characters, not bytes (a 50-char emoji subject is 50, not 200).

Two modes, selected by the optional `since:` field:

- **HEAD-only** (default, `since:` omitted): validate the tip commit. Right shape for push-trigger CI and post-commit hooks.
- **Range** (`since:` set): validate every commit reachable from HEAD but not from `since`. Right shape for `pull_request`-trigger CI, where `actions/checkout` checks out a synthetic merge commit whose subject the rule would always flag.

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
        uses: asamarts/alint@v0.13.0
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `include_merges` | boolean |  | `false` | When validating a range (`since:` set), include merge commits. Defaults to `false` because merge commits in PR contexts are typically the synthetic merge `actions/checkout` produces (with an auto-generated subject the rule would always flag) or maintainer-resolved merges from the base branch. Has no effect when `since:` is unset; combining `include_merges: true` with no `since:` is a load-time error. |
| `pattern` | string |  | `null` | Rust-regex pattern the full message (subject + body, joined with newlines) must match. Use `(?s)` to make `.` match newlines. |
| `requires_body` | boolean |  | `false` | When true, the message must have a non-empty body, that is, at least one line of content after the subject's blank-line separator. |
| `since` | string |  | `null` | Git ref to use as the base of the commit range. When set, validates every commit in `<since>..HEAD` instead of just HEAD. Accepts anything `git rev-parse` does: SHA (full or abbreviated), branch (`origin/main`), tag (`v1.2.3`), or relative ref (`HEAD~5`). Supports POSIX `${VAR}` and `${VAR:-default}` env-var interpolation so CI can pass a SHA via an env var (e.g. `since: ${ALINT_BASE_SHA:-origin/main}` with `ALINT_BASE_SHA` exported in a workflow step from `github.event.pull_request.base.sha`). The GitHub Actions double-brace template syntax `${{ ... }}` is NOT interpolated by alint. |
| `subject_max_length` | integer (>= 1) |  | `null` | Maximum number of characters allowed in the subject line. Common values: 50 (Tim Pope's recommendation), 72 (GitHub PR-title cutoff). |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A commit with a non-conventional subject

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
  - id: conventional-commits
    kind: git_commit_message
    pattern: '^(feat|fix|chore): '
    level: error
```

committed with this history (oldest first):

```text
wip nonsense  (adds README.md)
```

`alint check` reports:

```ansi
[2m--- Repository-level -----------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mconventional-commits[0m
              commit HEAD: commit message does not match pattern
              `^(feat|fix|chore): ` (subject: "wip nonsense")

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A commit that follows the convention

This repository is compliant:

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
  - id: conventional-commits
    kind: git_commit_message
    pattern: '^(feat|fix|chore): '
    level: error
```

committed with this history (oldest first):

```text
feat: add the readme  (adds README.md)
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

