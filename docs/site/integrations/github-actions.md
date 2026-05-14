---
title: GitHub Actions
description: Run alint as a step in a GitHub Actions workflow.
sidebar:
  order: 1
---

The official Action wraps the `install.sh` flow plus alint invocation into one step.

## Inline PR annotations (default)

```yaml
- uses: asamarts/alint@v0.9.20
```

This runs `alint check --format github` against `.` and emits findings as `::error::` / `::warning::` workflow commands, which GitHub renders inline on the PR.

## Inputs (all optional)

```yaml
- uses: asamarts/alint@v0.9.20
  with:
    version: v0.9.20        # alint release tag (default: latest)
    path: .                # directory to lint (default: .)
    format: github         # human | json | sarif | github (default)
    config: |              # extra config path(s), one per line
      .alint.yml
    fail-on-warning: false
    args: ""               # extra CLI args appended verbatim
```

## Upload findings to GitHub Code Scanning

Use `format: sarif` and pipe to the standard upload action:

```yaml
- uses: asamarts/alint@v0.9.20
  id: alint
  with:
    format: sarif
  continue-on-error: true
- uses: github/codeql-action/upload-sarif@v3
  if: always()
  with:
    sarif_file: ${{ steps.alint.outputs.sarif-file }}
```

`continue-on-error: true` is what lets the SARIF upload run even when alint finds issues — without it, a non-zero exit short-circuits the upload and the findings never reach Code Scanning.

## Pin to a SHA

For supply-chain hygiene (and to satisfy alint's own [`ci/github-actions@v1`](/docs/bundled-rulesets/) bundled ruleset), pin the action to a commit SHA:

```yaml
- uses: asamarts/alint@<40-char-sha>  # v0.9.20
```

Look up the SHA on the [tag page](https://github.com/asamarts/alint/tags).

## Validate PR commits with `git_commit_message`

`actions/checkout@v4` on a `pull_request` trigger checks out a synthetic merge commit, not the PR's tip. The HEAD subject is then auto-generated (`Merge <sha> into <sha>`) and trips any `git_commit_message` rule applied to HEAD only. To validate the PR's own commits instead, use the rule's `since:` option together with `actions/checkout`'s full-history fetch:

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
          # `since:` walks <base>..HEAD. fetch-depth: 0 makes the
          # base ref reachable; the default depth of 1 leaves it
          # out of local objects and the rule will hard-fail with
          # a shallow-clone hint.
          fetch-depth: 0
      - name: alint check
        env:
          ALINT_BASE_SHA: ${{ github.event.pull_request.base.sha }}
        uses: asamarts/alint@v0.9.20
```

The rule in `.alint.yml`:

```yaml
- id: pr-conventional-commits
  kind: git_commit_message
  pattern: '^(feat|fix|chore|docs|refactor|test|build|ci|perf|style|revert)(\(.+\))?!?: '
  subject_max_length: 72
  since: ${ALINT_BASE_SHA:-origin/main}
  level: error
```

The `${ALINT_BASE_SHA:-origin/main}` default makes the same config work locally too: when you run `alint check` on your feature branch without setting the env var, the rule falls back to `origin/main` and validates everything since you branched. See the [`git_commit_message` reference](/docs/rules/git-hygiene/git_commit_message/) for the full options surface.
