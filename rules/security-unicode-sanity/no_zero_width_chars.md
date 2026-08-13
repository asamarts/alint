---
title: 'no_zero_width_chars'
description: 'Flag body-internal zero-width characters (U+200B, U+200C, U+200D, and non-leading U+FEFF). alint no_zero_width_chars rule, security / unicode sanity family.'
sidebar:
  order: 3
categories: ['security-unicode-sanity', 'encoding']
---

Flag body-internal zero-width characters (U+200B, U+200C, U+200D, and non-leading U+FEFF). A leading U+FEFF is `no_bom`'s concern.

As of v0.14 the detection set also covers U+2060 (word joiner) and U+180E (Mongolian vowel separator).

---

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### An identifier with a hidden zero-width space

The rule fires on this repository:

```text
src/
src/clean.rs
src/obfuscated.rs
```

`src/clean.rs`:

```rust
pub fn normal() {}
```

`src/obfuscated.rs`:

```rust
pub fn sec​ret() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-zwsp
    kind: no_zero_width_chars
    paths: "src/**/*.rs"
    level: error
```

### A file whose only zero-width codepoint is an allowed leading BOM

This repository is compliant:

```text
src/
src/clean.rs
src/with_bom.rs
```

`src/clean.rs`:

```rust
pub fn ok2() {}
```

`src/with_bom.rs`:

```rust
﻿pub fn ok() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-zwsp
    kind: no_zero_width_chars
    paths: "src/**/*.rs"
    level: error
```

