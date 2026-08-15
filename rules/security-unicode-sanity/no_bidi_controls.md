---
title: 'no_bidi_controls'
description: 'Flag Trojan-Source bidi override characters (U+202A 202E, U+2066 2069). alint no_bidi_controls rule, security / unicode sanity family.'
sidebar:
  order: 2
categories: ['security-unicode-sanity', 'encoding']
---

Flag Trojan-Source bidi override characters (U+202A–202E, U+2066–2069). Defense against [CVE-2021-42574](https://trojansource.codes/).

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A source file with a Trojan Source bidi override

The rule fires on this repository:

```text
src/
src/clean.rs
src/sneaky.rs
```

`src/clean.rs`:

```rust
pub fn ok() {}
```

`src/sneaky.rs`:

```rust
let comment = "<U+202E>gnitcepsnI ydobon emah sih";
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-bidi
    kind: no_bidi_controls
    paths: "src/**/*.rs"
    level: error
```

`alint check` reports:

```ansi
[2m--- src/sneaky.rs --------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-bidi[0m
              [2m1:16[0m  Unicode bidi control U+202E at line 1 col 16 (Trojan-Source
              defense)

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Source files with ordinary emoji and no bidi controls

This repository is compliant:

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
// A ☃ comment with emoji 🦀
pub fn fine() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-bidi
    kind: no_bidi_controls
    paths: "src/**/*.rs"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

