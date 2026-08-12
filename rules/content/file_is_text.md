---
title: 'file_is_text'
description: 'Content is detected as text (magic bytes + UTF-8 validity check), fails on binary files matched by paths. alint file_is_text rule, content family.'
sidebar:
  order: 13
categories: ['content', 'encoding']
---

Content is detected as text (magic bytes + UTF-8 validity check) — fails on binary files matched by `paths`.

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A source file with a stray null byte

The rule fires on this repository:

```text
src/
src/accidental_blob.rs
src/clean.rs
```

`src/accidental_blob.rs`:

```text
(binary content, 11 bytes)
```

`src/clean.rs`:

```rust
pub fn ok() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: src-is-text
    kind: file_is_text
    paths: "src/**"
    level: error
```

### Every source file is UTF-8 text

This repository is compliant:

```text
src/
src/lib.rs
src/main.rs
```

`src/lib.rs`:

```rust
pub fn ok() {}
```

`src/main.rs`:

```rust
fn main() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: src-is-text
    kind: file_is_text
    paths: "src/**"
    level: error
```

