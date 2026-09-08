---
title: 'no_trailing_whitespace'
description: 'No line may end with space or tab. alint no_trailing_whitespace rule, text hygiene family.'
sidebar:
  order: 1
categories: ['text-hygiene']
---

No line may end with space or tab.

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Source files with trailing spaces or tabs

The rule fires on this repository:

```text
src/
src/bad.rs
src/good.rs
src/tabbed.md
```

```rust title="src/bad.rs"
fn bad() {}   
```

```rust title="src/good.rs"
fn good() {}
```

```markdown title="src/tabbed.md"
# Heading	
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-ws
    kind: no_trailing_whitespace
    paths: "src/**/*"
    level: error
```

`alint check` reports:

```ansi
[2m--- src/bad.rs -----------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-ws[0m
              [2m1:1[0m  trailing whitespace on line 1

[2m--- src/tabbed.md --------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-ws[0m
              [2m1:1[0m  trailing whitespace on line 1

[2mSummary (2 violations):[0m
  [1m[31mx 2 errors[0m
  0 passing [2m*[0m 1 failing
```

### Every source file is free of trailing whitespace

This repository is compliant:

```text
src/
src/one.rs
src/two.rs
```

```rust title="src/one.rs"
fn one() {}
```

```rust title="src/two.rs"
fn two() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-ws
    kind: no_trailing_whitespace
    paths: "src/**/*.rs"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

