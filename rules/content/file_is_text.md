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

```text title="src/accidental_blob.rs"
(binary content, 11 bytes)
```

```rust title="src/clean.rs"
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

`alint check` reports:

```ansi
[2m--- src/accidental_blob.rs -----------------------------------------------------[0m
  [1m[31mx  error  [0m  [2msrc-is-text[0m
              file is detected as binary; text is required here

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every source file is UTF-8 text

This repository is compliant:

```text
src/
src/lib.rs
src/main.rs
```

```rust title="src/lib.rs"
pub fn ok() {}
```

```rust title="src/main.rs"
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

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

