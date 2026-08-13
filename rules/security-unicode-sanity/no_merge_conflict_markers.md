---
title: 'no_merge_conflict_markers'
description: 'Flag <<<<<<<, =======, >>>>>>>, ||||||| markers at the start of a line, almost always left over from an unresolved merge.'
sidebar:
  order: 1
categories: ['security-unicode-sanity', 'text-hygiene']
---

Flag `<<<<<<< `, `=======`, `>>>>>>> `, `||||||| ` markers at the start of a line — almost always left over from an unresolved merge. The anchor markers carry a trailing ref (`<<<<<<< HEAD`), so they never collide with prose; a bare `=======` is reported only when the file also contains one of those anchors, because on its own a seven-character `=======` is indistinguishable from a reST/Markdown setext heading underline (so docs trees no longer need to be excluded).

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A source file with unresolved merge conflict markers

The rule fires on this repository:

```text
src/
src/clean.rs
src/half_merged.rs
```

`src/clean.rs`:

```rust
pub fn ok() {}
```

`src/half_merged.rs`:

```rust
<<<<<<< HEAD
ours
=======
theirs
>>>>>>> feature
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-conflict
    kind: no_merge_conflict_markers
    paths: "src/**/*"
    level: error
```

### Source files with no leftover conflict markers

This repository is compliant:

```text
src/
src/a.rs
src/docs.md
```

`src/a.rs`:

```rust
pub fn a() {}
```

`src/docs.md`:

```markdown
Conflict discussion: we use <<<<<< for ... (inline, no col-1 marker)
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-conflict
    kind: no_merge_conflict_markers
    paths: "src/**/*"
    level: error
```

