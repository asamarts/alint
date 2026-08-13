---
title: 'git_commit_no_fixup'
description: 'Fail on residual fixup! / squash! / amend! commits left in scope, the ones git commit --fixup / --squash produce, meant to be collapsed by.'
sidebar:
  order: 7
categories: ['git-hygiene']
---

Fail on residual `fixup!` / `squash!` / `amend!` commits left in scope — the ones `git commit --fixup` / `--squash` produce, meant to be collapsed by `git rebase --autosquash` before merging. Forgetting to rebase is the universal case; this rule catches the leftover so it doesn't land on the main branch.

No configuration knobs — the matched subject prefixes are exactly what `--autosquash` understands. Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `include_merges` | boolean |  | `false` | When validating a range (`since:` set), include merge commits. Has no effect when `since:` is unset; combining `include_merges: true` with no `since:` is a load-time error. |
| `since` | string |  | `null` | Git ref to use as the base of the commit range. When set, validates every commit in `<since>..HEAD` instead of just HEAD. Accepts anything `git rev-parse` does. Use the canonical `{{env.X}}` interpolation to pass a SHA via an env var, e.g. `since: "{{env.ALINT_BASE_SHA \| default('origin/main')}}"`. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A leftover `fixup!` commit never autosquashed

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
  - id: no-fixup
    kind: git_commit_no_fixup
    level: error
```

committed with this history (oldest first):

```text
2024-01-15T09:00:00+00:00  fixup! feat: the original change
```

### A normal commit with no leftover fixup

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
  - id: no-fixup
    kind: git_commit_no_fixup
    level: error
```

committed with this history (oldest first):

```text
feat: a normal change
```

