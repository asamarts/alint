---
title: 'file_starts_with'
description: 'alint file_starts_with rule (content): Byte-level prefix / suffix check.'
sidebar:
  order: 4
categories: ['content']
---

Byte-level prefix / suffix check. Works on any bytes (binary safe, unlike `file_header`).

Check-only: a fix would risk silently duplicating a near-matching prefix. Pair with `file_prepend` / `file_append` explicitly if you want auto-repair.

**When to use it**: when the exact bytes matter more than the text. Typical prefixes are a shebang on scripts (`prefix: "#!"` on `**/*.sh`), a file signature or magic number, and a fixed licence or generated-file banner. Typical suffixes are a generator's closing sentinel, so a hand-edited or truncated output fails, and a mandatory trailer line.

**Byte-for-byte means newlines too**: a `suffix` ending in `\n` requires the file's final newline, and one without it fails on a file that has one. An empty file fails any non-empty prefix or suffix. For a pattern rather than fixed bytes, use `file_header` / `file_footer`, which match a regex against the first or last lines; for "ends with a newline" alone, [`final_newline`](/docs/rules/text-hygiene/final_newline/) is the dedicated check.

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

```rust title="src/no_header.rs"
fn x() {}
```

```rust title="src/ok.rs"
// SPDX-License-Identifier: MIT
fn main() {}
```

```rust title="src/wrong_spdx.rs"
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

```rust title="src/a.rs"
// SPDX-License-Identifier: MIT
fn main() {}
```

```rust title="src/b.rs"
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
