---
title: 'file_exists'
description: 'Every glob match in paths must correspond to a real file. alint file_exists rule, existence family.'
sidebar:
  order: 1
categories: ['existence']
---

Every glob match in `paths` must correspond to a real file. Use an array to accept any of several names.

Fix: `file_create` — write a declared `content`. With an array of `paths`, the fix creates the first entry.

**Optional `git_tracked_only: true`** further requires that the matching file be in git's index — useful for rules like "every release must commit a CHANGELOG entry" where local-only files shouldn't satisfy the requirement. Outside a git repo, the rule fails (no file qualifies). See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `git_tracked_only` | boolean |  | `false` | Restrict matches to files tracked in git's index: entries present in the walked tree but not in `git ls-files` are skipped. No effect outside a git repo. Default `false`. |
| `respect_gitignore` | boolean |  | `null` | Per-rule override for the workspace `respect_gitignore` setting. When `false`, this rule's literal-path checks also stat the filesystem directly, so it sees files that are tracked AND `.gitignore`-masked (the bazel-style `.bazelversion` pattern — pitfall #18 in `docs/development/CONFIG-AUTHORING.md`). Honoured only by `file_exists` literal paths; glob patterns fall through to the workspace setting. Default: inherit the workspace `respect_gitignore`. |
| `root_only` | boolean |  | `false` | If true, only files directly at the repository root satisfy the rule. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A repository missing its README

The rule fires on this repository:

```text
Cargo.toml
```

`Cargo.toml`:

```toml
[package]
name = "demo"
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: has-readme
    kind: file_exists
    paths: README.md
    root_only: true
    level: error
```

`alint check` reports:

```ansi
[2m--- Repository-level -----------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mhas-readme[0m
              expected a file matching [README.md] at the repo root

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A repository with the README present

This repository is compliant:

```text
Cargo.toml
README.md
```

`Cargo.toml`:

```toml
[package]
name = "demo"
```

`README.md`:

```markdown
# demo
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: has-readme
    kind: file_exists
    paths: README.md
    root_only: true
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

