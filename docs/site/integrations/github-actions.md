---
title: GitHub Actions
description: Run alint as a step in a GitHub Actions workflow.
sidebar:
  order: 1
---

The official Action wraps the `install.sh` flow plus alint invocation into one step.

**Runs on Linux and macOS runners only.** The Action wraps `install.sh` (shell-based), so on `windows-latest` it fails with `unsupported platform`. For Windows CI, install alint in a prior `run:` step (`pip install alint`, `npm install -g @asamarts/alint`, or `cargo install alint`) and invoke `alint` directly.

<likec4-view view-id="ciActionFlow"></likec4-view>

## Inline PR annotations (default)

```yaml
- uses: asamarts/alint@d93c0283b19dd78afcd8a4b303f1556a7759ba81 # v0.17.0
  with:
    version: v0.17.0
```

This runs `alint check --format github` against `.` and emits findings as `::error::` / `::warning::` workflow commands, which GitHub renders inline on the PR.

## Inputs (all optional)

```yaml
- uses: asamarts/alint@d93c0283b19dd78afcd8a4b303f1556a7759ba81 # v0.17.0
  with:
    version: v0.17.0       # required for this release's SHA pin; see below
    path: .                # directory to lint (default: .)
    working-directory: .   # dir the action runs in (default: the runner workspace)
    format: github         # human | json | sarif | github | markdown | junit | gitlab | agent
    config: .alint.yml     # one extra config; compose more with extends:
    fail-on-warning: false
    args: ""               # extra CLI args appended verbatim
```

## Upload findings to GitHub Code Scanning

Use `format: sarif` and pipe to the standard upload action:

```yaml
- uses: asamarts/alint@d93c0283b19dd78afcd8a4b303f1556a7759ba81 # v0.17.0
  id: alint
  with:
    version: v0.17.0
    format: sarif
  continue-on-error: true
- uses: github/codeql-action/upload-sarif@2892aa5e19bbd11bc0cff5427e3b750a04d9e3c2 # v4.38.2
  if: always()
  with:
    sarif_file: ${{ steps.alint.outputs.sarif-file }}
```

`continue-on-error: true` is what lets the SARIF upload run even when alint finds issues — without it, a non-zero exit short-circuits the upload and the findings never reach Code Scanning.

alint's SARIF carries a stable [`partialFingerprints`](/docs/reference/output-formats/#stable-fingerprints) identity on every run, so Code Scanning correlates each alert across runs — deduping, and tracking a finding as fixed or reopened — with no `--baseline` needed.

## Pin to a SHA

For supply-chain hygiene (and to satisfy alint's own [`ci/github-actions@v1`](/docs/bundled-rulesets/) bundled ruleset), pin the action to a commit SHA:

```yaml
- uses: asamarts/alint@d93c0283b19dd78afcd8a4b303f1556a7759ba81 # v0.17.0
  with:
    version: v0.17.0
```

Look up the SHA on the [tag page](https://github.com/asamarts/alint/tags). The
v0.17.0 Action predates the baked-version default, so its SHA needs the explicit
`version: v0.17.0` shown above to pin the binary as well as the Action code. The
next release carries its matching binary version in `action.yml`; from that
release onward, the SHA alone pins both, and `version:` is only needed to choose
a different binary release intentionally.

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
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          # `since:` walks <base>..HEAD. fetch-depth: 0 makes the
          # base ref reachable; the default depth of 1 leaves it
          # out of local objects and the rule will hard-fail with
          # a shallow-clone hint.
          fetch-depth: 0
      - name: alint check
        env:
          ALINT_BASE_SHA: ${{ github.event.pull_request.base.sha }}
        uses: asamarts/alint@d93c0283b19dd78afcd8a4b303f1556a7759ba81 # v0.17.0
        with:
          version: v0.17.0
```

The rule in `.alint.yml`:

```yaml
- id: pr-conventional-commits
  kind: git_commit_message
  pattern: '^(feat|fix|chore|docs|refactor|test|build|ci|perf|style|revert)(\(.+\))?!?: '
  subject_max_length: 72
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

The `{{env.ALINT_BASE_SHA | default('origin/main')}}` default makes the same config work locally too: when you run `alint check` on your feature branch without setting the env var, the rule falls back to `origin/main` and validates everything since you branched. See the [`git_commit_message` reference](/docs/rules/git-hygiene/git_commit_message/) and [variable interpolation](/docs/configuration/variable-interpolation/) for the full surface. (The older POSIX `since: ${ALINT_BASE_SHA:-origin/main}` form still works but is deprecated — alint prints a one-line migration hint at load.)
