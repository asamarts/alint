---
title: 'file_footer'
description: 'Last lines lines of each file in scope must match a regex. alint file_footer rule, content family.'
sidebar:
  order: 11
categories: ['content']
---

Last `lines` lines of each file in scope must match a regex. Mirror of `file_header` anchored at the end of the file. Use for license footers, signed-off-by trailers, generated-file sentinels.

Fix: `file_append` — append a declared `content`. With no fix declared, violations are unfixable.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `lines` | integer (>= 1) |  | `20` | Number of trailing lines to consider. |
| `pattern` | string | yes |  | Rust regex. The last `lines` lines of each file must match. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A file missing its license footer

The rule fires on this repository:

```text
src/
src/missing.rs
src/ok.rs
```

`src/missing.rs`:

```rust
fn b() {}
// no footer here
```

`src/ok.rs`:

```rust
fn a() {}
// Licensed under the Apache License, Version 2.0
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: license-footer
    kind: file_footer
    paths: "src/**/*.rs"
    pattern: "Licensed under the Apache License"
    lines: 3
    level: warning
```

`alint check` reports:

```ansi
[2m--- src/missing.rs -------------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2mlicense-footer[0m
              last 3 line(s) do not match required footer /Licensed under the
              Apache License/

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### Every file ends with the license footer

This repository is compliant:

```text
src/
src/a.rs
src/b.rs
```

`src/a.rs`:

```rust
fn a() {}

// Licensed under the Apache License, Version 2.0
```

`src/b.rs`:

```rust
fn b() {}
// Licensed under the Apache License, Version 2.0
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: license-footer
    kind: file_footer
    paths: "src/**/*.rs"
    pattern: "Licensed under the Apache License"
    lines: 3
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

