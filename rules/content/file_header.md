---
title: 'file_header'
description: 'The first N lines must match a regex (line-oriented). alint file_header rule, content family.'
sidebar:
  order: 3
categories: ['content']
---

The first N lines must match a regex (line-oriented). For a byte-level prefix check, prefer `file_starts_with`.

Fix: `file_prepend` — inject declared content at the top (preserves UTF-8 BOM).

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `lines` | integer (>= 1) |  | `20` | Number of leading lines to consider. |
| `pattern` | string | yes |  | Rust regex. The first `lines` lines of each file in scope must match. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A source file missing its copyright header

The rule fires on this repository:

```text
src/
src/a.rs
src/b.rs
```

`src/a.rs`:

```rust
fn a() {}
```

`src/b.rs`:

```rust
// Copyright 2026
// SPDX-License-Identifier: Apache-2.0
fn b() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: copyright-header
    kind: file_header
    paths: "src/**/*.rs"
    pattern: "(?s)Copyright"
    lines: 3
    level: error
```

`alint check` reports:

```ansi
[2m--- src/a.rs -------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mcopyright-header[0m
              [2m1:1[0m  first 3 line(s) do not match required header /(?s)Copyright/

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every source file carries the header

This repository is compliant:

```text
src/
src/a.rs
src/b.rs
```

`src/a.rs`:

```rust
// Copyright 2026
fn a() {}
```

`src/b.rs`:

```rust
// Copyright 2026
// SPDX-License-Identifier: Apache-2.0
fn b() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: copyright-header
    kind: file_header
    paths: "src/**/*.rs"
    pattern: "(?s)Copyright"
    lines: 3
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

