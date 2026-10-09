---
title: 'file_max_lines'
description: 'alint file_max_lines rule (content): File must have at most max_lines lines, using the same accounting as file_min_lines.'
sidebar:
  order: 10
categories: ['content', 'structure']
---

File must have at most `max_lines` lines, using the same accounting as `file_min_lines`. Catches the everything-module anti-pattern — a `lib.rs` / `index.ts` / `helpers.py` that grew unbounded.

**Choosing a limit**: set it just above the largest file you accept today, as a ratchet rather than an ideal, and lower it as modules get split. Scope it to hand-written sources (`paths: "src/**/*.rs"`) and leave out lockfiles, generated code, fixtures and vendored files, which are long by nature. A warning level suits an advisory size budget; an error suits a hard cap.

**Lines and bytes are different budgets**: a minified bundle can be one enormous line, which `file_max_lines` passes. Pair it with `file_max_size` when the concern is repository weight rather than readability.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max_lines` | integer (>= 0) | yes |  | Maximum allowed line count. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A source file over the line cap

The rule fires on this repository:

```text
src/
src/bloated.rs
src/tiny.rs
```

```rust title="src/bloated.rs"
fn a() {}
fn b() {}
fn c() {}
fn d() {}
fn e() {}
fn f() {}
fn g() {}
```

```rust title="src/tiny.rs"
fn a() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: cap-source-file
    kind: file_max_lines
    paths: "src/**/*.rs"
    max_lines: 5
    level: warning
```

`alint check` reports:

```ansi
[2m--- src/bloated.rs -------------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2mcap-source-file[0m
              file has 7 line(s); at most 5 allowed

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### A source file within the line cap

This repository is compliant:

```text
src/
src/medium.rs
src/tiny.rs
```

```rust title="src/medium.rs"
fn a() {}
fn b() {}
fn c() {}
fn d() {}
fn e() {}
```

```rust title="src/tiny.rs"
fn a() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: cap-source-file
    kind: file_max_lines
    paths: "src/**/*.rs"
    max_lines: 5
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

