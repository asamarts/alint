---
title: 'dir_exists'
description: 'Directory counterpart of file_exists. alint dir_exists rule, existence family.'
sidebar:
  order: 3
categories: ['existence']
---

Directory counterpart of `file_exists`. Every match must correspond to a real directory in the walked tree.

**Optional `root_only: true`** (like `file_exists`) requires the match to be a
directory directly at the repository root, not nested.
**Optional `git_tracked_only: true`** further requires that the directory contain at least one tracked file. A tree with a `docs/` checked out from a stale clone where every file was later removed via `git rm` would fail under this stricter check. See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `git_tracked_only` | boolean |  | `false` | Restrict matches to directories that contain at least one git-tracked file. No effect outside a git repo. Default `false`. |
| `root_only` | boolean |  | `false` | If true, only a directory directly at the repository root satisfies the rule; a nested match does not. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A repository missing its src directory

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
  - id: has-src
    kind: dir_exists
    paths: "src"
    level: error
```

`alint check` reports:

```ansi
[2m--- Repository-level -----------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mhas-src[0m
              expected a directory matching [src]

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A repository with a src directory

This repository is compliant:

```text
Cargo.toml
src/
src/main.rs
```

`Cargo.toml`:

```toml
[package]
name = "demo"
```

`src/main.rs`:

```rust
fn main() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: has-src
    kind: dir_exists
    paths: "src"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

