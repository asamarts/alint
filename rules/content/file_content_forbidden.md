---
title: 'file_content_forbidden'
description: 'File contents must NOT match a regex. alint file_content_forbidden rule, content family.'
sidebar:
  order: 2
categories: ['content', 'security-unicode-sanity']
---

File contents must NOT match a regex.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `pattern` | string | yes |  | Rust regex. File contents must NOT match. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Source that left a debug macro in

The rule fires on this repository:

```text
src/
src/clean.rs
src/main.rs
```

`src/clean.rs`:

```rust
pub fn ok() {}
```

`src/main.rs`:

```rust
fn main() {
    dbg!(42);
}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-dbg
    kind: file_content_forbidden
    paths: "src/**/*.rs"
    pattern: '\bdbg!\s*\('
    level: warning
```

### Source with no forbidden macros

This repository is compliant:

```text
src/
src/clean.rs
src/main.rs
```

`src/clean.rs`:

```rust
pub fn ok() {}
```

`src/main.rs`:

```rust
fn main() {
    println!("hi");
}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-dbg
    kind: file_content_forbidden
    paths: "src/**/*.rs"
    pattern: '\bdbg!\s*\('
    level: warning
```

