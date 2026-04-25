---
title: GitHub Actions
description: Run alint as a step in a GitHub Actions workflow.
sidebar:
  order: 1
---

The official Action wraps the `install.sh` flow plus alint invocation into one step.

## Inline PR annotations (default)

```yaml
- uses: asamarts/alint@v0.4.7
```

This runs `alint check --format github` against `.` and emits findings as `::error::` / `::warning::` workflow commands, which GitHub renders inline on the PR.

## Inputs (all optional)

```yaml
- uses: asamarts/alint@v0.4.7
  with:
    version: v0.4.7        # alint release tag (default: latest)
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
- uses: asamarts/alint@v0.4.7
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
- uses: asamarts/alint@<40-char-sha>  # v0.4.7
```

Look up the SHA on the [tag page](https://github.com/asamarts/alint/tags).
