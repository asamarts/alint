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

`alint check` reports:

```ansi
[2m--- src/main.rs ----------------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2mno-dbg[0m
              [2m2:1[0m  forbidden pattern /\bdbg!\s*\(/ found

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
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

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

