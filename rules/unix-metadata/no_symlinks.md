---
title: 'no_symlinks'
description: 'Flag tracked paths that are symbolic links. alint no_symlinks rule, unix metadata family.'
sidebar:
  order: 1
categories: ['unix-metadata', 'portable-metadata', 'security-unicode-sanity']
---

Flag tracked paths that are symbolic links. Symlinks are a portability footgun: Windows NTFS needs admin rights to create them, git-for-Windows can silently flatten them, CI runners vary.

Caveat: a symlink whose target escapes the repository root (`link -> /etc`) is pruned by the walker *before* indexing, so it is **not** flagged — the rule reports in-tree symlinks (to files or directories), not escaping ones. The escaping symlink can't be read out-of-root either (path confinement blocks that), so this is a reporting gap, not a disclosure one; recording escaping symlinks safely is a tracked follow-up.

Fix: `file_remove` — unlinks the symlink; the target is untouched.

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A repository containing a symlink

The rule fires on this repository:

```text
README.md
latest.rs -> src/main.rs
src/
src/main.rs
```

`README.md`:

```markdown
# demo
```

`src/main.rs`:

```rust
fn main() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-links
    kind: no_symlinks
    paths: "**"
    level: error
```

### A repository with no symlinks

This repository is compliant:

```text
README.md
src/
src/lib.rs
src/main.rs
```

`README.md`:

```markdown
# demo
```

`src/main.rs`:

```rust
fn main() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-links
    kind: no_symlinks
    paths: "**"
    level: error
```

