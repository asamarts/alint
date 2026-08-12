---
title: 'file_max_lines'
description: 'File must have at most max_lines lines, using the same accounting as file_min_lines. alint file_max_lines rule, content family.'
sidebar:
  order: 10
categories: ['content', 'structure']
---

File must have at most `max_lines` lines, using the same accounting as `file_min_lines`. Catches the everything-module anti-pattern — a `lib.rs` / `index.ts` / `helpers.py` that grew unbounded.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max_lines` | integer (>= 0) | yes |  | Maximum allowed line count. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A source file over the line cap

The rule fires on this repository:

```text
src/
src/bloated.rs
src/tiny.rs
```

`src/bloated.rs`:

```rust
fn a() {}
fn b() {}
fn c() {}
fn d() {}
fn e() {}
fn f() {}
fn g() {}
```

`src/tiny.rs`:

```rust
fn a() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: cap-source-file
    kind: file_max_lines
    paths: "src/**/*.rs"
    max_lines: 5
    level: warning
```

### A source file within the line cap

This repository is compliant:

```text
src/
src/medium.rs
src/tiny.rs
```

`src/medium.rs`:

```rust
fn a() {}
fn b() {}
fn c() {}
fn d() {}
fn e() {}
```

`src/tiny.rs`:

```rust
fn a() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: cap-source-file
    kind: file_max_lines
    paths: "src/**/*.rs"
    max_lines: 5
    level: warning
```

