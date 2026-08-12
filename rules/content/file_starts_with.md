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

## See also

- [`file_ends_with`](/docs/rules/content/file_ends_with/)
