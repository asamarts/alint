---
title: 'file_content_forbidden'
description: 'alint file_content_forbidden rule (content): File contents must NOT match a regex.'
sidebar:
  order: 2
categories: ['content', 'security-unicode-sanity']
---

File contents must NOT match a regex.

The pattern is matched against the raw bytes, so a file that is not valid UTF-8 (a stray Latin-1 byte, say) is still searched rather than skipped; an invalid byte simply never matches a Unicode class. The `replace` fix edits such a file at the same byte offsets and leaves every other byte intact. A binary-looking file that is not valid UTF-8 (an image, a font, an archive) is still skipped, and the `replace` fix never edits a binary-looking file (a valid-UTF-8 file with a NUL byte is searched, but its finding is reported as not auto-fixable).

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

```rust title="src/clean.rs"
pub fn ok() {}
```

```rust title="src/main.rs"
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

```rust title="src/clean.rs"
pub fn ok() {}
```

```rust title="src/main.rs"
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

