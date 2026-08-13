---
title: 'file_starts_with'
description: 'Byte-level prefix / suffix check. alint file_starts_with rule, content family.'
sidebar:
  order: 4
categories: ['content']
---

Byte-level prefix / suffix check. Works on any bytes (binary safe, unlike `file_header`).

Check-only: a fix would risk silently duplicating a near-matching prefix. Pair with `file_prepend` / `file_append` explicitly if you want auto-repair.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `prefix` | string | yes |  | Required prefix, matched byte-for-byte. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Files missing the required SPDX prefix

The rule fires on this repository:

```text
src/
src/no_header.rs
src/ok.rs
src/wrong_spdx.rs
```

`src/no_header.rs`:

```rust
fn x() {}
```

`src/ok.rs`:

```rust
// SPDX-License-Identifier: MIT
fn main() {}
```

`src/wrong_spdx.rs`:

```rust
// SPDX-License-Identifier: GPL-3.0
fn y() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: spdx-header
    kind: file_starts_with
    paths: "src/**/*.rs"
    prefix: "// SPDX-License-Identifier: MIT\n"
    level: error
```

`alint check` reports:

```ansi
[2m--- src/no_header.rs -----------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mspdx-header[0m
              [2m1:1[0m  file does not start with the required prefix

[2m--- src/wrong_spdx.rs ----------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mspdx-header[0m
              [2m1:1[0m  file does not start with the required prefix

[2mSummary (2 violations):[0m
  [1m[31mx 2 errors[0m
  0 passing [2m*[0m 1 failing
```

### Every file begins with the SPDX prefix

This repository is compliant:

```text
src/
src/a.rs
src/b.rs
```

`src/a.rs`:

```rust
// SPDX-License-Identifier: MIT
fn main() {}
```

`src/b.rs`:

```rust
// SPDX-License-Identifier: MIT
fn x() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: spdx-header
    kind: file_starts_with
    paths: "src/**/*.rs"
    prefix: "// SPDX-License-Identifier: MIT\n"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

## See also

- [`file_ends_with`](/docs/rules/content/file_ends_with/)
