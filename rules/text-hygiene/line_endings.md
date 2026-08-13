---
title: 'line_endings'
description: 'Every line ending matches target: lf or crlf. alint line_endings rule, text hygiene family.'
sidebar:
  order: 3
categories: ['text-hygiene', 'portable-metadata']
---

Every line ending matches `target`: `lf` or `crlf`. Mixed endings in a single file fail.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `target` | one of `lf` \| `crlf` | yes |  | Required line ending style: `lf` or `crlf`. Mixed endings within a file also fail. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A source file with CRLF line endings under an LF policy

The rule fires on this repository:

```text
src/
src/has_crlf.rs
src/pure_lf.rs
```

`src/has_crlf.rs`:

```rust
fn c() {}
fn d() {}
```

`src/pure_lf.rs`:

```rust
fn a() {}
fn b() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: lf-only
    kind: line_endings
    paths: "src/**/*.rs"
    target: lf
    level: error
```

`alint check` reports:

```ansi
[2m--- src/has_crlf.rs ------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mlf-only[0m
              [2m1:1[0m  line 1 does not use lf line endings

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every source file uses LF line endings

This repository is compliant:

```text
src/
src/a.rs
src/b.rs
```

`src/a.rs`:

```rust
fn a() {}
fn b() {}
```

`src/b.rs`:

```rust
fn c() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: lf-only
    kind: line_endings
    paths: "src/**/*.rs"
    target: lf
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

