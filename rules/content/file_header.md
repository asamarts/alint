---
title: 'file_header'
description: 'alint file_header rule (content): The first N lines must match a regex (line-oriented).'
sidebar:
  order: 3
categories: ['content']
---

The first N lines must match a regex (line-oriented). For a byte-level prefix check, prefer `file_starts_with`.

Fix: `file_prepend` — inject declared content at the top, at BOF (after any UTF-8 BOM, which it preserves). Blind to a shebang / XML declaration (it would push either off line 1) -- use `insert_header` when that matters.

Fix: `insert_header` — the position-aware alternative to `file_prepend` (same `content` / `content_from`): inserts the header AFTER a leading UTF-8 BOM, shebang (`#!...`), or XML declaration (`<?xml ...?>`), so it never displaces a line that must stay first (a kernel reads a shebang only on line 1; an XML parser needs the declaration first). For a file with none of those prefixes it inserts at BOF, exactly like `file_prepend`. **`Safe` by default** (the insertion point is the one canonical header spot and the content is inert -- at least as safe as the `Safe` `file_prepend` it refines, and safer on a file with a shebang / XML declaration); demoted to a suggestion from an untrusted remote `extends:` (the header bytes are ruleset-authored). Its idempotency guard anchors on both the insertion point and the file top, so a repeated fix is a guaranteed no-op even when the header content itself begins with a `#!` / `<?xml` prefix. Because the header can land below line 1, the rule's `pattern` must be able to match below the first line: use an unanchored pattern (or `(?m)`); a `^`-anchored pattern that only matches line 1 cannot be satisfied once the header sits under a shebang.

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

```rust title="src/a.rs"
fn a() {}
```

```rust title="src/b.rs"
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

```rust title="src/a.rs"
// Copyright 2026
fn a() {}
```

```rust title="src/b.rs"
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

