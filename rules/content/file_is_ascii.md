---
title: 'file_is_ascii'
description: 'Every byte in the file must be < 0x80 (pure ASCII), except codepoints listed in allow:. alint file_is_ascii rule, content family.'
sidebar:
  order: 14
categories: ['content', 'encoding', 'security-unicode-sanity']
---

Every byte in the file must be < 0x80 (pure ASCII), except codepoints listed in `allow:`. Strict variant of `is_text` for configs that must round-trip through strictly-ASCII tools. `allow:` exempts specific non-ASCII codepoints — each entry a single character (`"ö"`), a `U+XXXX` codepoint, or a `U+XXXX-U+YYYY` inclusive range (curl keeps its source ASCII but allows `ö` in "Björn"; the recurring need across llvm / vscode / elixir). With `allow:` the file is decoded as UTF-8 and checked per character; without it, the strict byte-level fast path is used.

---

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `allow` | list of string |  | `[]` | Permitted non-ASCII codepoints - each a single character (e.g. "o-umlaut"), a `U+XXXX` codepoint, or a `U+XXXX-U+YYYY` inclusive range. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A source file with a non-ASCII byte

The rule fires on this repository:

```text
src/
src/ascii.rs
src/unicode.rs
```

`src/ascii.rs`:

```rust
pub fn ok() {}
```

`src/unicode.rs`:

```rust
// The ☃ is here
pub fn cold() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: ascii-only
    kind: file_is_ascii
    paths: "src/**/*.rs"
    level: error
```

`alint check` reports:

```ansi
[2m--- src/unicode.rs -------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mascii-only[0m
              non-ASCII byte 0xE2 at offset 7

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every source file is pure ASCII

This repository is compliant:

```text
src/
src/a.rs
src/b.rs
```

`src/a.rs`:

```rust
pub fn a() {}
```

`src/b.rs`:

```rust
pub fn b() { let _ = 42; }
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: ascii-only
    kind: file_is_ascii
    paths: "src/**/*.rs"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

