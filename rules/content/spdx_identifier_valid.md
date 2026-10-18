---
title: 'spdx_identifier_valid'
description: 'alint spdx_identifier_valid rule (content): Validate every SPDX-License-Identifier: value as a strict SPDX license expression.'
sidebar:
  order: 20
categories: ['content', 'security-unicode-sanity']
---

Validate every `SPDX-License-Identifier:` value as a strict SPDX license expression.

The offline SPDX License List 3.29.0 table recognizes current licenses and exceptions; `AND`, `OR`, `WITH`, and parentheses follow SPDX precedence, and valid `LicenseRef-*`, `AdditionRef-*`, and document-qualified custom references are accepted. Unknown, malformed, and deprecated standard identifiers fail. Files with no tag are silent—pair this kind with `file_content_matches` when a tag is mandatory. Binary-looking files are skipped. Check-only because selecting or changing a license is not a mechanical formatting decision.

---

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, `when`, `scope_filter`, and `expect_matches` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### An unknown identifier cannot masquerade as an SPDX expression

The rule fires on this repository:

```text
src/lib.rs
```

```rust title="src/lib.rs"
// SPDX-License-Identifier: Apach-2.0 OR MIT

pub fn answer() -> u8 { 42 }
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: valid-spdx
    kind: spdx_identifier_valid
    paths: "src/**/*.rs"
    level: error
```

`alint check` reports:

```ansi
[2m--- src/lib.rs -----------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mvalid-spdx[0m
              [2m1:29[0m  invalid SPDX license expression: Apach-2.0 OR MIT
              ^^^^^^^^^ unknown term

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A current license expression and exception are accepted

This repository is compliant:

```text
src/lib.rs
```

```rust title="src/lib.rs"
// SPDX-License-Identifier: MIT OR Apache-2.0 WITH LLVM-exception

pub fn answer() -> u8 { 42 }
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: valid-spdx
    kind: spdx_identifier_valid
    paths: "src/**/*.rs"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

