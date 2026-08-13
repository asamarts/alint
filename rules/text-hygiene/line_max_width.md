---
title: 'line_max_width'
description: 'Cap line length in characters (not bytes, code points). alint line_max_width rule, text hygiene family.'
sidebar:
  order: 4
categories: ['text-hygiene']
---

Cap line length in characters (not bytes — code points). Optional `tab_width` for tab expansion.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max_width` | integer (>= 1) | yes |  | Maximum number of Unicode scalar values (chars) allowed per line. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A source file with a line past the width limit

The rule fires on this repository:

```text
src/
src/ok.rs
src/wide.rs
```

`src/ok.rs`:

```rust
fn a() {}
fn b() {}
```

`src/wide.rs`:

```rust
fn x() {}
this line is intentionally way too long to be acceptable under the limit
fn y() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: narrow
    kind: line_max_width
    paths: "src/**/*.rs"
    max_width: 30
    level: warning
```

### Every line stays within the width limit

This repository is compliant:

```text
src/
src/a.rs
src/b.rs
```

`src/a.rs`:

```rust
fn a() {}
fn b() {}
```

`src/b.rs`:

```rust
pub fn c() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: narrow
    kind: line_max_width
    paths: "src/**/*.rs"
    max_width: 30
    level: warning
```

