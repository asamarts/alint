---
title: 'no_empty_files'
description: 'no_empty_files rule in alint''s structure family.'
sidebar:
  order: 3
categories: ['structure']
---

Flag zero-byte files. Fixable via `file_remove`.

---

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Zero-byte placeholder files left in the tree

The rule fires on this repository:

```text
README.md
placeholder.rs
stray.log
```

`README.md`:

```markdown
# hi
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-empty
    kind: no_empty_files
    paths: "**"
    level: warning
```

### Every file carries at least one byte of content

This repository is compliant:

```text
README.md
src/
src/a.rs
```

`README.md`:

```markdown
# hi
```

`src/a.rs`:

```rust
fn a() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-empty
    kind: no_empty_files
    paths: "**"
    level: warning
```

