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

`src/bad.rs`:

```rust
fn bad() {}   
```

`src/good.rs`:

```rust
fn good() {}
```

`src/tabbed.md`:

```markdown
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

### Every source file is free of trailing whitespace

This repository is compliant:

```text
src/
src/one.rs
src/two.rs
```

`src/one.rs`:

```rust
fn one() {}
```

`src/two.rs`:

```rust
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

